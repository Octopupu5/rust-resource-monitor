use crate::metrics::{ErrorResponse, RpcMetricsSnapshot};
use crate::storage::MetricsBuffer;
use crate::web;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct AppState {
    pub buffer: Arc<MetricsBuffer>,
    pub stream_tx: broadcast::Sender<RpcMetricsSnapshot>,
    pub shutdown: CancellationToken,
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct RangeQuery {
    pub from_ts: u64,
    pub to_ts: u64,
}

fn api_routes() -> Router<AppState> {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/latest", get(get_latest))
        .route("/api/metrics", get(get_latest))
        .route("/api/range", get(get_range))
        .route("/api/history", get(get_history))
        .route("/api/stream", get(stream))
}

/// API-only router: no web page (used by server)
pub fn api_only_router(state: AppState) -> Router {
    api_routes().with_state(state)
}

/// Full router: API endpoints + web page (used by client)
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .merge(api_routes())
        .with_state(state)
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(HealthResponse { status: "ok" })).into_response()
}

async fn index() -> impl IntoResponse {
    web::index().await
}

async fn get_latest(State(state): State<AppState>) -> impl IntoResponse {
    match state.buffer.latest() {
        Some(snap) => (StatusCode::OK, Json(snap.to_rpc_format())).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "no data yet".to_string(),
            }),
        )
            .into_response(),
    }
}

async fn get_range(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<RangeQuery>,
) -> impl IntoResponse {
    let from = query.from_ts as u128;
    let to = query.to_ts as u128;

    let snapshots: Vec<RpcMetricsSnapshot> = state
        .buffer
        .history(None)
        .into_iter()
        .filter(|s| s.timestamp_ms >= from && s.timestamp_ms <= to)
        .map(|s| s.to_rpc_format())
        .collect();

    (StatusCode::OK, Json(snapshots)).into_response()
}

async fn get_history(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<HistoryQuery>,
) -> impl IntoResponse {
    let all = state.buffer.history(None);

    let history: Vec<RpcMetricsSnapshot> = if let Some(limit) = query.limit {
        let len = all.len();
        let take = limit.min(len);
        all.into_iter()
            .skip(len - take)
            .map(|s| s.to_rpc_format())
            .collect()
    } else {
        all.into_iter().map(|s| s.to_rpc_format()).collect()
    };

    (StatusCode::OK, Json(history)).into_response()
}

async fn stream(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.stream_tx.subscribe();
    let shutdown = state.shutdown.clone();
    let stream = BroadcastStream::new(rx)
        .take_until(async move { shutdown.cancelled().await })
        .map(|msg| match msg {
            Ok(snapshot) => match serde_json::to_string(&snapshot) {
                Ok(json) => Ok(Event::default().data(json)),
                Err(e) => Ok(Event::default()
                    .event("error")
                    .data(format!("serialize_error: {e}"))),
            },
            Err(e) => Ok(Event::default()
                .event("error")
                .data(format!("stream_error: {e}"))),
        });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(10))
            .text("keep-alive"),
    )
}
