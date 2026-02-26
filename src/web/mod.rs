use axum::response::Html;
use axum::response::IntoResponse;

mod templates;

pub async fn index() -> impl IntoResponse {
    Html(templates::render_index())
}
