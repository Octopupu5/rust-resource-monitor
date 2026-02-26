use axum::response::Html;
use axum::response::IntoResponse;

mod templates;
mod widgets;
pub use widgets::*;

pub async fn index() -> impl IntoResponse {
    Html(templates::render_index())
}
