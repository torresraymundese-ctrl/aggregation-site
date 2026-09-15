use axum::{
    routing::{get, post},
    Router,
};

pub fn auth_router() -> Router {
    Router::new()
        .route("/register", post(super::handlers::auth_handler::register))
        .route("/login", post(super::handlers::auth_handler::login))
        .route("/logout", post(super::handlers::auth_handler::logout))
        .route("/me", get(super::handlers::auth_handler::me))
}