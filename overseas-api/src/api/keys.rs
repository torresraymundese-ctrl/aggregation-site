use axum::{
    routing::{get, post},
    Router,
};

pub fn keys_router() -> Router {
    Router::new()
        .route("/", post(super::handlers::api_key_handler::create))
        .route("/", get(super::handlers::api_key_handler::list))
}