use std::env;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub fn init_logging() {
    let log_level = env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());

    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&log_level));

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(true).with_thread_ids(true))
        .with(env_filter)
        .init();
}
