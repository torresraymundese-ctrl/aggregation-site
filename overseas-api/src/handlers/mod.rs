pub mod user_handler;
pub mod key_handler;
pub mod packages;
pub mod orders;
pub mod balance;
pub mod gateway;
pub mod admin;
pub mod auth;
pub mod keys;

pub fn all_routes() -> axum::Router {
    use axum::Router;
    Router::new()
        .merge(auth::router())
        .merge(keys::router())
        .merge(packages::router())
        .merge(orders::router())
        .merge(balance::router())
        .merge(gateway::router())
        .merge(admin::router())
}