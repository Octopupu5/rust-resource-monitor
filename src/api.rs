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
    pub since_ms: Option<u64>,
    pub until_ms: Option<u64>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/health", get(health))
        .route("/api/metrics", get(get_latest))
        .route("/api/history", get(get_history))
        .route("/api/stream", get(stream))
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

async fn get_history(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<HistoryQuery>,
) -> impl IntoResponse {
    let limit = query.limit;
    let since_ms = query.since_ms;
    let until_ms = query.until_ms;

    let history: Vec<RpcMetricsSnapshot> = state
        .buffer
        .history(None)
        .into_iter()
        .filter(|s| {
            since_ms
                .map(|ts| s.timestamp_ms >= ts as u128)
                .unwrap_or(true)
        })
        .filter(|s| {
            until_ms
                .map(|ts| s.timestamp_ms <= ts as u128)
                .unwrap_or(true)
        })
        .map(|s| s.to_rpc_format())
        .collect();

    let history = if let Some(limit) = limit {
        let len = history.len();
        let take = limit.min(len);
        history.into_iter().skip(len - take).collect()
    } else {
        history
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
