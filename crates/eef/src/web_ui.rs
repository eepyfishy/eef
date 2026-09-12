//! Optional browser routes; no coordinator operations live here.
use crate::Runtime;
use axum::{
    Router,
    response::{Html, IntoResponse},
    routing::get,
};
use std::sync::Arc;
pub(crate) fn router() -> Router<Arc<Runtime>> {
    Router::new()
        .route("/", get(root))
        .route("/app.js", get(ui_script))
        .route("/app.css", get(ui_style))
        .route("/advanced/legacy", get(legacy))
}
async fn root() -> Html<&'static str> {
    Html(include_str!("../../../dashboard/app.html"))
}
async fn legacy() -> Html<&'static str> {
    Html(include_str!("../../../dashboard/index.html"))
}
async fn ui_script() -> impl IntoResponse {
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/javascript; charset=utf-8",
        )],
        include_str!("../../../dashboard/app.js"),
    )
}
async fn ui_style() -> impl IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../../../dashboard/app.css"),
    )
}
