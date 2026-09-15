pub mod codes {
    pub use axum::http::StatusCode;

    #[allow(dead_code)]
    pub const SUCCESS: (i32, StatusCode, &str) = (0, StatusCode::OK, "success");
    #[allow(dead_code)]
    pub const PARAM_ERROR: (i32, StatusCode, &str) = (1001, StatusCode::BAD_REQUEST, "参数错误");
    #[allow(dead_code)]
    pub const NOT_FOUND: (i32, StatusCode, &str) = (2001, StatusCode::NOT_FOUND, "资源不存在");
    #[allow(dead_code)]
    pub const ALREADY_EXISTS: (i32, StatusCode, &str) = (2003, StatusCode::CONFLICT, "资源已存在");
    #[allow(dead_code)]
    pub const NOT_LOGIN: (i32, StatusCode, &str) = (3001, StatusCode::UNAUTHORIZED, "未登录");
    #[allow(dead_code)]
    pub const TOKEN_INVALID: (i32, StatusCode, &str) =
        (3002, StatusCode::UNAUTHORIZED, "Token无效");
    #[allow(dead_code)]
    pub const FORBIDDEN: (i32, StatusCode, &str) = (4001, StatusCode::FORBIDDEN, "无权访问");
    #[allow(dead_code)]
    pub const SERVER_ERROR: (i32, StatusCode, &str) =
        (5000, StatusCode::INTERNAL_SERVER_ERROR, "系统错误");
}

use axum::{http::StatusCode, response::IntoResponse, response::Response, Json};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum AppError {
    #[error("参数错误: {0}")]
    ParamError(String),

    #[error("资源不存在: {0}")]
    NotFound(String),

    #[error("资源已存在: {0}")]
    AlreadyExists(String),

    #[error("未登录")]
    NotLogin,

    #[error("Token无效")]
    TokenInvalid,

    #[error("Token过期")]
    TokenExpired,

    #[error("无权访问: {0}")]
    Forbidden(String),

    #[error("系统错误: {0}")]
    ServerError(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            AppError::ParamError(msg) => (StatusCode::BAD_REQUEST, 1001, msg.clone()),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, 2001, msg.clone()),
            AppError::AlreadyExists(msg) => (StatusCode::CONFLICT, 2003, msg.clone()),
            AppError::NotLogin => (StatusCode::UNAUTHORIZED, 3001, "未登录".to_string()),
            AppError::TokenInvalid => (StatusCode::UNAUTHORIZED, 3002, "Token无效".to_string()),
            AppError::TokenExpired => (StatusCode::UNAUTHORIZED, 3003, "Token过期".to_string()),
            AppError::Forbidden(msg) => (StatusCode::FORBIDDEN, 4001, msg.clone()),
            AppError::ServerError(msg) => {
                tracing::error!(error = %msg, "Application operation failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    5000,
                    "系统错误，请稍后重试".to_string(),
                )
            }
        };

        (
            status,
            Json(json!({
                "code": code,
                "message": message,
            })),
        )
            .into_response()
    }
}

#[allow(dead_code)]
impl AppError {
    pub fn param_error(msg: impl Into<String>) -> Self {
        AppError::ParamError(msg.into())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        AppError::NotFound(msg.into())
    }

    pub fn not_login() -> Self {
        AppError::NotLogin
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::ServerError(e.to_string())
    }
}
