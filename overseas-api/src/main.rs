mod async_media;
mod budget;
mod byok;
mod config;
mod console_data;
mod embeddings;
mod error;
mod logging;
mod middleware;
mod models;
mod protocol;
mod repository;
mod utils;

use crate::utils::error::sanitize_error;
use axum::{
    body::{to_bytes, Body},
    extract::{Extension, Path, State},
    http::{
        header::{HeaderName, AUTHORIZATION, CONTENT_TYPE},
        HeaderValue, Method, StatusCode,
    },
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post, put},
    Router,
};
use chrono::Utc;
use futures_util::StreamExt;
use serde_json::json;
use sha2::Digest;
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use std::net::SocketAddr;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tower_http::cors::CorsLayer;

// 账户锁定配置
const MAX_LOGIN_ATTEMPTS: i16 = 5; // 最大登录失败次数
const LOCKOUT_DURATION_SECS: i64 = 300; // 锁定 5 分钟
const MAX_DISPLAY_TEXT_LEN: usize = 50;
static SERVER_STARTED_AT: OnceLock<Instant> = OnceLock::new();

/// Current user extractor - 从 Authorization header 提取用户
#[derive(Clone, Debug)]
pub struct CurrentUser {
    pub user_id: i64,
    pub workspace_id: i64,
    pub project_id: i64,
    pub workspace_role: String,
    pub uid: String,
    pub email: String,
    pub api_key_id: Option<i64>,
    pub models_allowed: Option<serde_json::Value>,
    pub rate_limit: Option<i32>,
    pub daily_spend_limit: Option<f64>,
    pub monthly_spend_limit: Option<f64>,
    pub total_spend_limit: Option<f64>,
}

#[derive(Clone, Debug)]
struct RateLimitState {
    limit: i32,
    remaining: i32,
    reset_secs: i64,
}

#[tokio::main]
async fn main() {
    SERVER_STARTED_AT
        .set(Instant::now())
        .expect("Server start time must only be initialized once");
    dotenvy::dotenv().ok();
    logging::init_logging();

    // JWT 是控制台登录的安全根；缺失或过短时必须阻止服务启动。
    let _jwt_secret = config::jwt_secret();

    // Initialize database pool
    let database_url = config::database_url();
    let max_pool_size = std::env::var("DB_MAX_POOL_SIZE")
        .unwrap_or_else(|_| "20".to_string())
        .parse()
        .unwrap_or(20);

    let pool = PgPoolOptions::new()
        .max_connections(max_pool_size)
        .acquire_timeout(Duration::from_secs(3))
        .connect(&database_url)
        .await
        .expect("Failed to create database pool");

    tracing::info!(
        "Database pool initialized with {} max connections",
        max_pool_size
    );

    // 先建立版本化结构；兼容 DDL 只能在基础表存在后执行。
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to apply database migrations");

    // 数据库结构只由版本化迁移维护，启动时不再重复执行兼容 DDL。
    let has_byok_provider: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM providers WHERE credential_mode = 'byok')")
            .fetch_one(&pool)
            .await
            .expect("Failed to inspect BYOK provider configuration");
    if has_byok_provider {
        byok::ByokCipher::from_env()
            .expect("BYOK_MASTER_KEY_B64 is required and must decode to exactly 32 bytes");
    }

    let port = config::server_port();
    tracing::info!("Starting Overseas API Server on port {}", port);

    let cors_origins: Vec<HeaderValue> = config::cors_allowed_origins()
        .into_iter()
        .map(|origin| {
            origin
                .parse()
                .unwrap_or_else(|_| panic!("Invalid CORS origin: {}", origin))
        })
        .collect();
    let cors = CorsLayer::new()
        .allow_origin(cors_origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            AUTHORIZATION,
            CONTENT_TYPE,
            HeaderName::from_static("x-api-key"),
            HeaderName::from_static("x-nexus-workspace"),
        ]);

    // 控制台接口只接受 JWT，不允许平台 API Key 访问账户和订单。
    let console_routes = Router::new()
        // Auth
        .route("/auth/me", get(me))
        .route("/auth/logout", post(logout))
        // Workspaces, members and projects
        .route("/workspaces", get(list_workspaces).post(create_workspace))
        .route("/workspaces/:uid", put(update_workspace))
        .route(
            "/workspaces/:uid/members",
            get(list_workspace_members).post(add_workspace_member),
        )
        .route(
            "/workspaces/:uid/members/:user_uid",
            put(update_workspace_member).delete(remove_workspace_member),
        )
        .route(
            "/workspaces/:uid/projects",
            get(list_workspace_projects).post(create_workspace_project),
        )
        .route(
            "/workspaces/:uid/projects/:project_uid",
            put(update_workspace_project).delete(disable_workspace_project),
        )
        // API Keys
        .route("/keys", post(create_key_handler))
        .route("/keys", get(list_keys_handler))
        .route("/keys/:uid/disable", put(disable_key_handler))
        .route("/keys/:uid/enable", put(enable_key_handler))
        .route("/keys/:uid", put(update_key_handler))
        .route("/keys/:uid", delete(delete_key_handler))
        // BYOK provider credentials
        .route(
            "/provider-credentials",
            get(list_provider_credentials).post(create_provider_credential),
        )
        .route(
            "/provider-credentials/:uid",
            delete(delete_provider_credential),
        )
        // Orders
        .route("/orders", post(create_order))
        .route("/orders", get(list_orders))
        .route("/orders/:id", get(console_data::get_order))
        // Balance
        .route("/balance", get(get_balance))
        .route("/balance/logs", get(balance_logs))
        // Billing
        .route("/billing/usage", get(console_data::billing_usage))
        // Logs
        .route("/logs/calls", get(call_logs))
        .route("/logs/calls/:id", get(console_data::call_log_detail))
        .route("/logs/stats", get(call_stats))
        .route_layer(axum::middleware::from_fn_with_state(
            pool.clone(),
            auth_layer,
        ));

    // 模型调用只接受平台 API Key，不接受控制台 JWT。
    let gateway_routes = Router::new()
        .route("/v1/chat/completions", post(api_proxy))
        .route("/v1/chat/completions", get(api_proxy))
        .route("/v1/responses", post(responses_proxy))
        .route("/v1/embeddings", post(embeddings::proxy))
        .route(
            "/v1/images/generations",
            post(async_media::create_image_task),
        )
        .route(
            "/v1/audio/generations",
            post(async_media::create_audio_task),
        )
        .route(
            "/v1/videos/generations",
            post(async_media::create_video_task),
        )
        .route(
            "/v1/tasks/:uid",
            get(async_media::get_task).delete(async_media::cancel_task),
        )
        .route("/v1/tasks/:uid/events", get(async_media::task_events))
        .route("/v1/models", get(list_gateway_models))
        .route("/rate/limit", get(rate_limit_check))
        .route_layer(axum::middleware::from_fn_with_state(
            pool.clone(),
            api_key_layer,
        ));

    // 管理端路由只允许 ADMIN_EMAILS 配置中的登录用户访问。
    let admin_routes = Router::new()
        .route("/admin/users", get(admin_users))
        .route("/admin/users/:id", get(admin_user_detail))
        .route("/admin/users/:id/disable", put(disable_user))
        .route("/admin/users/:id/enable", put(enable_user))
        .route("/admin/orders", get(admin_orders))
        .route("/admin/orders/:id", get(admin_order_detail))
        .route("/admin/orders/:id/confirm", post(confirm_order_payment))
        .route("/admin/orders/:id/refund", post(refund_order))
        .route("/admin/models", get(admin_models))
        .route("/admin/models/:id/pricing", put(update_admin_model_pricing))
        .route("/admin/providers", get(admin_providers))
        .route("/admin/providers/:id", put(update_admin_provider))
        .route("/admin/reports/profit", get(admin_profit_report))
        .route("/admin/audit-logs", get(admin_audit_logs))
        .route("/admin/risk/users", get(admin_risk_users))
        .route_layer(axum::middleware::from_fn_with_state(
            pool.clone(),
            admin_layer,
        ));

    let app = Router::new()
        .route("/", get(index))
        .route("/health", get(health_check))
        // Auth
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        // Packages
        .route("/packages", get(list_packages))
        .route("/packages/:id", get(console_data::get_package))
        // Models
        .route("/models", get(list_models))
        // Payment Webhook (单独处理，有签名验证)
        .route("/webhooks/payment", post(payment_webhook))
        // Replicate Standard Webhooks：使用每个 BYOK 账号的签名密钥验证。
        .route(
            "/webhooks/replicate/:uid",
            post(async_media::replicate_webhook),
        )
        // Docs
        .route("/docs", get(api_docs))
        .route("/openapi.json", get(openapi_spec))
        // Metrics
        .route("/metrics", get(metrics))
        .merge(console_routes)
        .merge(gateway_routes)
        .merge(admin_routes)
        .with_state(pool)
        .layer(cors)
        .layer(axum::middleware::from_fn(normalize_api_errors));

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Server listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

/// 统一所有 JSON 错误：修正真实 HTTP 状态，并补齐可追踪字段。
async fn normalize_api_errors(
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let inspect_success_body = !req.uri().path().starts_with("/v1/");
    let response = next.run(req).await;
    if response.status().is_success() && !inspect_success_body {
        return response;
    }
    let is_json = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"));
    if !is_json {
        return response;
    }

    let original_status = response.status();
    let (mut parts, body) = response.into_parts();
    let bytes = match to_bytes(body, 16 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::error!(error = %error, "Failed to read JSON response for error normalization");
            let request_id = crate::utils::id_generator::generate_request_id();
            let mut response = (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "code": "RESPONSE_NORMALIZATION_FAILED",
                    "message": "Internal server error",
                    "request_id": request_id,
                    "retryable": false
                })),
            )
                .into_response();
            if let Ok(value) = HeaderValue::from_str(&request_id) {
                response
                    .headers_mut()
                    .insert(HeaderName::from_static("x-nexus-request-id"), value);
            }
            return response;
        }
    };
    let mut value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return Response::from_parts(parts, Body::from(bytes)),
    };
    let embedded_status = value
        .get("code")
        .and_then(serde_json::Value::as_u64)
        .filter(|code| (400..=599).contains(code))
        .and_then(|code| StatusCode::from_u16(code as u16).ok());
    let status = if original_status.is_client_error() || original_status.is_server_error() {
        original_status
    } else if let Some(status) = embedded_status {
        status
    } else {
        return Response::from_parts(parts, Body::from(bytes));
    };

    let request_id = parts
        .headers
        .get("x-nexus-request-id")
        .and_then(|header| header.to_str().ok())
        .map(str::to_string)
        .unwrap_or_else(crate::utils::id_generator::generate_request_id);
    let nested_error = value.get("error").and_then(serde_json::Value::as_object);
    let message = value
        .get("message")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            nested_error
                .and_then(|error| error.get("message"))
                .and_then(serde_json::Value::as_str)
        })
        .unwrap_or("Request failed")
        .to_string();
    let code = value
        .get("code")
        .cloned()
        .or_else(|| nested_error.and_then(|error| error.get("code")).cloned());
    let retryable = value
        .get("retryable")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(matches!(
            status,
            StatusCode::REQUEST_TIMEOUT
                | StatusCode::TOO_MANY_REQUESTS
                | StatusCode::BAD_GATEWAY
                | StatusCode::SERVICE_UNAVAILABLE
                | StatusCode::GATEWAY_TIMEOUT
        ));
    if let Some(object) = value.as_object_mut() {
        if let Some(code) = code {
            object.insert("code".to_string(), code);
        } else {
            object.insert(
                "code".to_string(),
                serde_json::Value::String(status.as_u16().to_string()),
            );
        }
        object.insert("message".to_string(), serde_json::Value::String(message));
        object.insert(
            "request_id".to_string(),
            serde_json::Value::String(request_id.clone()),
        );
        object.insert("retryable".to_string(), serde_json::Value::Bool(retryable));
    }
    parts.status = status;
    parts.headers.remove(axum::http::header::CONTENT_LENGTH);
    if let Ok(header) = HeaderValue::from_str(&request_id) {
        parts
            .headers
            .insert(HeaderName::from_static("x-nexus-request-id"), header);
    }
    let body = match serde_json::to_vec(&value) {
        Ok(body) => body,
        Err(error) => {
            tracing::error!(error = %error, "Failed to serialize normalized JSON error");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "code": "RESPONSE_NORMALIZATION_FAILED",
                    "message": "Internal server error",
                    "request_id": request_id,
                    "retryable": false
                })),
            )
                .into_response();
        }
    };
    Response::from_parts(parts, Body::from(body))
}

/// Auth extractor - 从请求中提取当前用户（通过 JWT）
#[allow(dead_code)]
pub async fn require_auth(
    Extension(pool): Extension<PgPool>,
    req: axum::extract::Request,
) -> Result<CurrentUser, Json<serde_json::Value>> {
    use axum::http::header;

    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(auth) if auth.starts_with("Bearer ") => &auth[7..],
        _ => {
            return Err(Json(json!({
                "code": 401,
                "message": "Missing or invalid Authorization header"
            })));
        }
    };

    // 验证 token
    let claims = match crate::utils::jwt::verify_token(token) {
        Ok(c) => c,
        Err(_) => {
            return Err(Json(json!({
                "code": 401,
                "message": "Invalid or expired token"
            })));
        }
    };

    let token_is_current = match sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(
            SELECT 1 FROM overseas_users
            WHERE id = $1 AND status = 0 AND token_version = $2
        )",
    )
    .bind(claims.sub)
    .bind(claims.token_version)
    .fetch_one(&pool)
    .await
    {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(error = %error, "Failed to verify login token state");
            return Err(Json(json!({
                "code": 500,
                "message": "Internal server error"
            })));
        }
    };
    if !token_is_current {
        return Err(Json(
            json!({"code": 401, "message": "Invalidated login token"}),
        ));
    }

    Ok(CurrentUser {
        user_id: claims.sub,
        workspace_id: 0,
        project_id: 0,
        workspace_role: "member".to_string(),
        uid: claims.uid,
        email: claims.email,
        api_key_id: None,
        models_allowed: None,
        rate_limit: None,
        daily_spend_limit: None,
        monthly_spend_limit: None,
        total_spend_limit: None,
    })
}

/// 控制台认证中间件：只接受有效 JWT，并确认账号当前仍启用。
async fn auth_layer(
    State(pool): State<PgPool>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    use axum::http::header;

    let requested_workspace = req
        .headers()
        .get("x-nexus-workspace")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let claims = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "))
        .and_then(|token| crate::utils::jwt::verify_token(token).ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"code": 401, "message": "Valid login token required"})),
            )
        })?;

    let account = sqlx::query(
        "SELECT w.id, p.id, wm.role
         FROM overseas_users u
         JOIN workspaces w ON w.status = 0
         JOIN workspace_members wm ON wm.workspace_id = w.id AND wm.user_id = u.id AND wm.status = 0
         JOIN projects p ON p.workspace_id = w.id AND p.name = 'Default' AND p.status = 0
         WHERE u.id = $1 AND u.status = 0 AND u.token_version = $3
           AND w.uid = COALESCE($2, 'ws_' || u.uid)",
    )
    .bind(claims.sub)
    .bind(requested_workspace.as_deref())
    .bind(claims.token_version)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to verify console account");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"code": 500, "message": "Internal server error"})),
        )
    })?;
    let Some(account) = account else {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"code": 401, "message": "Account is disabled or unavailable"})),
        ));
    };

    let user = CurrentUser {
        user_id: claims.sub,
        workspace_id: account.get(0),
        project_id: account.get(1),
        workspace_role: account.get(2),
        uid: claims.uid,
        email: claims.email,
        api_key_id: None,
        models_allowed: None,
        rate_limit: None,
        daily_spend_limit: None,
        monthly_spend_limit: None,
        total_spend_limit: None,
    };
    let mut req = req;
    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}

/// 模型调用认证中间件：只接受有效且未过期的平台 API Key。
async fn api_key_layer(
    State(pool): State<PgPool>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    let peer_ip = req
        .extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|connect| connect.0.ip())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code": 500, "message": "Client address unavailable"})),
            )
        })?;
    let client_ip = if config::trust_proxy_headers() {
        let value = req
            .headers()
            .get("x-real-ip")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<std::net::IpAddr>().ok())
            .ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"code": 400, "message": "Trusted proxy client address is missing or invalid"})),
                )
            })?;
        value.to_string()
    } else {
        peer_ip.to_string()
    };
    let api_key = req
        .headers()
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"code": 401, "message": "Valid API key required"})),
            )
        })?;
    let mut hasher = sha2::Sha256::default();
    hasher.update(api_key.as_bytes());
    let key_hash = hex::encode(hasher.finalize());

    let row = sqlx::query(
        "SELECT ak.id, ak.user_id, ak.models_allowed, ak.rate_limit,
                CAST(ak.daily_spend_limit AS VARCHAR), u.uid, u.email,
                ak.workspace_id, ak.project_id,
                CAST(ak.monthly_spend_limit AS VARCHAR), CAST(ak.total_spend_limit AS VARCHAR),
                wm.role
         FROM api_keys ak
         JOIN overseas_users u ON u.id = ak.user_id
         JOIN workspaces w ON w.id = ak.workspace_id AND w.status = 0
         JOIN projects p ON p.id = ak.project_id AND p.workspace_id = ak.workspace_id AND p.status = 0
         JOIN workspace_members wm ON wm.workspace_id = ak.workspace_id AND wm.user_id = ak.user_id AND wm.status = 0
         WHERE ak.key_hash = $1 AND ak.status = 0 AND u.status = 0
           AND (ak.expires_at IS NULL OR ak.expires_at > NOW())
           AND (CARDINALITY(ak.ip_allowlist) = 0 OR $2::INET <<= ANY(ak.ip_allowlist))",
    )
    .bind(&key_hash)
    .bind(&client_ip)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to verify API key");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"code": 500, "message": "Internal server error"})),
        )
    })?
    .ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(json!({"code": 401, "message": "Invalid or expired API key"})),
        )
    })?;

    let api_key_id: i64 = row.get(0);
    let user = CurrentUser {
        user_id: row.get(1),
        workspace_id: row.get(7),
        project_id: row.get(8),
        workspace_role: row.get(11),
        models_allowed: row.get(2),
        rate_limit: Some(row.get(3)),
        daily_spend_limit: row
            .get::<Option<String>, _>(4)
            .and_then(|value| value.parse::<f64>().ok()),
        uid: row.get(5),
        email: row.get(6),
        api_key_id: Some(api_key_id),
        monthly_spend_limit: row
            .get::<Option<String>, _>(9)
            .and_then(|value| value.parse::<f64>().ok()),
        total_spend_limit: row
            .get::<Option<String>, _>(10)
            .and_then(|value| value.parse::<f64>().ok()),
    };
    sqlx::query("UPDATE api_keys SET last_used_at = NOW(), updated_at = NOW() WHERE id = $1")
        .bind(api_key_id)
        .execute(&pool)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to update API key usage time");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code": 500, "message": "Internal server error"})),
            )
        })?;
    let mut req = req;
    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}

async fn admin_layer(
    State(pool): State<PgPool>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    use axum::http::header;

    let requested_workspace = req
        .headers()
        .get("x-nexus-workspace")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let token = match auth_header {
        Some(auth) if auth.starts_with("Bearer ") => &auth[7..],
        _ => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"code": 401, "message": "Admin login required"})),
            ));
        }
    };

    let claims = match crate::utils::jwt::verify_token(token) {
        Ok(claims) => claims,
        Err(_) => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({"code": 401, "message": "Invalid or expired admin token"})),
            ));
        }
    };

    let admin_emails = config::admin_emails();
    if admin_emails.is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"code": 403, "message": "Admin access is not configured"})),
        ));
    }

    let email = claims.email.to_lowercase();
    if !admin_emails.iter().any(|admin_email| admin_email == &email) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"code": 403, "message": "Admin permission required"})),
        ));
    }

    let account = sqlx::query(
        "SELECT w.id, p.id, wm.role
         FROM overseas_users u
         JOIN workspaces w ON w.status = 0
         JOIN workspace_members wm ON wm.workspace_id = w.id AND wm.user_id = u.id AND wm.status = 0
         JOIN projects p ON p.workspace_id = w.id AND p.name = 'Default' AND p.status = 0
         WHERE u.id = $1 AND u.status = 0 AND u.token_version = $3
           AND w.uid = COALESCE($2, 'ws_' || u.uid)",
    )
    .bind(claims.sub)
    .bind(requested_workspace.as_deref())
    .bind(claims.token_version)
    .fetch_optional(&pool)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to verify admin account");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"code": 500, "message": "Internal server error"})),
        )
    })?;
    let Some(account) = account else {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"code": 401, "message": "Admin account is disabled"})),
        ));
    };

    let user = CurrentUser {
        user_id: claims.sub,
        workspace_id: account.get(0),
        project_id: account.get(1),
        workspace_role: account.get(2),
        uid: claims.uid,
        email: claims.email,
        api_key_id: None,
        models_allowed: None,
        rate_limit: None,
        daily_spend_limit: None,
        monthly_spend_limit: None,
        total_spend_limit: None,
    };
    let mut req = req;
    req.extensions_mut().insert(user);

    Ok(next.run(req).await)
}

async fn index() -> &'static str {
    "Overseas API Server"
}

async fn health_check() -> &'static str {
    "OK"
}

// Auth handlers
// 使用 bcrypt 验证密码
async fn login(
    State(pool): State<PgPool>,
    Json(input): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    use sqlx::Row;

    let email = match input.get("email").and_then(|v| v.as_str()) {
        Some(e) if !e.is_empty() => e,
        _ => return Json(json!({"code": 400, "message": "Email is required"})),
    };

    let password = match input.get("password").and_then(|v| v.as_str()) {
        Some(p) if !p.is_empty() => p,
        _ => return Json(json!({"code": 400, "message": "Password is required"})),
    };

    // 查询用户
    let user_row = match sqlx::query(
        "SELECT id, uid, email, nickname, password_hash, status, failed_login_attempts, locked_until, token_version
         FROM overseas_users WHERE email = $1"
    )
    .bind(email)
    .fetch_optional(&pool)
    .await {
        Ok(Some(row)) => row,
        Ok(None) => return Json(json!({"code": 401, "message": "Invalid email or password"})),
        Err(e) => return Json(json!({"code": 500, "message": sanitize_error(e)})),
    };

    let user_id: i64 = user_row.get(0);
    let uid: String = user_row.get(1);
    let nickname: String = user_row.get(3);
    let stored_hash: String = user_row.get(4);
    let status: i16 = user_row.get(5);
    let failed_attempts: Option<i16> = user_row.get(6);
    let locked_until: Option<chrono::DateTime<Utc>> = user_row.get(7);
    let token_version: i64 = user_row.get(8);

    let failed_count = failed_attempts.unwrap_or(0);

    // 检查账户状态
    if status == 1 {
        return Json(json!({"code": 403, "message": "Account is disabled"}));
    }

    // 检查是否被锁定
    if let Some(lock_time) = locked_until {
        if lock_time > chrono::Utc::now() {
            let remaining = (lock_time - chrono::Utc::now()).num_seconds();
            return Json(json!({
                "code": 423,
                "message": format!("Account is locked. Try again in {} seconds", remaining)
            }));
        } else {
            // 锁定到期，解锁账户
            sqlx::query(
                "UPDATE overseas_users SET locked_until = NULL, failed_login_attempts = 0 WHERE id = $1"
            )
            .bind(user_id)
            .execute(&pool)
            .await
            .ok();
        }
    }

    // 验证密码
    let password_valid = match bcrypt::verify(password, &stored_hash) {
        Ok(valid) => valid,
        Err(_) => false,
    };

    if !password_valid {
        // 增加失败计数
        let new_attempts = failed_count + 1;
        if new_attempts >= MAX_LOGIN_ATTEMPTS {
            // 锁定账户
            let lock_until = chrono::Utc::now() + chrono::Duration::seconds(LOCKOUT_DURATION_SECS);
            sqlx::query(
                "UPDATE overseas_users SET failed_login_attempts = $1, locked_until = $2 WHERE id = $3"
            )
            .bind(new_attempts)
            .bind(lock_until)
            .bind(user_id)
            .execute(&pool)
            .await
            .ok();

            return Json(json!({
                "code": 423,
                "message": "Too many failed attempts. Account locked for 5 minutes"
            }));
        } else {
            sqlx::query("UPDATE overseas_users SET failed_login_attempts = $1 WHERE id = $2")
                .bind(new_attempts)
                .bind(user_id)
                .execute(&pool)
                .await
                .ok();
        }

        return Json(json!({
            "code": 401,
            "message": "Invalid email or password",
            "attempts_remaining": MAX_LOGIN_ATTEMPTS - new_attempts
        }));
    }

    // 登录成功，重置失败计数
    sqlx::query(
        "UPDATE overseas_users SET failed_login_attempts = 0, locked_until = NULL, last_login_at = NOW() WHERE id = $1"
    )
    .bind(user_id)
    .execute(&pool)
    .await
    .ok();

    // 生成 JWT token
    let claims = crate::utils::jwt::Claims::new(user_id, &uid, email, token_version);
    let token = match crate::utils::jwt::generate_token(&claims) {
        Ok(t) => t,
        Err(_) => return Json(json!({"code": 500, "message": "Failed to generate token"})),
    };

    Json(json!({
        "code": 0,
        "message": "Login successful",
        "data": {
            "uid": uid,
            "email": email,
            "nickname": nickname,
            "token": token
        }
    }))
}
async fn logout(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Json<serde_json::Value> {
    match sqlx::query(
        "UPDATE overseas_users
         SET token_version = token_version + 1, updated_at = NOW()
         WHERE id = $1 AND status = 0",
    )
    .bind(user.user_id)
    .execute(&pool)
    .await
    {
        Ok(result) if result.rows_affected() == 1 => {
            Json(json!({"code": 0, "message": "Logged out"}))
        }
        Ok(_) => Json(json!({"code": 401, "message": "Account is unavailable"})),
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

// 注册 - 简化版，需要补充邮箱验证逻辑
async fn register(
    State(pool): State<PgPool>,
    Json(input): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    use sqlx::Row;

    let email = match input.get("email").and_then(|v| v.as_str()) {
        Some(e) if !e.is_empty() => e,
        _ => return Json(json!({"code": 400, "message": "Email is required"})),
    };

    let password = match input.get("password").and_then(|v| v.as_str()) {
        Some(p) if p.len() >= 6 => p,
        _ => {
            return Json(json!({"code": 400, "message": "Password must be at least 6 characters"}))
        }
    };

    let nickname_raw = input
        .get("nickname")
        .and_then(|v| v.as_str())
        .unwrap_or("User");
    let nickname = match clean_display_text(nickname_raw, "Nickname") {
        Ok(value) => value,
        Err(message) => return Json(json!({"code": 400, "message": message})),
    };

    // 检查邮箱是否已存在
    let existing = sqlx::query("SELECT id FROM overseas_users WHERE email = $1")
        .bind(email)
        .fetch_optional(&pool)
        .await;

    if let Ok(Some(_)) = existing {
        return Json(json!({"code": 409, "message": "Email already registered"}));
    }

    // 生成 UID
    let uid = crate::utils::id_generator::generate_uid();
    let password_hash = match bcrypt::hash(password, 10) {
        Ok(h) => h,
        Err(_e) => return Json(json!({"code": 500, "message": "Failed to process password"})),
    };

    // 创建用户
    let result = sqlx::query(
        "INSERT INTO overseas_users (uid, email, nickname, password_hash)
         VALUES ($1, $2, $3, $4)
         RETURNING id, token_version",
    )
    .bind(&uid)
    .bind(email)
    .bind(&nickname)
    .bind(&password_hash)
    .fetch_one(&pool)
    .await;

    match result {
        Ok(row) => {
            let user_id: i64 = row.get(0);
            let token_version: i64 = row.get(1);
            let claims = crate::utils::jwt::Claims::new(user_id, &uid, email, token_version);
            let token = match crate::utils::jwt::generate_token(&claims) {
                Ok(t) => t,
                Err(_) => return Json(json!({"code": 500, "message": "Failed to generate token"})),
            };

            Json(json!({
                "code": 0,
                "message": "Registration successful",
                "data": {
                    "uid": uid,
                    "email": email,
                    "nickname": nickname,
                    "token": token
                }
            }))
        }
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}
async fn me(Extension(user): Extension<CurrentUser>) -> Json<serde_json::Value> {
    let email = user.email.to_lowercase();
    let is_admin = config::admin_emails()
        .iter()
        .any(|admin_email| admin_email == &email);

    Json(json!({
        "code": 0,
        "data": {
            "uid": user.uid,
            "email": user.email,
            "is_admin": is_admin,
            "workspace_id": user.workspace_id,
            "project_id": user.project_id,
            "workspace_role": user.workspace_role
        }
    }))
}

fn workspace_admin_required(user: &CurrentUser) -> Result<(), Json<serde_json::Value>> {
    if user.workspace_role == "admin" {
        Ok(())
    } else {
        Err(Json(json!({
            "code": 403,
            "message": "Workspace administrator permission required"
        })))
    }
}

async fn selected_workspace_required(
    pool: &PgPool,
    user: &CurrentUser,
    workspace_uid: &str,
) -> Result<(), Json<serde_json::Value>> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(
            SELECT 1 FROM workspaces
            WHERE id = $1 AND uid = $2 AND status = 0
        )",
    )
    .bind(user.workspace_id)
    .bind(workspace_uid)
    .fetch_one(pool)
    .await
    .map_err(|e| Json(json!({"code": 500, "message": sanitize_error(e)})))?;
    if exists {
        Ok(())
    } else {
        Err(Json(json!({"code": 404, "message": "Workspace not found"})))
    }
}

fn requested_member_role(input: &serde_json::Value) -> Result<&str, Json<serde_json::Value>> {
    match input.get("role").and_then(|value| value.as_str()) {
        Some("admin") => Ok("admin"),
        Some("member") => Ok("member"),
        _ => Err(Json(json!({
            "code": 400,
            "message": "Role must be admin or member"
        }))),
    }
}

async fn list_workspaces(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Json<serde_json::Value> {
    let rows = match sqlx::query(
        "SELECT w.uid, w.name, wm.role, w.owner_user_id = $1 AS is_owner,
                (SELECT COUNT(*) FROM workspace_members members
                 WHERE members.workspace_id = w.id AND members.status = 0) AS member_count,
                (SELECT COUNT(*) FROM projects p
                 WHERE p.workspace_id = w.id AND p.status = 0) AS project_count,
                w.created_at, w.id = $2 AS selected
         FROM workspace_members wm
         JOIN workspaces w ON w.id = wm.workspace_id AND w.status = 0
         WHERE wm.user_id = $1 AND wm.status = 0
         ORDER BY selected DESC, w.created_at ASC",
    )
    .bind(user.user_id)
    .bind(user.workspace_id)
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => return Json(json!({"code": 500, "message": sanitize_error(e)})),
    };

    let items: Vec<_> = rows
        .iter()
        .map(|row| {
            json!({
                "uid": row.get::<String, _>(0),
                "name": row.get::<String, _>(1),
                "role": row.get::<String, _>(2),
                "is_owner": row.get::<bool, _>(3),
                "member_count": row.get::<i64, _>(4),
                "project_count": row.get::<i64, _>(5),
                "created_at": row.get::<chrono::NaiveDateTime, _>(6),
                "selected": row.get::<bool, _>(7),
            })
        })
        .collect();

    Json(json!({"code": 0, "data": {"items": items}}))
}

async fn create_workspace(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
    Json(input): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let name = match input
        .get("name")
        .and_then(|value| value.as_str())
        .map(|value| clean_display_text(value, "Workspace name"))
    {
        Some(Ok(name)) => name,
        Some(Err(message)) => return Json(json!({"code": 400, "message": message})),
        None => return Json(json!({"code": 400, "message": "Workspace name is required"})),
    };
    let workspace_uid = crate::utils::id_generator::generate_workspace_id();
    let project_uid = crate::utils::id_generator::generate_project_id();
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(e) => return Json(json!({"code": 500, "message": sanitize_error(e)})),
    };

    let workspace_id: i64 = match sqlx::query_scalar(
        "INSERT INTO workspaces (uid, name, owner_user_id, status)
         VALUES ($1, $2, $3, 0)
         RETURNING id",
    )
    .bind(&workspace_uid)
    .bind(&name)
    .bind(user.user_id)
    .fetch_one(&mut *tx)
    .await
    {
        Ok(id) => id,
        Err(e) => return Json(json!({"code": 500, "message": sanitize_error(e)})),
    };

    if let Err(e) = sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role, status)
         VALUES ($1, $2, 'admin', 0)",
    )
    .bind(workspace_id)
    .bind(user.user_id)
    .execute(&mut *tx)
    .await
    {
        return Json(json!({"code": 500, "message": sanitize_error(e)}));
    }
    if let Err(e) = sqlx::query(
        "INSERT INTO projects (uid, workspace_id, name, status)
         VALUES ($1, $2, 'Default', 0)",
    )
    .bind(&project_uid)
    .bind(workspace_id)
    .execute(&mut *tx)
    .await
    {
        return Json(json!({"code": 500, "message": sanitize_error(e)}));
    }
    if let Err(e) = sqlx::query(
        "INSERT INTO balances
         (user_id, workspace_id, balance, frozen_balance, total_recharged, total_consumed)
         VALUES ($1, $2, 0, 0, 0, 0)",
    )
    .bind(user.user_id)
    .bind(workspace_id)
    .execute(&mut *tx)
    .await
    {
        return Json(json!({"code": 500, "message": sanitize_error(e)}));
    }
    if let Err(e) = tx.commit().await {
        return Json(json!({"code": 500, "message": sanitize_error(e)}));
    }

    Json(json!({
        "code": 0,
        "message": "Workspace created",
        "data": {"uid": workspace_uid, "name": name, "role": "admin"}
    }))
}

async fn update_workspace(
    Path(workspace_uid): Path<String>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
    Json(input): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    if let Err(response) = workspace_admin_required(&user) {
        return response;
    }
    if let Err(response) = selected_workspace_required(&pool, &user, &workspace_uid).await {
        return response;
    }
    let name = match input
        .get("name")
        .and_then(|value| value.as_str())
        .map(|value| clean_display_text(value, "Workspace name"))
    {
        Some(Ok(name)) => name,
        Some(Err(message)) => return Json(json!({"code": 400, "message": message})),
        None => return Json(json!({"code": 400, "message": "Workspace name is required"})),
    };
    match sqlx::query(
        "UPDATE workspaces SET name = $1, updated_at = NOW()
         WHERE id = $2 AND uid = $3 AND status = 0",
    )
    .bind(&name)
    .bind(user.workspace_id)
    .bind(&workspace_uid)
    .execute(&pool)
    .await
    {
        Ok(result) if result.rows_affected() == 1 => {
            Json(json!({"code": 0, "message": "Workspace updated", "data": {"name": name}}))
        }
        Ok(_) => Json(json!({"code": 404, "message": "Workspace not found"})),
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn list_workspace_members(
    Path(workspace_uid): Path<String>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Json<serde_json::Value> {
    if let Err(response) = selected_workspace_required(&pool, &user, &workspace_uid).await {
        return response;
    }
    let rows = match sqlx::query(
        "SELECT u.uid, u.email, u.nickname, wm.role,
                w.owner_user_id = u.id AS is_owner, wm.created_at
         FROM workspace_members wm
         JOIN overseas_users u ON u.id = wm.user_id AND u.status = 0
         JOIN workspaces w ON w.id = wm.workspace_id
         WHERE wm.workspace_id = $1 AND wm.status = 0
         ORDER BY is_owner DESC, wm.created_at ASC",
    )
    .bind(user.workspace_id)
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => return Json(json!({"code": 500, "message": sanitize_error(e)})),
    };
    let items: Vec<_> = rows
        .iter()
        .map(|row| {
            json!({
                "uid": row.get::<String, _>(0),
                "email": row.get::<String, _>(1),
                "nickname": row.get::<String, _>(2),
                "role": row.get::<String, _>(3),
                "is_owner": row.get::<bool, _>(4),
                "created_at": row.get::<chrono::NaiveDateTime, _>(5),
            })
        })
        .collect();
    Json(json!({"code": 0, "data": {"items": items}}))
}

async fn add_workspace_member(
    Path(workspace_uid): Path<String>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
    Json(input): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    if let Err(response) = workspace_admin_required(&user) {
        return response;
    }
    if let Err(response) = selected_workspace_required(&pool, &user, &workspace_uid).await {
        return response;
    }
    let email = match input
        .get("email")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(email) => email.to_lowercase(),
        None => return Json(json!({"code": 400, "message": "Member email is required"})),
    };
    let role = match requested_member_role(&input) {
        Ok(role) => role,
        Err(response) => return response,
    };
    let member = match sqlx::query(
        "SELECT id, uid, email, nickname FROM overseas_users
         WHERE LOWER(email) = $1 AND status = 0",
    )
    .bind(&email)
    .fetch_optional(&pool)
    .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return Json(json!({"code": 404, "message": "Registered user not found"})),
        Err(e) => return Json(json!({"code": 500, "message": sanitize_error(e)})),
    };
    let member_id: i64 = member.get(0);
    let owner_id: i64 = match sqlx::query_scalar(
        "SELECT owner_user_id FROM workspaces WHERE id = $1 AND uid = $2 AND status = 0",
    )
    .bind(user.workspace_id)
    .bind(&workspace_uid)
    .fetch_one(&pool)
    .await
    {
        Ok(id) => id,
        Err(e) => return Json(json!({"code": 500, "message": sanitize_error(e)})),
    };
    if member_id == owner_id && role != "admin" {
        return Json(
            json!({"code": 409, "message": "Workspace owner must remain an administrator"}),
        );
    }
    if let Err(e) = sqlx::query(
        "INSERT INTO workspace_members (workspace_id, user_id, role, status)
         VALUES ($1, $2, $3, 0)
         ON CONFLICT (workspace_id, user_id)
         DO UPDATE SET role = EXCLUDED.role, status = 0, updated_at = NOW()",
    )
    .bind(user.workspace_id)
    .bind(member_id)
    .bind(role)
    .execute(&pool)
    .await
    {
        return Json(json!({"code": 500, "message": sanitize_error(e)}));
    }
    Json(json!({
        "code": 0,
        "message": "Workspace member saved",
        "data": {
            "uid": member.get::<String, _>(1),
            "email": member.get::<String, _>(2),
            "nickname": member.get::<String, _>(3),
            "role": role
        }
    }))
}

async fn update_workspace_member(
    Path((workspace_uid, member_uid)): Path<(String, String)>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
    Json(input): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    if let Err(response) = workspace_admin_required(&user) {
        return response;
    }
    if let Err(response) = selected_workspace_required(&pool, &user, &workspace_uid).await {
        return response;
    }
    let role = match requested_member_role(&input) {
        Ok(role) => role,
        Err(response) => return response,
    };
    let result = sqlx::query(
        "UPDATE workspace_members wm
         SET role = $1, updated_at = NOW()
         FROM overseas_users u, workspaces w
         WHERE wm.workspace_id = $2 AND wm.user_id = u.id
           AND w.id = wm.workspace_id AND w.uid = $3
           AND u.uid = $4 AND wm.status = 0
           AND w.owner_user_id <> u.id",
    )
    .bind(role)
    .bind(user.workspace_id)
    .bind(&workspace_uid)
    .bind(&member_uid)
    .execute(&pool)
    .await;
    match result {
        Ok(result) if result.rows_affected() == 1 => {
            Json(json!({"code": 0, "message": "Member role updated"}))
        }
        Ok(_) => Json(
            json!({"code": 409, "message": "Member not found or owner role cannot be changed"}),
        ),
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn remove_workspace_member(
    Path((workspace_uid, member_uid)): Path<(String, String)>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Json<serde_json::Value> {
    if let Err(response) = workspace_admin_required(&user) {
        return response;
    }
    if member_uid == user.uid {
        return Json(json!({"code": 409, "message": "You cannot remove your current account"}));
    }
    if let Err(response) = selected_workspace_required(&pool, &user, &workspace_uid).await {
        return response;
    }
    let result = sqlx::query(
        "UPDATE workspace_members wm
         SET status = 1, updated_at = NOW()
         FROM overseas_users u, workspaces w
         WHERE wm.workspace_id = $1 AND wm.user_id = u.id
           AND w.id = wm.workspace_id AND w.uid = $2
           AND u.uid = $3 AND wm.status = 0
           AND w.owner_user_id <> u.id",
    )
    .bind(user.workspace_id)
    .bind(&workspace_uid)
    .bind(&member_uid)
    .execute(&pool)
    .await;
    match result {
        Ok(result) if result.rows_affected() == 1 => {
            Json(json!({"code": 0, "message": "Member removed"}))
        }
        Ok(_) => Json(
            json!({"code": 409, "message": "Member not found or workspace owner cannot be removed"}),
        ),
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn list_workspace_projects(
    Path(workspace_uid): Path<String>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Json<serde_json::Value> {
    if let Err(response) = selected_workspace_required(&pool, &user, &workspace_uid).await {
        return response;
    }
    let rows = match sqlx::query(
        "SELECT uid, name, created_at FROM projects
         WHERE workspace_id = $1 AND status = 0
         ORDER BY CASE WHEN name = 'Default' THEN 0 ELSE 1 END, created_at ASC",
    )
    .bind(user.workspace_id)
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => rows,
        Err(e) => return Json(json!({"code": 500, "message": sanitize_error(e)})),
    };
    let items: Vec<_> = rows
        .iter()
        .map(|row| {
            json!({
                "uid": row.get::<String, _>(0),
                "name": row.get::<String, _>(1),
                "is_default": row.get::<String, _>(1) == "Default",
                "created_at": row.get::<chrono::NaiveDateTime, _>(2),
            })
        })
        .collect();
    Json(json!({"code": 0, "data": {"items": items}}))
}

async fn create_workspace_project(
    Path(workspace_uid): Path<String>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
    Json(input): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    if let Err(response) = workspace_admin_required(&user) {
        return response;
    }
    if let Err(response) = selected_workspace_required(&pool, &user, &workspace_uid).await {
        return response;
    }
    let name = match input
        .get("name")
        .and_then(|value| value.as_str())
        .map(|value| clean_display_text(value, "Project name"))
    {
        Some(Ok(name)) if name != "Default" => name,
        Some(Ok(_)) => {
            return Json(json!({"code": 409, "message": "Default project already exists"}))
        }
        Some(Err(message)) => return Json(json!({"code": 400, "message": message})),
        None => return Json(json!({"code": 400, "message": "Project name is required"})),
    };
    let exists = match sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(
            SELECT 1 FROM projects
            WHERE workspace_id = $1 AND name = $2 AND status = 0
        )",
    )
    .bind(user.workspace_id)
    .bind(&name)
    .fetch_one(&pool)
    .await
    {
        Ok(exists) => exists,
        Err(error) => return Json(json!({"code": 500, "message": sanitize_error(error)})),
    };
    if exists {
        return Json(json!({"code": 409, "message": "Project name already exists"}));
    }
    let project_uid = crate::utils::id_generator::generate_project_id();
    match sqlx::query(
        "INSERT INTO projects (uid, workspace_id, name, status)
         VALUES ($1, $2, $3, 0)",
    )
    .bind(&project_uid)
    .bind(user.workspace_id)
    .bind(&name)
    .execute(&pool)
    .await
    {
        Ok(_) => Json(json!({
            "code": 0,
            "message": "Project created",
            "data": {"uid": project_uid, "name": name}
        })),
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn update_workspace_project(
    Path((workspace_uid, project_uid)): Path<(String, String)>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
    Json(input): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    if let Err(response) = workspace_admin_required(&user) {
        return response;
    }
    if let Err(response) = selected_workspace_required(&pool, &user, &workspace_uid).await {
        return response;
    }
    let name = match input
        .get("name")
        .and_then(|value| value.as_str())
        .map(|value| clean_display_text(value, "Project name"))
    {
        Some(Ok(name)) if name != "Default" => name,
        Some(Ok(_)) => {
            return Json(json!({"code": 409, "message": "Default is a reserved project name"}))
        }
        Some(Err(message)) => return Json(json!({"code": 400, "message": message})),
        None => return Json(json!({"code": 400, "message": "Project name is required"})),
    };
    let duplicate = match sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(
            SELECT 1 FROM projects
            WHERE workspace_id = $1 AND name = $2 AND uid <> $3 AND status = 0
        )",
    )
    .bind(user.workspace_id)
    .bind(&name)
    .bind(&project_uid)
    .fetch_one(&pool)
    .await
    {
        Ok(duplicate) => duplicate,
        Err(error) => return Json(json!({"code": 500, "message": sanitize_error(error)})),
    };
    if duplicate {
        return Json(json!({"code": 409, "message": "Project name already exists"}));
    }
    match sqlx::query(
        "UPDATE projects SET name = $1, updated_at = NOW()
         WHERE workspace_id = $2 AND uid = $3 AND status = 0 AND name <> 'Default'",
    )
    .bind(&name)
    .bind(user.workspace_id)
    .bind(&project_uid)
    .execute(&pool)
    .await
    {
        Ok(result) if result.rows_affected() == 1 => {
            Json(json!({"code": 0, "message": "Project updated", "data": {"name": name}}))
        }
        Ok(_) => Json(
            json!({"code": 409, "message": "Project not found or Default project cannot be renamed"}),
        ),
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn disable_workspace_project(
    Path((workspace_uid, project_uid)): Path<(String, String)>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Json<serde_json::Value> {
    if let Err(response) = workspace_admin_required(&user) {
        return response;
    }
    if let Err(response) = selected_workspace_required(&pool, &user, &workspace_uid).await {
        return response;
    }
    let active_key_count: i64 = match sqlx::query_scalar(
        "SELECT COUNT(*) FROM api_keys k
         JOIN projects p ON p.id = k.project_id
         WHERE p.workspace_id = $1 AND p.uid = $2 AND k.status IN (0, 2)",
    )
    .bind(user.workspace_id)
    .bind(&project_uid)
    .fetch_one(&pool)
    .await
    {
        Ok(count) => count,
        Err(e) => return Json(json!({"code": 500, "message": sanitize_error(e)})),
    };
    if active_key_count > 0 {
        return Json(json!({"code": 409, "message": "Disable or move project API keys first"}));
    }
    match sqlx::query(
        "UPDATE projects SET status = 1, updated_at = NOW()
         WHERE workspace_id = $1 AND uid = $2 AND status = 0 AND name <> 'Default'",
    )
    .bind(user.workspace_id)
    .bind(&project_uid)
    .execute(&pool)
    .await
    {
        Ok(result) if result.rows_affected() == 1 => {
            Json(json!({"code": 0, "message": "Project disabled"}))
        }
        Ok(_) => Json(
            json!({"code": 409, "message": "Project not found or Default project cannot be disabled"}),
        ),
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

// API Keys - 使用 handlers 模块的实现
async fn create_key_handler(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
    Json(input): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let user_id = user.user_id;
    let repo = crate::repository::api_key_repo::ApiKeyRepository::new(&pool);

    let name_raw = input
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("default");
    let name = match clean_display_text(name_raw, "Key name") {
        Ok(value) => value,
        Err(message) => return Json(json!({"code": 400, "message": message})),
    };
    let rate_limit = input
        .get("rate_limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(500) as i32;
    if !(1..=10_000).contains(&rate_limit) {
        return Json(json!({"code": 400, "message": "Rate limit must be between 1 and 10000"}));
    }
    let models = input
        .get("models")
        .or_else(|| input.get("models_allowed"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        });
    let daily_spend_limit = input
        .get("daily_spend_limit")
        .and_then(|v| v.as_f64())
        .filter(|value| *value > 0.0);
    let monthly_spend_limit = input
        .get("monthly_spend_limit")
        .and_then(|v| v.as_f64())
        .filter(|value| *value > 0.0);
    let total_spend_limit = input
        .get("total_spend_limit")
        .and_then(|v| v.as_f64())
        .filter(|value| *value > 0.0);
    let expires_at = match input.get("expires_at").and_then(|value| value.as_str()) {
        Some(value) if !value.trim().is_empty() => {
            let parsed = match chrono::DateTime::parse_from_rfc3339(value.trim()) {
                Ok(value) => value.with_timezone(&Utc).naive_utc(),
                Err(_) => {
                    return Json(
                        json!({"code": 400, "message": "expires_at must be an RFC 3339 timestamp"}),
                    )
                }
            };
            if parsed <= Utc::now().naive_utc() {
                return Json(json!({"code": 400, "message": "expires_at must be in the future"}));
            }
            Some(parsed)
        }
        _ => None,
    };
    let raw_ip_allowlist = input
        .get("ip_allowlist")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    if raw_ip_allowlist.len() > 100 {
        return Json(json!({"code": 400, "message": "ip_allowlist supports at most 100 entries"}));
    }
    let mut ip_allowlist = Vec::with_capacity(raw_ip_allowlist.len());
    for value in raw_ip_allowlist {
        let Some(value) = value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return Json(
                json!({"code": 400, "message": "ip_allowlist entries must be non-empty strings"}),
            );
        };
        let canonical = match value.parse::<ipnet::IpNet>() {
            Ok(network) => network.to_string(),
            Err(_) => match value.parse::<std::net::IpAddr>() {
                Ok(address) => ipnet::IpNet::from(address).to_string(),
                Err(_) => {
                    return Json(
                        json!({"code": 400, "message": format!("Invalid IP or CIDR: {}", value)}),
                    )
                }
            },
        };
        ip_allowlist.push(canonical);
    }

    let prefix = key_prefix_seed(&name);
    let (raw_key, key_hash) = crate::utils::key_generator::generate_api_key(prefix);
    let key_prefix = crate::utils::key_generator::get_key_prefix(&raw_key);
    let uid = crate::utils::id_generator::generate_key_id();
    let project_id = match input.get("project_uid").and_then(|value| value.as_str()) {
        Some(project_uid) if !project_uid.trim().is_empty() => {
            match sqlx::query_scalar(
                "SELECT id FROM projects
                 WHERE workspace_id = $1 AND uid = $2 AND status = 0",
            )
            .bind(user.workspace_id)
            .bind(project_uid.trim())
            .fetch_optional(&pool)
            .await
            {
                Ok(Some(id)) => id,
                Ok(None) => return Json(json!({"code": 404, "message": "Project not found"})),
                Err(e) => return Json(json!({"code": 500, "message": sanitize_error(e)})),
            }
        }
        _ => user.project_id,
    };

    let api_key = match repo
        .create_for_workspace(
            &uid,
            user_id,
            user.workspace_id,
            project_id,
            &key_prefix,
            &key_hash,
            &name,
            rate_limit,
            serde_json::json!(models.unwrap_or_else(|| vec!["gpt-4.1-mini".to_string()])),
            daily_spend_limit,
            monthly_spend_limit,
            total_spend_limit,
            expires_at,
            &ip_allowlist,
        )
        .await
    {
        Ok(k) => k,
        Err(e) => return Json(json!({"code": 500, "message": sanitize_error(e)})),
    };

    Json(json!({
        "code": 0,
        "message": "创建成功",
        "data": {
            "uid": api_key.uid,
            "key": raw_key,  // 只返回一次
            "name": api_key.name,
            "rate_limit": api_key.rate_limit,
            "models_allowed": api_key.models_allowed,
            "daily_spend_limit": daily_spend_limit,
            "monthly_spend_limit": monthly_spend_limit,
            "total_spend_limit": total_spend_limit,
            "expires_at": expires_at,
            "ip_allowlist": ip_allowlist,
            "project_id": project_id,
        }
    }))
}

async fn list_keys_handler(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Json<serde_json::Value> {
    let user_id = user.user_id;

    let rows = match sqlx::query(
        "SELECT k.uid, k.key_prefix, k.name, k.rate_limit, k.models_allowed, k.last_used_at, k.status,
                CAST(daily_spend_limit AS VARCHAR), CAST(monthly_spend_limit AS VARCHAR),
                CAST(total_spend_limit AS VARCHAR), k.expires_at, k.ip_allowlist::TEXT[],
                p.uid, p.name
         FROM api_keys k
         JOIN projects p ON p.id = k.project_id AND p.workspace_id = k.workspace_id
         WHERE k.user_id = $1 AND k.workspace_id = $2 AND k.status IN (0, 2)
         ORDER BY k.created_at DESC",
    )
    .bind(user_id)
    .bind(user.workspace_id)
    .fetch_all(&pool)
    .await
    {
        Ok(k) => k,
        Err(e) => return Json(json!({"code": 500, "message": sanitize_error(e)})),
    };

    let items: Vec<_> = rows
        .iter()
        .map(|k| {
            json!({
                "uid": k.get::<String, _>(0),
                "key_prefix": k.get::<String, _>(1),
                "name": k.get::<Option<String>, _>(2),
                "rate_limit": k.get::<i32, _>(3),
                "models_allowed": k.get::<Option<serde_json::Value>, _>(4),
                "last_used_at": k.get::<Option<chrono::NaiveDateTime>, _>(5),
                "status": k.get::<i16, _>(6),
                "daily_spend_limit": k.get::<Option<String>, _>(7).and_then(|value| value.parse::<f64>().ok()),
                "monthly_spend_limit": k.get::<Option<String>, _>(8).and_then(|value| value.parse::<f64>().ok()),
                "total_spend_limit": k.get::<Option<String>, _>(9).and_then(|value| value.parse::<f64>().ok()),
                "expires_at": k.get::<Option<chrono::NaiveDateTime>, _>(10),
                "ip_allowlist": k.get::<Vec<String>, _>(11),
                "project_uid": k.get::<String, _>(12),
                "project_name": k.get::<String, _>(13),
            })
        })
        .collect();

    Json(json!({ "code": 0, "data": { "items": items } }))
}

async fn update_key_handler(
    Path(uid): Path<String>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
    Json(input): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let user_id = user.user_id;
    let repo = crate::repository::api_key_repo::ApiKeyRepository::new(&pool);

    // Find the key first
    let key = match repo
        .find_by_uid_in_workspace(&uid, user_id, user.workspace_id)
        .await
    {
        Ok(Some(k)) => k,
        Ok(None) => return Json(json!({"code": 404, "message": "Key不存在"})),
        Err(e) => return Json(json!({"code": 500, "message": sanitize_error(e)})),
    };

    // Extract fields from input
    let name_raw = input
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or(key.name.as_deref().unwrap_or("default"));
    let name = match clean_display_text(name_raw, "Key name") {
        Ok(value) => value,
        Err(message) => return Json(json!({"code": 400, "message": message})),
    };
    let rate_limit = input
        .get("rate_limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(key.rate_limit as i64) as i32;
    if !(1..=10_000).contains(&rate_limit) {
        return Json(json!({"code": 400, "message": "Rate limit must be between 1 and 10000"}));
    }

    // Update
    match repo.update(key.id, &name, rate_limit).await {
        Ok(_) => Json(json!({"code": 0, "message": "更新成功", "data": {"uid": uid}})),
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn set_key_status_handler(
    uid: String,
    status: i16,
    pool: PgPool,
    user: CurrentUser,
) -> Json<serde_json::Value> {
    let repo = crate::repository::api_key_repo::ApiKeyRepository::new(&pool);

    let key = match repo
        .find_by_uid_in_workspace(&uid, user.user_id, user.workspace_id)
        .await
    {
        Ok(Some(k)) => k,
        Ok(None) => return Json(json!({"code": 404, "message": "Key不存在"})),
        Err(e) => return Json(json!({"code": 500, "message": sanitize_error(e)})),
    };

    match repo.set_status(key.id, status).await {
        Ok(_) => Json(json!({
            "code": 0,
            "message": if status == 0 { "Key enabled" } else { "Key disabled" },
            "data": {"uid": uid, "status": status}
        })),
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn disable_key_handler(
    Path(uid): Path<String>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Json<serde_json::Value> {
    set_key_status_handler(uid, 2, pool, user).await
}

async fn enable_key_handler(
    Path(uid): Path<String>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Json<serde_json::Value> {
    set_key_status_handler(uid, 0, pool, user).await
}

async fn delete_key_handler(
    Path(uid): Path<String>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Json<serde_json::Value> {
    let user_id = user.user_id;
    let repo = crate::repository::api_key_repo::ApiKeyRepository::new(&pool);

    // Find the key first
    let key = match repo
        .find_by_uid_in_workspace(&uid, user_id, user.workspace_id)
        .await
    {
        Ok(Some(k)) => k,
        Ok(None) => return Json(json!({"code": 404, "message": "Key不存在"})),
        Err(e) => return Json(json!({"code": 500, "message": sanitize_error(e)})),
    };

    // Soft delete (status = 1)
    match repo.soft_delete(key.id).await {
        Ok(_) => Json(json!({"code": 0, "message": "删除成功"})),
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn list_provider_credentials(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Response {
    let provider_rows = match sqlx::query(
        "SELECT id, provider_id, name, credential_mode, status
         FROM providers
         WHERE credential_mode = 'byok' AND status = 0
         ORDER BY sort, id",
    )
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code": 500, "message": sanitize_error(error)})),
            )
                .into_response()
        }
    };
    let rows = match sqlx::query(
        "SELECT pc.uid, pc.provider_id, p.name, pc.name, pc.key_prefix,
                pc.encryption_key_id, pc.status,
                pc.last_used_at, pc.created_at, pc.updated_at
         FROM provider_credentials pc
         JOIN providers p ON p.id = pc.provider_id
         WHERE pc.workspace_id = $1 AND pc.status = 0
         ORDER BY pc.created_at DESC",
    )
    .bind(user.workspace_id)
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => rows,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code": 500, "message": sanitize_error(error)})),
            )
                .into_response()
        }
    };

    let items: Vec<byok::ProviderCredentialMetadata> = rows
        .iter()
        .map(|row| byok::ProviderCredentialMetadata {
            uid: row.get(0),
            provider_id: row.get(1),
            provider_name: row.get(2),
            name: row.get(3),
            key_prefix: row.get(4),
            encryption_key_id: row.get(5),
            status: row.get(6),
            last_used_at: row.get(7),
            created_at: row.get(8),
            updated_at: row.get(9),
        })
        .collect();
    let providers: Vec<_> = provider_rows
        .iter()
        .map(|row| {
            json!({
                "id": row.get::<i64, _>(0),
                "provider_id": row.get::<String, _>(1),
                "name": row.get::<String, _>(2),
                "credential_mode": row.get::<String, _>(3),
                "status": row.get::<i16, _>(4),
            })
        })
        .collect();

    Json(json!({
        "code": 0,
        "data": {
            "providers": providers,
            "credentials": &items,
            "items": &items
        }
    }))
    .into_response()
}

async fn create_provider_credential(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
    Json(input): Json<serde_json::Value>,
) -> Response {
    if user.workspace_role != "admin" {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"code": 403, "message": "Workspace admin permission required"})),
        )
            .into_response();
    }
    let provider_id = match input.get("provider_id").and_then(|value| value.as_i64()) {
        Some(value) if value > 0 => value,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"code": 400, "message": "provider_id is required"})),
            )
                .into_response()
        }
    };
    let name = match input.get("name").and_then(|value| value.as_str()) {
        Some(value) if !value.trim().is_empty() && value.chars().count() <= 100 => value.trim(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"code": 400, "message": "name must contain 1 to 100 characters"})),
            )
                .into_response()
        }
    };
    let api_key = match input.get("api_key").and_then(|value| value.as_str()) {
        Some(value) if !value.is_empty() && value.len() <= 8192 => value,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"code": 400, "message": "api_key must contain 1 to 8192 bytes"})),
            )
                .into_response()
        }
    };

    let provider_name = match sqlx::query_scalar::<_, String>(
        "SELECT name FROM providers WHERE id = $1 AND credential_mode = 'byok' AND status = 0",
    )
    .bind(provider_id)
    .fetch_optional(&pool)
    .await
    {
        Ok(Some(value)) => value,
        Ok(None) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"code": 400, "message": "provider is not configured for BYOK"})),
            )
                .into_response()
        }
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code": 500, "message": sanitize_error(error)})),
            )
                .into_response()
        }
    };

    let cipher = match byok::ByokCipher::from_env() {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(error = %error, "BYOK master key is unavailable");
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"code": 503, "message": "BYOK encryption is unavailable"})),
            )
                .into_response();
        }
    };
    let encrypted = match cipher.encrypt(api_key) {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(error = %error, "Failed to encrypt provider credential");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code": 500, "message": "Credential encryption failed"})),
            )
                .into_response();
        }
    };
    let uid = format!("PCR{}", crate::utils::id_generator::generate_request_id());
    let key_prefix = byok::prefix(api_key);
    let key_fingerprint = byok::fingerprint(api_key);

    let row = match sqlx::query(
        "INSERT INTO provider_credentials
         (uid, workspace_id, provider_id, name, key_prefix, key_fingerprint,
          ciphertext, nonce, encryption_key_id, status, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 0, $10)
         ON CONFLICT (workspace_id, provider_id) DO UPDATE SET
             name = EXCLUDED.name,
             key_prefix = EXCLUDED.key_prefix,
             key_fingerprint = EXCLUDED.key_fingerprint,
             ciphertext = EXCLUDED.ciphertext,
             nonce = EXCLUDED.nonce,
             encryption_key_id = EXCLUDED.encryption_key_id,
             status = 0,
             created_by = EXCLUDED.created_by,
             updated_at = NOW()
         RETURNING uid, provider_id, name, key_prefix, encryption_key_id,
                   status, last_used_at, created_at, updated_at",
    )
    .bind(uid)
    .bind(user.workspace_id)
    .bind(provider_id)
    .bind(name)
    .bind(key_prefix)
    .bind(key_fingerprint)
    .bind(encrypted.ciphertext)
    .bind(encrypted.nonce.as_slice())
    .bind(cipher.key_id())
    .bind(user.user_id)
    .fetch_one(&pool)
    .await
    {
        Ok(row) => row,
        Err(error) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"code": 500, "message": sanitize_error(error)})),
            )
                .into_response()
        }
    };

    let metadata = byok::ProviderCredentialMetadata {
        uid: row.get(0),
        provider_id: row.get(1),
        provider_name,
        name: row.get(2),
        key_prefix: row.get(3),
        encryption_key_id: row.get(4),
        status: row.get(5),
        last_used_at: row.get(6),
        created_at: row.get(7),
        updated_at: row.get(8),
    };

    (
        StatusCode::CREATED,
        Json(json!({"code": 0, "data": metadata})),
    )
        .into_response()
}

async fn delete_provider_credential(
    Path(uid): Path<String>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Response {
    if user.workspace_role != "admin" {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"code": 403, "message": "Workspace admin permission required"})),
        )
            .into_response();
    }
    let uid = uid.trim();
    if uid.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"code": 400, "message": "uid is required"})),
        )
            .into_response();
    }

    match sqlx::query(
        "UPDATE provider_credentials
         SET status = 1, updated_at = NOW()
         WHERE uid = $1 AND workspace_id = $2 AND status = 0",
    )
    .bind(uid)
    .bind(user.workspace_id)
    .execute(&pool)
    .await
    {
        Ok(result) if result.rows_affected() == 1 => {
            Json(json!({"code": 0, "message": "Credential deleted"})).into_response()
        }
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({"code": 404, "message": "Credential not found"})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"code": 500, "message": sanitize_error(error)})),
        )
            .into_response(),
    }
}

// Packages
async fn list_packages(State(pool): State<PgPool>) -> Json<serde_json::Value> {
    use sqlx::Row;
    match sqlx::query(
        "SELECT id, package_id, name, CAST(price AS VARCHAR), token_quota, bonus_token, duration_days
         FROM packages WHERE status = 0 ORDER BY sort"
    )
    .fetch_all(&pool)
    .await {
        Ok(rows) => {
            let items: Vec<_> = rows.iter().map(|r| json!({
                "id": r.get::<String, _>(1),
                "name": r.get::<String, _>(2),
                "price": r.get::<String, _>(3),
                "tokens": r.get::<i32, _>(4),
                "bonus_tokens": r.get::<i32, _>(5),
                "duration_days": r.get::<i32, _>(6)
            })).collect();
            Json(json!({"code": 0, "data": {"items": items}}))
        }
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}
// Models (Stage 2.5)
async fn list_models(State(pool): State<PgPool>) -> Json<serde_json::Value> {
    use sqlx::Row;
    match sqlx::query(
        "SELECT lm.model_id, lm.display_name,
                CAST(lm.input_price_per_million AS VARCHAR),
                CAST(lm.output_price_per_million AS VARCHAR),
                lm.context_len, lm.max_tokens, lm.vendor,
                lm.input_modalities, lm.output_modalities, lm.supported_parameters
         FROM logical_models lm
         WHERE lm.status = 0
           AND EXISTS (
               SELECT 1 FROM provider_models pm
               JOIN providers p ON p.id = pm.provider_id
               WHERE pm.logical_model_id = lm.id AND pm.status = 0 AND p.status = 0
           )
         ORDER BY lm.sort, lm.id",
    )
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => {
            let items: Vec<_> = rows
                .iter()
                .map(|r| {
                    json!({
                        "model_id": r.get::<String, _>(0),
                        "display_name": r.get::<String, _>(1),
                        "input_rate": r.get::<String, _>(2),
                        "output_rate": r.get::<String, _>(3),
                        "context_len": r.get::<i32, _>(4),
                        "max_tokens": r.get::<i32, _>(5),
                        "provider": r.get::<String, _>(6),
                        "vendor": r.get::<String, _>(6),
                        "input_modalities": r.get::<serde_json::Value, _>(7),
                        "output_modalities": r.get::<serde_json::Value, _>(8),
                        "supported_parameters": r.get::<serde_json::Value, _>(9),
                        "rate_unit": "usd_per_million_tokens"
                    })
                })
                .collect();
            Json(json!({"code": 0, "data": {"items": items}}))
        }
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn list_gateway_models(State(pool): State<PgPool>) -> Response {
    use sqlx::Row;

    let request_id = crate::utils::id_generator::generate_request_id();

    let response = match sqlx::query(
        "SELECT lm.model_id, lm.vendor, lm.created_at, lm.context_len, lm.max_tokens,
                CAST(lm.input_price_per_million AS VARCHAR),
                CAST(lm.output_price_per_million AS VARCHAR),
                lm.input_modalities, lm.output_modalities, lm.supported_parameters
         FROM logical_models lm
         WHERE lm.status = 0
           AND EXISTS (
               SELECT 1 FROM provider_models pm
               JOIN providers p ON p.id = pm.provider_id
               WHERE pm.logical_model_id = lm.id AND pm.status = 0 AND p.status = 0
           )
         ORDER BY lm.sort, lm.id",
    )
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => {
            let data: Vec<_> = rows
                .iter()
                .map(|row| {
                    json!({
                        "id": row.get::<String, _>(0),
                        "object": "model",
                        "created": row.get::<chrono::NaiveDateTime, _>(2).and_utc().timestamp(),
                        "owned_by": row.get::<String, _>(1),
                        "context_length": row.get::<i32, _>(3),
                        "max_output_tokens": row.get::<i32, _>(4),
                        "pricing": {
                            "input_per_million": row.get::<String, _>(5),
                            "output_per_million": row.get::<String, _>(6),
                            "currency": "USD"
                        },
                        "input_modalities": row.get::<serde_json::Value, _>(7),
                        "output_modalities": row.get::<serde_json::Value, _>(8),
                        "supported_parameters": row.get::<serde_json::Value, _>(9)
                    })
                })
                .collect();
            Json(json!({"object": "list", "data": data})).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to list gateway models");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": {"code": "MODEL_LIST_FAILED", "message": "Failed to load models", "retryable": true}
                })),
            )
                .into_response()
        }
    };

    with_nexus_headers(response, &request_id, None, None)
}

// Orders
async fn create_order(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
    Json(input): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    use sqlx::Row;

    let user_id = user.user_id;
    let package_id = match input.get("package_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return Json(json!({"code": 400, "message": "package_id is required"})),
    };

    // Get package info
    let package = match sqlx::query(
        "SELECT package_id, name, CAST(price AS VARCHAR) FROM packages WHERE package_id = $1 AND status = 0"
    )
    .bind(package_id)
    .fetch_optional(&pool)
    .await {
        Ok(Some(row)) => row,
        Ok(None) => return Json(json!({"code": 404, "message": "Package not found"})),
        Err(e) => return Json(json!({"code": 500, "message": sanitize_error(e)})),
    };

    let pkg_id: String = package.get(0);
    let pkg_name: String = package.get(1);
    let price: f64 = package
        .get::<String, _>(2)
        .parse()
        .expect("database package price must parse");

    // Generate order_no
    let order_no = crate::utils::id_generator::generate_order_no();

    // Insert order
    let result = sqlx::query(
        "INSERT INTO overseas_orders (order_no, user_id, workspace_id, package_id, amount, actual_amount, payment_status)
         VALUES ($1, $2, $3, (SELECT id FROM packages WHERE package_id = $4), $5, $5, 0)
         RETURNING id"
    )
    .bind(&order_no)
    .bind(user_id)
    .bind(user.workspace_id)
    .bind(package_id)
    .bind(price)
    .fetch_one(&pool)
    .await;

    match result {
        Ok(row) => {
            let order_id: i64 = row.get(0);
            Json(json!({
                "code": 0,
                "message": "Order created",
                "data": {
                    "order_id": order_id,
                    "order_no": order_no,
                    "package_id": pkg_id,
                    "package_name": pkg_name,
                    "amount": price,
                    "order_url": format!("/orders/{}", order_id)
                }
            }))
        }
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn list_orders(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Json<serde_json::Value> {
    use sqlx::Row;
    match sqlx::query(
        "SELECT o.order_no, CAST(o.amount AS VARCHAR), o.payment_status, o.created_at, p.name as package_name
         FROM overseas_orders o
         JOIN packages p ON o.package_id = p.id
         WHERE o.workspace_id = $1
         ORDER BY o.created_at DESC"
    )
    .bind(user.workspace_id)
    .fetch_all(&pool)
    .await {
        Ok(rows) => {
            let items: Vec<_> = rows.iter().map(|r| json!({
                "order_no": r.get::<String, _>(0),
                "amount": r.get::<String, _>(1),
                "status": r.get::<i16, _>(2),
                "created_at": r.get::<chrono::NaiveDateTime, _>(3),
                "package_name": r.get::<String, _>(4)
            })).collect();
            Json(json!({"code": 0, "data": {"items": items}}))
        }
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

fn webhook_response(status: StatusCode, code: &str, message: &str) -> Response {
    (status, Json(json!({"code": code, "message": message}))).into_response()
}

/// 使用标准 HMAC-SHA256 校验原始请求体，避免只签订单号造成字段被篡改。
fn hmac_sha256(secret: &[u8], message: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};

    const BLOCK_SIZE: usize = 64;
    let mut key_block = [0_u8; BLOCK_SIZE];
    if secret.len() > BLOCK_SIZE {
        key_block[..32].copy_from_slice(&Sha256::digest(secret));
    } else {
        key_block[..secret.len()].copy_from_slice(secret);
    }
    let mut inner_pad = [0x36_u8; BLOCK_SIZE];
    let mut outer_pad = [0x5c_u8; BLOCK_SIZE];
    for index in 0..BLOCK_SIZE {
        inner_pad[index] ^= key_block[index];
        outer_pad[index] ^= key_block[index];
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(message);
    let inner_hash = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_hash);
    outer.finalize().into()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

fn webhook_amount_text(value: &serde_json::Value) -> Option<String> {
    let text = match value {
        serde_json::Value::String(value) => value.trim().to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        _ => return None,
    };
    let amount = text.parse::<f64>().ok()?;
    (amount.is_finite() && amount > 0.0).then_some(text)
}

// 自动支付回调：未配置密钥时关闭；启用后强制原始请求体签名、金额核对和事件幂等。
async fn payment_webhook(
    State(pool): State<PgPool>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    use sha2::{Digest, Sha256};
    use sqlx::Row;

    let webhook_secret = std::env::var("PAYMENT_WEBHOOK_SECRET").unwrap_or_default();
    if webhook_secret.len() < 32 {
        return webhook_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "PAYMENT_WEBHOOK_DISABLED",
            "Payment webhook is not configured",
        );
    }
    let provided_signature = headers
        .get("x-webhook-signature")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("sha256=").or(Some(value)))
        .unwrap_or("")
        .to_ascii_lowercase();
    let expected_signature = hex::encode(hmac_sha256(webhook_secret.as_bytes(), &body));
    if !constant_time_eq(provided_signature.as_bytes(), expected_signature.as_bytes()) {
        tracing::warn!("Rejected payment webhook with invalid signature");
        return webhook_response(
            StatusCode::UNAUTHORIZED,
            "INVALID_WEBHOOK_SIGNATURE",
            "Invalid webhook signature",
        );
    }

    let input: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(input) => input,
        Err(_) => {
            return webhook_response(
                StatusCode::BAD_REQUEST,
                "INVALID_WEBHOOK_PAYLOAD",
                "Invalid JSON payload",
            )
        }
    };
    let required_text = |field: &str| {
        input
            .get(field)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    let Some(provider) = required_text("provider") else {
        return webhook_response(
            StatusCode::BAD_REQUEST,
            "INVALID_WEBHOOK_PAYLOAD",
            "provider is required",
        );
    };
    let Some(event_id) = required_text("event_id") else {
        return webhook_response(
            StatusCode::BAD_REQUEST,
            "INVALID_WEBHOOK_PAYLOAD",
            "event_id is required",
        );
    };
    let Some(order_no) = required_text("order_no") else {
        return webhook_response(
            StatusCode::BAD_REQUEST,
            "INVALID_WEBHOOK_PAYLOAD",
            "order_no is required",
        );
    };
    if provider.len() > 40 || event_id.len() > 128 || order_no.len() > 32 {
        return webhook_response(
            StatusCode::BAD_REQUEST,
            "INVALID_WEBHOOK_PAYLOAD",
            "Webhook field is too long",
        );
    }
    if required_text("status") != Some("paid") {
        return webhook_response(
            StatusCode::BAD_REQUEST,
            "PAYMENT_NOT_CONFIRMED",
            "Only paid events are accepted",
        );
    }
    let Some(amount_text) = input.get("amount").and_then(webhook_amount_text) else {
        return webhook_response(
            StatusCode::BAD_REQUEST,
            "INVALID_WEBHOOK_PAYLOAD",
            "A positive amount is required",
        );
    };
    let payload_hash = hex::encode(Sha256::digest(&body));

    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            tracing::error!(error = %e, "Failed to start payment transaction");
            return webhook_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "PAYMENT_FAILED",
                "Payment processing failed",
            );
        }
    };
    let event = match sqlx::query(
        "INSERT INTO payment_events (provider, event_id, payload_hash, status)
         VALUES ($1, $2, $3, 'received')
         ON CONFLICT (provider, event_id) DO NOTHING
         RETURNING id",
    )
    .bind(provider)
    .bind(event_id)
    .bind(&payload_hash)
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(event) => event,
        Err(e) => {
            tracing::error!(error = %e, "Failed to record payment event");
            return webhook_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "PAYMENT_FAILED",
                "Payment processing failed",
            );
        }
    };
    let event_id_db = if let Some(event) = event {
        event.get::<i64, _>(0)
    } else {
        let existing = sqlx::query(
            "SELECT payload_hash, status FROM payment_events WHERE provider = $1 AND event_id = $2",
        )
        .bind(provider)
        .bind(event_id)
        .fetch_optional(&mut *tx)
        .await;
        let _ = tx.rollback().await;
        return match existing {
            Ok(Some(row))
                if row.get::<String, _>(0) == payload_hash
                    && row.get::<String, _>(1) == "processed" =>
            {
                Json(json!({"code": 0, "message": "Already processed"})).into_response()
            }
            Ok(Some(_)) => webhook_response(
                StatusCode::CONFLICT,
                "PAYMENT_EVENT_CONFLICT",
                "Payment event conflicts with an existing event",
            ),
            Ok(None) => webhook_response(
                StatusCode::CONFLICT,
                "PAYMENT_EVENT_CONFLICT",
                "Payment event is already being processed",
            ),
            Err(e) => {
                tracing::error!(error = %e, "Failed to inspect duplicate payment event");
                webhook_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "PAYMENT_FAILED",
                    "Payment processing failed",
                )
            }
        };
    };

    let settlement = match settle_order_payment_tx(&mut tx, order_no, Some(&amount_text)).await {
        Ok(settlement) => settlement,
        Err(error) => {
            let _ = tx.rollback().await;
            return settlement_error_response(error);
        }
    };
    if let Err(e) = sqlx::query(
        "UPDATE payment_events
         SET order_id = $1, status = 'processed', processed_at = NOW()
         WHERE id = $2",
    )
    .bind(settlement.order_id)
    .bind(event_id_db)
    .execute(&mut *tx)
    .await
    {
        tracing::error!(error = %e, "Failed to finish payment event");
        let _ = tx.rollback().await;
        return webhook_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "PAYMENT_FAILED",
            "Payment processing failed",
        );
    }
    if let Err(e) = tx.commit().await {
        tracing::error!(error = %e, "Failed to commit payment transaction");
        return webhook_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "PAYMENT_FAILED",
            "Payment processing failed",
        );
    }

    Json(json!({
        "code": 0,
        "message": "Payment processed",
        "data": {
            "order_no": order_no,
            "payment_status": 1,
            "balance_added": settlement.amount,
            "currency": "USD"
        }
    }))
    .into_response()
}

// Balance
async fn get_balance(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Json<serde_json::Value> {
    use sqlx::Row;
    match sqlx::query("SELECT CAST(balance AS VARCHAR), CAST(frozen_balance AS VARCHAR), CAST(total_recharged AS VARCHAR), CAST(total_consumed AS VARCHAR) FROM balances WHERE workspace_id = $1")
        .bind(user.workspace_id)
        .fetch_optional(&pool)
        .await {
        Ok(Some(row)) => Json(json!({
            "code": 0,
            "data": {
                "balance": row.get::<String, _>(0),
                "frozen_balance": row.get::<String, _>(1),
                "total_recharged": row.get::<String, _>(2),
                "total_consumed": row.get::<String, _>(3)
            }
        })),
        Ok(None) => Json(json!({
            "code": 500,
            "message": "Workspace balance record is missing"
        })),
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn balance_logs(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Json<serde_json::Value> {
    use sqlx::Row;
    match sqlx::query("SELECT change_type, change_amount, balance_before, balance_after, created_at FROM balance_logs WHERE workspace_id = $1 ORDER BY created_at DESC LIMIT 20")
        .bind(user.workspace_id)
        .fetch_all(&pool)
        .await {
        Ok(rows) => {
            let items: Vec<_> = rows.iter().map(|r| json!({
                "type": r.get::<String, _>(0),
                "amount": r.get::<String, _>(1),
                "balance_before": r.get::<String, _>(2),
                "balance_after": r.get::<String, _>(3),
                "created_at": r.get::<chrono::NaiveDateTime, _>(4)
            })).collect();
            Json(json!({"code": 0, "data": {"items": items}}))
        }
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

fn is_model_allowed_for_key(models_allowed: &Option<serde_json::Value>, model: &str) -> bool {
    match models_allowed {
        None => true,
        Some(serde_json::Value::Array(models)) => {
            models.iter().any(|item| item.as_str() == Some(model))
        }
        _ => false,
    }
}

fn clean_display_text(input: &str, field_name: &str) -> Result<String, String> {
    let trimmed = input.trim();
    let char_len = trimmed.chars().count();
    if char_len == 0 {
        return Err(format!("{} is required", field_name));
    }
    if char_len > MAX_DISPLAY_TEXT_LEN {
        return Err(format!(
            "{} must be {} characters or fewer",
            field_name, MAX_DISPLAY_TEXT_LEN
        ));
    }

    let mut output = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        match ch {
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '&' => output.push_str("&amp;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#x27;"),
            ch if ch.is_control() => {}
            ch => output.push(ch),
        }
    }

    Ok(output)
}

fn key_prefix_seed(name: &str) -> &str {
    name.char_indices()
        .nth(8)
        .map(|(index, _)| &name[..index])
        .unwrap_or(name)
}

fn parse_redis_host_port(redis_url: &str) -> Result<String, String> {
    let without_scheme = redis_url
        .strip_prefix("redis://")
        .ok_or_else(|| "REDIS_URL must start with redis://".to_string())?;
    let authority = without_scheme.split('/').next().unwrap_or(without_scheme);
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    if host_port.is_empty() {
        return Err("REDIS_URL host is empty".to_string());
    }
    if host_port.contains(':') {
        Ok(host_port.to_string())
    } else {
        Ok(format!("{}:6379", host_port))
    }
}

fn build_redis_command(args: &[String]) -> Vec<u8> {
    let mut command = format!("*{}\r\n", args.len()).into_bytes();
    for arg in args {
        command.extend_from_slice(format!("${}\r\n", arg.len()).as_bytes());
        command.extend_from_slice(arg.as_bytes());
        command.extend_from_slice(b"\r\n");
    }
    command
}

async fn redis_integer_command(args: Vec<String>) -> Result<i64, String> {
    let address = parse_redis_host_port(&config::redis_url())?;
    let mut stream = TcpStream::connect(&address)
        .await
        .map_err(|e| format!("Redis connect failed: {}", e))?;
    let command = build_redis_command(&args);
    stream
        .write_all(&command)
        .await
        .map_err(|e| format!("Redis write failed: {}", e))?;

    let mut buffer = vec![0_u8; 512];
    let read = stream
        .read(&mut buffer)
        .await
        .map_err(|e| format!("Redis read failed: {}", e))?;
    if read == 0 {
        return Err("Redis returned empty response".to_string());
    }

    let response = String::from_utf8_lossy(&buffer[..read]);
    if let Some(value) = response.strip_prefix(':') {
        return value
            .trim()
            .parse::<i64>()
            .map_err(|e| format!("Redis integer parse failed: {}", e));
    }
    if let Some(error) = response.strip_prefix('-') {
        return Err(format!("Redis error: {}", error.trim()));
    }
    Err(format!("Unexpected Redis response: {}", response.trim()))
}

async fn redis_optional_integer_command(args: Vec<String>) -> Result<Option<i64>, String> {
    let address = parse_redis_host_port(&config::redis_url())?;
    let mut stream = TcpStream::connect(&address)
        .await
        .map_err(|e| format!("Redis connect failed: {}", e))?;
    let command = build_redis_command(&args);
    stream
        .write_all(&command)
        .await
        .map_err(|e| format!("Redis write failed: {}", e))?;

    let mut buffer = vec![0_u8; 512];
    let read = stream
        .read(&mut buffer)
        .await
        .map_err(|e| format!("Redis read failed: {}", e))?;
    if read == 0 {
        return Err("Redis returned empty response".to_string());
    }

    let response = String::from_utf8_lossy(&buffer[..read]);
    if response.starts_with("$-1\r\n") {
        return Ok(None);
    }
    if let Some(payload) = response.strip_prefix('$') {
        let Some((length, value)) = payload.split_once("\r\n") else {
            return Err("Redis bulk response is incomplete".to_string());
        };
        let length = length
            .parse::<usize>()
            .map_err(|e| format!("Redis bulk length parse failed: {}", e))?;
        let Some(value) = value.get(..length) else {
            return Err("Redis bulk response is incomplete".to_string());
        };
        return value
            .parse::<i64>()
            .map(Some)
            .map_err(|e| format!("Redis integer parse failed: {}", e));
    }
    if let Some(error) = response.strip_prefix('-') {
        return Err(format!("Redis error: {}", error.trim()));
    }
    Err(format!("Unexpected Redis response: {}", response.trim()))
}

async fn enforce_rate_limit(user: &CurrentUser) -> Result<Option<RateLimitState>, Response> {
    let Some(api_key_id) = user.api_key_id else {
        return Ok(None);
    };

    let limit = user
        .rate_limit
        .expect("Authenticated API key must include a rate limit")
        .clamp(1, 10_000);
    let key = format!("nexus:rate_limit:api_key:{}", api_key_id);
    let count = match redis_integer_command(vec!["INCR".to_string(), key.clone()]).await {
        Ok(value) => value,
        Err(e) => {
            tracing::error!(error = %e, "Rate limiter increment failed");
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": {
                        "message": "Rate limiter unavailable",
                        "type": "rate_limiter_unavailable",
                        "code": "RATE_LIMITER_UNAVAILABLE"
                    }
                })),
            )
                .into_response());
        }
    };

    if count == 1 {
        if let Err(e) =
            redis_integer_command(vec!["EXPIRE".to_string(), key.clone(), "3600".to_string()]).await
        {
            tracing::error!(error = %e, "Rate limiter expiry setup failed");
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": {
                        "message": "Rate limiter unavailable",
                        "type": "rate_limiter_unavailable",
                        "code": "RATE_LIMITER_UNAVAILABLE"
                    }
                })),
            )
                .into_response());
        }
    }

    let ttl = match redis_integer_command(vec!["TTL".to_string(), key]).await {
        Ok(value) if value >= 0 => value.max(1),
        Ok(value) => {
            tracing::error!(ttl = value, "Rate limiter returned an invalid expiry");
            return Err(api_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Rate limiter unavailable",
                "rate_limiter_unavailable",
                "RATE_LIMITER_UNAVAILABLE",
            ));
        }
        Err(e) => {
            tracing::error!(error = %e, "Rate limiter expiry read failed");
            return Err(api_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Rate limiter unavailable",
                "rate_limiter_unavailable",
                "RATE_LIMITER_UNAVAILABLE",
            ));
        }
    };
    let remaining = (limit as i64 - count).max(0) as i32;
    let state = RateLimitState {
        limit,
        remaining,
        reset_secs: ttl,
    };

    if count > limit as i64 {
        let mut response = (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": {
                    "message": "API key rate limit exceeded",
                    "type": "rate_limit_exceeded",
                    "code": "RATE_LIMIT_EXCEEDED"
                }
            })),
        )
            .into_response();
        add_rate_limit_headers(&mut response, Some(&state), true);
        return Err(response);
    }

    Ok(Some(state))
}

fn add_rate_limit_headers(
    response: &mut Response,
    state: Option<&RateLimitState>,
    retry_after: bool,
) {
    let Some(state) = state else {
        return;
    };
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(&state.limit.to_string()) {
        headers.insert(HeaderName::from_static("x-ratelimit-limit"), value);
    }
    if let Ok(value) = HeaderValue::from_str(&state.remaining.to_string()) {
        headers.insert(HeaderName::from_static("x-ratelimit-remaining"), value);
    }
    if let Ok(value) = HeaderValue::from_str(&state.reset_secs.to_string()) {
        headers.insert(HeaderName::from_static("x-ratelimit-reset"), value.clone());
        if retry_after {
            headers.insert(HeaderName::from_static("retry-after"), value);
        }
    }
}

fn with_rate_limit_headers(
    mut response: Response,
    state: Option<&RateLimitState>,
    retry_after: bool,
) -> Response {
    add_rate_limit_headers(&mut response, state, retry_after);
    response
}

fn with_nexus_headers(
    mut response: Response,
    request_id: &str,
    provider: Option<&str>,
    routing_policy: Option<&str>,
) -> Response {
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(request_id) {
        headers.insert(HeaderName::from_static("x-nexus-request-id"), value);
    }
    if let Some(provider) = provider {
        if let Ok(value) = HeaderValue::from_str(provider) {
            headers.insert(HeaderName::from_static("x-nexus-provider"), value);
        }
    }
    if let Some(policy) = routing_policy {
        if let Ok(value) = HeaderValue::from_str(policy) {
            headers.insert(HeaderName::from_static("x-nexus-routing-policy"), value);
        }
    }
    response
}

fn expected_provider_host(provider_code: &str) -> Option<&'static str> {
    match provider_code {
        "openai" => Some("api.openai.com"),
        "anthropic" => Some("api.anthropic.com"),
        "google" => Some("generativelanguage.googleapis.com"),
        "volcengine" => Some("ark.cn-beijing.volces.com"),
        "deepseek" => Some("api.deepseek.com"),
        "zhipu" => Some("open.bigmodel.cn"),
        "qwen" => Some("dashscope.aliyuncs.com"),
        "moonshot" => Some("api.moonshot.cn"),
        "minimax" => Some("api.minimax.io"),
        "stepfun" => Some("api.stepfun.com"),
        _ => None,
    }
}

fn provider_base_url_is_allowed(provider_code: &str, base_url: &str) -> bool {
    let Some(expected_host) = expected_provider_host(provider_code) else {
        return false;
    };
    let Ok(url) = reqwest::Url::parse(base_url) else {
        return false;
    };
    url.scheme() == "https"
        && url.host_str() == Some(expected_host)
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

async fn responses_proxy(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
    Json(input): Json<serde_json::Value>,
) -> Response {
    let model = input
        .get("model")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    let chat_input = match protocol::responses_to_chat(&input) {
        Ok(value) => value,
        Err(error) => {
            let request_id = crate::utils::id_generator::generate_request_id();
            return with_nexus_headers(protocol_error_response(error), &request_id, None, None);
        }
    };
    let response = api_proxy(State(pool), Extension(user), Json(chat_input)).await;
    if !response.status().is_success() {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    let bytes = match to_bytes(body, 16 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::error!(error = %error, "Failed to read chat response for Responses adapter");
            return api_error_response(
                StatusCode::BAD_GATEWAY,
                "Gateway response could not be converted",
                "response_conversion_failed",
                "RESPONSE_CONVERSION_FAILED",
            );
        }
    };
    let chat: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(error = %error, "Failed to parse chat response for Responses adapter");
            return api_error_response(
                StatusCode::BAD_GATEWAY,
                "Gateway response could not be converted",
                "response_conversion_failed",
                "RESPONSE_CONVERSION_FAILED",
            );
        }
    };
    parts.headers.remove(axum::http::header::CONTENT_LENGTH);
    parts.headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    let body = Json(protocol::chat_to_responses(&chat, &model))
        .into_response()
        .into_body();
    Response::from_parts(parts, body)
}

async fn async_media_not_available(Json(input): Json<serde_json::Value>) -> Response {
    let request_id = crate::utils::id_generator::generate_request_id();
    let model = input
        .get("model")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    with_nexus_headers(
        api_error_response(
            if model.is_empty() {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::NOT_IMPLEMENTED
            },
            if model.is_empty() {
                "model is required"
            } else {
                "No enabled asynchronous provider endpoint is configured for this capability"
            },
            "capability_not_available",
            if model.is_empty() {
                "INVALID_REQUEST"
            } else {
                "ASYNC_MEDIA_NOT_AVAILABLE"
            },
        ),
        &request_id,
        None,
        None,
    )
}

async fn get_async_task(
    Path(uid): Path<String>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Response {
    let request_id = crate::utils::id_generator::generate_request_id();
    let task = sqlx::query(
        "SELECT uid, request_id, task_type, model_id, state, progress, result_url,
                error_code, retryable, created_at, started_at, completed_at, expires_at
         FROM async_inference_tasks
         WHERE uid = $1 AND workspace_id = $2 AND api_key_id = $3",
    )
    .bind(&uid)
    .bind(user.workspace_id)
    .bind(user.api_key_id)
    .fetch_optional(&pool)
    .await;
    let row = match task {
        Ok(Some(row)) => row,
        Ok(None) => {
            return with_nexus_headers(
                api_error_response(
                    StatusCode::NOT_FOUND,
                    "Task not found",
                    "not_found",
                    "TASK_NOT_FOUND",
                ),
                &request_id,
                None,
                None,
            )
        }
        Err(error) => {
            tracing::error!(error = %error, "Failed to read asynchronous task");
            return with_nexus_headers(
                api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Task state is unavailable",
                    "task_state_unavailable",
                    "TASK_STATE_UNAVAILABLE",
                ),
                &request_id,
                None,
                None,
            );
        }
    };
    with_nexus_headers(
        Json(json!({
            "id": row.get::<String, _>(0),
            "object": "inference.task",
            "request_id": row.get::<String, _>(1),
            "type": row.get::<String, _>(2),
            "model": row.get::<String, _>(3),
            "status": row.get::<String, _>(4),
            "progress": row.get::<i16, _>(5),
            "result_url": row.get::<Option<String>, _>(6),
            "error": row.get::<Option<String>, _>(7).map(|code| json!({"code": code, "retryable": row.get::<bool, _>(8)})),
            "created_at": row.get::<chrono::NaiveDateTime, _>(9),
            "started_at": row.get::<Option<chrono::NaiveDateTime>, _>(10),
            "completed_at": row.get::<Option<chrono::NaiveDateTime>, _>(11),
            "expires_at": row.get::<chrono::NaiveDateTime, _>(12)
        }))
        .into_response(),
        &request_id,
        None,
        None,
    )
}

async fn cancel_async_task(
    Path(uid): Path<String>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Response {
    let request_id = crate::utils::id_generator::generate_request_id();
    let result = sqlx::query(
        "UPDATE async_inference_tasks
         SET state = 'cancelled', completed_at = NOW(), updated_at = NOW()
         WHERE uid = $1 AND workspace_id = $2 AND api_key_id = $3 AND state = 'queued'
         RETURNING uid",
    )
    .bind(&uid)
    .bind(user.workspace_id)
    .bind(user.api_key_id)
    .fetch_optional(&pool)
    .await;
    match result {
        Ok(Some(_)) => with_nexus_headers(
            Json(json!({"id": uid, "object": "inference.task", "status": "cancelled"}))
                .into_response(),
            &request_id,
            None,
            None,
        ),
        Ok(None) => with_nexus_headers(
            api_error_response(
                StatusCode::CONFLICT,
                "Task does not exist or can no longer be cancelled",
                "task_not_cancellable",
                "TASK_NOT_CANCELLABLE",
            ),
            &request_id,
            None,
            None,
        ),
        Err(error) => {
            tracing::error!(error = %error, "Failed to cancel asynchronous task");
            with_nexus_headers(
                api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Task state is unavailable",
                    "task_state_unavailable",
                    "TASK_STATE_UNAVAILABLE",
                ),
                &request_id,
                None,
                None,
            )
        }
    }
}

// Gateway - API Proxy (multi-provider: OpenAI, Anthropic, Google)
async fn api_proxy(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
    Json(input): Json<serde_json::Value>,
) -> axum::response::Response {
    use sqlx::Row;

    let request_id = crate::utils::id_generator::generate_request_id();
    let user_id = user.user_id;
    let api_key_id = user.api_key_id;
    if api_key_id.is_none() {
        return with_nexus_headers(
            api_error_response(
                StatusCode::UNAUTHORIZED,
                "Use X-API-Key for model calls",
                "api_key_required",
                "API_KEY_REQUIRED",
            ),
            &request_id,
            None,
            None,
        );
    }
    let model = input
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("gpt-4.1-mini");
    let start_time = std::time::Instant::now();
    let rate_limit_state = match enforce_rate_limit(&user).await {
        Ok(state) => state,
        Err(response) => return with_nexus_headers(response, &request_id, None, None),
    };

    // 请求前只选择一次接入点；上游失败后不重新选路、不替换模型。
    let model_row = sqlx::query(
        "SELECT lm.id, lm.model_id, lm.display_name,
                CAST(lm.input_price_per_million AS VARCHAR),
                CAST(lm.output_price_per_million AS VARCHAR),
                selected.id, selected.model_id, selected.provider_id,
                p.name, p.base_url, p.api_key_env, rp.strategy, p.credential_mode,
                CAST(selected.upstream_input_price_per_million AS VARCHAR),
                CAST(selected.upstream_output_price_per_million AS VARCHAR),
                p.provider_id, lm.context_len, lm.max_tokens,
                CASE WHEN p.credential_mode = 'byok' THEN (
                    SELECT pc.id FROM provider_credentials pc
                    WHERE pc.workspace_id = $2 AND pc.provider_id = p.id AND pc.status = 0
                ) END
         FROM logical_models lm
         JOIN routing_policies rp ON rp.logical_model_id = lm.id
         JOIN LATERAL (
             SELECT pm.*
             FROM provider_models pm
             JOIN providers candidate_provider ON candidate_provider.id = pm.provider_id
             LEFT JOIN LATERAL (
                 SELECT COUNT(*)::BIGINT AS samples,
                        AVG(ra.latency_ms) FILTER (WHERE ra.status_code BETWEEN 200 AND 299) AS avg_latency,
                        AVG(CASE WHEN ra.status_code BETWEEN 200 AND 299 THEN 1.0 ELSE 0.0 END) AS stability
                 FROM request_attempts ra
                 WHERE ra.provider_model_id = pm.id
             ) stats ON TRUE
             WHERE pm.logical_model_id = lm.id
               AND pm.status = 0 AND candidate_provider.status = 0
               AND (
                   candidate_provider.credential_mode = 'platform_authorized'
                   OR EXISTS (
                       SELECT 1 FROM provider_credentials pc
                       WHERE pc.workspace_id = $2
                         AND pc.provider_id = candidate_provider.id
                         AND pc.status = 0
                   )
               )
               AND (rp.strategy <> 'fixed' OR pm.id = rp.fixed_provider_model_id)
             ORDER BY
               CASE WHEN rp.strategy = 'lowest_price' THEN
                   pm.upstream_input_price_per_million * rp.input_weight
                   + pm.upstream_output_price_per_million * rp.output_weight END ASC NULLS LAST,
               CASE WHEN rp.strategy = 'lowest_latency' AND stats.samples >= rp.minimum_samples
                   THEN stats.avg_latency END ASC NULLS LAST,
               CASE WHEN rp.strategy = 'highest_stability' AND stats.samples >= rp.minimum_samples
                   THEN stats.stability END DESC NULLS LAST,
               pm.route_priority, pm.id
             LIMIT 1
         ) selected ON TRUE
         JOIN providers p ON p.id = selected.provider_id
         WHERE lm.model_id = $1 AND lm.status = 0"
    )
    .bind(model)
    .bind(user.workspace_id)
    .fetch_optional(&pool)
    .await;

    let (
        logical_model_id,
        model_db_id,
        upstream_model,
        input_rate,
        output_rate,
        provider_name,
        provider_id,
        base_url,
        api_key_env,
        routing_strategy,
        credential_mode,
        upstream_input_rate,
        upstream_output_rate,
        provider_code,
        provider_credential_id,
        model_context_len,
        model_max_output_tokens,
    ) = match model_row {
        Ok(Some(ref row)) => {
            let logical_id: i64 = row.get(0);
            let mid: i64 = row.get(5);
            let ir: f64 = row
                .get::<String, _>(3)
                .parse()
                .expect("database input price must parse");
            let or: f64 = row
                .get::<String, _>(4)
                .parse()
                .expect("database output price must parse");
            let upstream_ir: f64 = row
                .get::<String, _>(13)
                .parse()
                .expect("database upstream input price must parse");
            let upstream_or: f64 = row
                .get::<String, _>(14)
                .parse()
                .expect("database upstream output price must parse");
            (
                logical_id,
                Some(mid),
                row.get::<String, _>(6),
                ir,
                or,
                row.get::<String, _>(8),
                row.get::<i64, _>(7),
                row.get::<String, _>(9),
                row.get::<String, _>(10),
                row.get::<String, _>(11),
                row.get::<String, _>(12),
                upstream_ir,
                upstream_or,
                row.get::<String, _>(15),
                row.get::<Option<i64>, _>(18),
                row.get::<i32, _>(16),
                row.get::<i32, _>(17),
            )
        }
        _ => {
            return with_nexus_headers(
                with_rate_limit_headers(
                    api_error_response(
                        StatusCode::BAD_REQUEST,
                        format!("Unsupported model: {}", model),
                        "unsupported_model",
                        "UNSUPPORTED_MODEL",
                    ),
                    rate_limit_state.as_ref(),
                    false,
                ),
                &request_id,
                None,
                None,
            )
        }
    };

    let credential_source = match credential_mode.as_str() {
        "byok" => "byok",
        "platform_authorized" => "platform",
        _ => {
            return with_nexus_headers(
                api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Provider credential mode is invalid",
                    "provider_configuration_invalid",
                    "PROVIDER_CONFIGURATION_INVALID",
                ),
                &request_id,
                Some(&provider_name),
                Some(&routing_strategy),
            )
        }
    };
    let (
        billable_input_rate,
        billable_output_rate,
        logged_upstream_input_rate,
        logged_upstream_output_rate,
    ) = if credential_mode == "byok" {
        (0.0, 0.0, 0.0, 0.0)
    } else {
        (
            input_rate,
            output_rate,
            upstream_input_rate,
            upstream_output_rate,
        )
    };

    let mut upstream_input = input.clone();
    upstream_input["model"] = json!(upstream_model);
    let gateway_context = GatewayLogContext {
        request_id: request_id.clone(),
        workspace_id: user.workspace_id,
        project_id: user.project_id,
        logical_model_id,
        provider_model_id: model_db_id.expect("Selected route must have a provider model"),
        provider_credential_id,
        routing_strategy: routing_strategy.clone(),
        credential_source: credential_source.to_string(),
        upstream_input_rate: logged_upstream_input_rate,
        upstream_output_rate: logged_upstream_output_rate,
    };

    if !is_model_allowed_for_key(&user.models_allowed, model) {
        let latency = start_time.elapsed().as_millis() as i32;
        let error_msg = format!("Model {} is not allowed for this API key", model);
        log_api_call(
            &pool,
            &gateway_context,
            api_key_id,
            user_id,
            model,
            model_db_id,
            provider_id,
            0,
            0,
            0.0,
            latency,
            403,
            &error_msg,
        )
        .await;
        return with_rate_limit_headers(
            (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": {
                        "message": error_msg,
                        "type": "model_not_allowed",
                        "code": "MODEL_NOT_ALLOWED"
                    }
                })),
            )
                .into_response(),
            rate_limit_state.as_ref(),
            false,
        );
    }

    match provider_circuit_open(&pool, provider_id).await {
        Ok(Some(reason)) => {
            let latency = start_time.elapsed().as_millis() as i32;
            log_api_call(
                &pool,
                &gateway_context,
                api_key_id,
                user_id,
                model,
                model_db_id,
                provider_id,
                0,
                0,
                0.0,
                latency,
                503,
                &reason,
            )
            .await;
            return with_rate_limit_headers(
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({
                        "error": {
                            "message": reason,
                            "type": "provider_circuit_open",
                            "code": "PROVIDER_CIRCUIT_OPEN"
                        }
                    })),
                )
                    .into_response(),
                rate_limit_state.as_ref(),
                false,
            );
        }
        Ok(None) => {}
        Err(message) => {
            return with_rate_limit_headers(
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({
                        "error": {
                            "message": message,
                            "type": "provider_health_unavailable",
                            "code": "PROVIDER_HEALTH_UNAVAILABLE"
                        }
                    })),
                )
                    .into_response(),
                rate_limit_state.as_ref(),
                false,
            );
        }
    }

    if !provider_base_url_is_allowed(&provider_code, &base_url) {
        return with_nexus_headers(
            api_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Provider endpoint is not allowed",
                "provider_endpoint_rejected",
                "PROVIDER_ENDPOINT_REJECTED",
            ),
            &request_id,
            Some(&provider_name),
            Some(&routing_strategy),
        );
    }

    let provider_key = if credential_mode == "byok" {
        let Some(credential_id) = provider_credential_id else {
            return with_nexus_headers(
                api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "No active BYOK credential is configured for this provider",
                    "byok_credential_unavailable",
                    "BYOK_CREDENTIAL_UNAVAILABLE",
                ),
                &request_id,
                Some(&provider_name),
                Some(&routing_strategy),
            );
        };
        let credential = sqlx::query(
            "SELECT ciphertext, nonce, encryption_key_id
             FROM provider_credentials
             WHERE id = $1 AND workspace_id = $2 AND provider_id = $3 AND status = 0",
        )
        .bind(credential_id)
        .bind(user.workspace_id)
        .bind(provider_id)
        .fetch_optional(&pool)
        .await;
        let credential = match credential {
            Ok(Some(row)) => row,
            Ok(None) => {
                return with_nexus_headers(
                    api_error_response(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "No active BYOK credential is configured for this provider",
                        "byok_credential_unavailable",
                        "BYOK_CREDENTIAL_UNAVAILABLE",
                    ),
                    &request_id,
                    Some(&provider_name),
                    Some(&routing_strategy),
                )
            }
            Err(error) => {
                tracing::error!(error = %error, "Failed to load BYOK credential");
                return with_nexus_headers(
                    api_error_response(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "BYOK credential is unavailable",
                        "byok_credential_unavailable",
                        "BYOK_CREDENTIAL_UNAVAILABLE",
                    ),
                    &request_id,
                    Some(&provider_name),
                    Some(&routing_strategy),
                );
            }
        };
        let ciphertext: Vec<u8> = credential.get(0);
        let nonce: Vec<u8> = credential.get(1);
        let encryption_key_id: String = credential.get(2);
        let cipher = match byok::ByokCipher::from_env() {
            Ok(cipher) if cipher.key_id() == encryption_key_id => cipher,
            Ok(_) | Err(_) => {
                tracing::error!(
                    credential_id,
                    "BYOK encryption key is unavailable or does not match"
                );
                return with_nexus_headers(
                    api_error_response(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "BYOK credential cannot be decrypted",
                        "byok_decryption_unavailable",
                        "BYOK_DECRYPTION_UNAVAILABLE",
                    ),
                    &request_id,
                    Some(&provider_name),
                    Some(&routing_strategy),
                );
            }
        };
        let plaintext = match cipher.decrypt(&ciphertext, &nonce) {
            Ok(plaintext) => plaintext,
            Err(error) => {
                tracing::error!(error = %error, credential_id, "Failed to decrypt BYOK credential");
                return with_nexus_headers(
                    api_error_response(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "BYOK credential cannot be decrypted",
                        "byok_decryption_unavailable",
                        "BYOK_DECRYPTION_UNAVAILABLE",
                    ),
                    &request_id,
                    Some(&provider_name),
                    Some(&routing_strategy),
                );
            }
        };
        if let Err(error) = sqlx::query(
            "UPDATE provider_credentials SET last_used_at = NOW(), updated_at = NOW()
             WHERE id = $1 AND workspace_id = $2 AND status = 0",
        )
        .bind(credential_id)
        .bind(user.workspace_id)
        .execute(&pool)
        .await
        {
            tracing::error!(error = %error, credential_id, "Failed to update BYOK credential usage");
            return with_nexus_headers(
                api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "BYOK credential state could not be updated",
                    "byok_credential_unavailable",
                    "BYOK_CREDENTIAL_UNAVAILABLE",
                ),
                &request_id,
                Some(&provider_name),
                Some(&routing_strategy),
            );
        }
        plaintext
    } else {
        crate::config::provider_api_key(&api_key_env)
    };

    let requested_max_output_tokens = input
        .get("max_completion_tokens")
        .or_else(|| input.get("max_tokens"))
        .and_then(|value| value.as_i64())
        .unwrap_or(model_max_output_tokens as i64);
    if requested_max_output_tokens <= 0
        || requested_max_output_tokens > model_max_output_tokens as i64
    {
        return with_nexus_headers(
            api_error_response(
                StatusCode::BAD_REQUEST,
                format!(
                    "max_tokens must be between 1 and {}",
                    model_max_output_tokens
                ),
                "invalid_parameter",
                "INVALID_MAX_TOKENS",
            ),
            &request_id,
            Some(&provider_name),
            Some(&routing_strategy),
        );
    }

    // 输入按模型完整上下文、输出按本次声明上限冻结。这是无需猜测分词结果的严格费用上限。
    let maximum_user_cost = calculate_token_cost(
        model_context_len,
        requested_max_output_tokens as i32,
        billable_input_rate,
        billable_output_rate,
    );
    let budget_context = gateway_context.budget_context(
        api_key_id.expect("API key checked before routing"),
        user_id,
        model,
        provider_id,
    );
    if let Err(error) =
        budget::reserve_request_budget(&pool, &budget_context, maximum_user_cost).await
    {
        use budget::ReserveFailureKind;
        let (status, error_type, code) = match error.kind {
            ReserveFailureKind::DailyBudget => (
                StatusCode::TOO_MANY_REQUESTS,
                "key_daily_limit_exceeded",
                "KEY_DAILY_LIMIT_EXCEEDED",
            ),
            ReserveFailureKind::MonthlyBudget => (
                StatusCode::TOO_MANY_REQUESTS,
                "key_monthly_limit_exceeded",
                "KEY_MONTHLY_LIMIT_EXCEEDED",
            ),
            ReserveFailureKind::TotalBudget => (
                StatusCode::TOO_MANY_REQUESTS,
                "key_total_limit_exceeded",
                "KEY_TOTAL_LIMIT_EXCEEDED",
            ),
            ReserveFailureKind::InsufficientBalance => (
                StatusCode::PAYMENT_REQUIRED,
                "insufficient_balance",
                "INSUFFICIENT_BALANCE",
            ),
            ReserveFailureKind::InvalidAmount | ReserveFailureKind::Database => (
                StatusCode::SERVICE_UNAVAILABLE,
                "budget_unavailable",
                "BUDGET_UNAVAILABLE",
            ),
        };
        log_api_call(
            &pool,
            &gateway_context,
            api_key_id,
            user_id,
            model,
            model_db_id,
            provider_id,
            0,
            0,
            0.0,
            start_time.elapsed().as_millis() as i32,
            status.as_u16() as i32,
            &error.message,
        )
        .await;
        return with_nexus_headers(
            with_rate_limit_headers(
                api_error_response(status, error.message, error_type, code),
                rate_limit_state.as_ref(),
                false,
            ),
            &request_id,
            Some(&provider_name),
            Some(&routing_strategy),
        );
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .expect("HTTP client configuration must be valid");

    let provider_lower = provider_name.to_lowercase();

    // Route to provider
    if provider_lower.contains("openai") {
        let response = call_openai(
            &pool,
            &gateway_context,
            &client,
            user_id,
            api_key_id,
            model,
            &upstream_input,
            billable_input_rate,
            billable_output_rate,
            start_time,
            model_db_id,
            provider_id,
            &provider_key,
        )
        .await;
        with_nexus_headers(
            with_rate_limit_headers(response, rate_limit_state.as_ref(), false),
            &request_id,
            Some(&provider_name),
            Some(&routing_strategy),
        )
    } else if provider_lower.contains("anthropic") || provider_lower.contains("claude") {
        let response = call_anthropic(
            &pool,
            &gateway_context,
            &client,
            user_id,
            api_key_id,
            model,
            &upstream_input,
            billable_input_rate,
            billable_output_rate,
            start_time,
            model_db_id,
            provider_id,
            &provider_key,
        )
        .await;
        with_nexus_headers(
            with_rate_limit_headers(response, rate_limit_state.as_ref(), false),
            &request_id,
            Some(&provider_name),
            Some(&routing_strategy),
        )
    } else if provider_lower.contains("google") || provider_lower.contains("gemini") {
        let response = call_google(
            &pool,
            &gateway_context,
            &client,
            user_id,
            api_key_id,
            model,
            &upstream_input,
            billable_input_rate,
            billable_output_rate,
            start_time,
            model_db_id,
            provider_id,
            &provider_key,
        )
        .await;
        with_nexus_headers(
            with_rate_limit_headers(response, rate_limit_state.as_ref(), false),
            &request_id,
            Some(&provider_name),
            Some(&routing_strategy),
        )
    } else if is_openai_compatible_provider(&provider_lower) {
        let response = call_openai_compatible(
            &pool,
            &gateway_context,
            &client,
            user_id,
            api_key_id,
            model,
            &upstream_input,
            billable_input_rate,
            billable_output_rate,
            start_time,
            model_db_id,
            provider_id,
            &provider_name,
            &base_url,
            &provider_key,
        )
        .await;
        with_nexus_headers(
            with_rate_limit_headers(response, rate_limit_state.as_ref(), false),
            &request_id,
            Some(&provider_name),
            Some(&routing_strategy),
        )
    } else {
        let latency = start_time.elapsed().as_millis() as i32;
        let error_msg = format!("Unsupported provider: {}", provider_name);
        log_api_call(
            &pool,
            &gateway_context,
            api_key_id,
            user_id,
            model,
            model_db_id,
            provider_id,
            0,
            0,
            0.0,
            latency,
            400,
            &error_msg,
        )
        .await;
        with_nexus_headers(
            with_rate_limit_headers(
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": {
                            "message": error_msg,
                            "type": "unsupported_provider",
                            "code": "UNSUPPORTED_PROVIDER"
                        }
                    })),
                )
                    .into_response(),
                rate_limit_state.as_ref(),
                false,
            ),
            &request_id,
            Some(&provider_name),
            Some(&routing_strategy),
        )
    }
}

fn is_openai_compatible_provider(provider_lower: &str) -> bool {
    [
        "volcengine",
        "deepseek",
        "glm",
        "zhipu",
        "qwen",
        "dashscope",
        "moonshot",
        "kimi",
        "minimax",
        "stepfun",
    ]
    .iter()
    .any(|needle| provider_lower.contains(needle))
}

#[derive(Clone, Debug)]
struct GatewayLogContext {
    request_id: String,
    workspace_id: i64,
    project_id: i64,
    logical_model_id: i64,
    provider_model_id: i64,
    provider_credential_id: Option<i64>,
    routing_strategy: String,
    credential_source: String,
    upstream_input_rate: f64,
    upstream_output_rate: f64,
}

impl GatewayLogContext {
    fn budget_context(
        &self,
        api_key_id: i64,
        user_id: i64,
        model: &str,
        provider_id: i64,
    ) -> budget::RequestBudgetContext {
        budget::RequestBudgetContext {
            request_id: self.request_id.clone(),
            api_key_id,
            user_id,
            workspace_id: self.workspace_id,
            project_id: self.project_id,
            logical_model_id: self.logical_model_id,
            provider_model_id: self.provider_model_id,
            provider_id,
            provider_credential_id: self.provider_credential_id,
            model: model.to_string(),
            routing_strategy: self.routing_strategy.clone(),
            credential_source: self.credential_source.clone(),
        }
    }
}

async fn log_api_call(
    pool: &PgPool,
    context: &GatewayLogContext,
    api_key_id: Option<i64>,
    user_id: i64,
    model: &str,
    _model_db_id: Option<i64>,
    provider_id: i64,
    input_tokens: i32,
    output_tokens: i32,
    cost: f64,
    latency_ms: i32,
    status_code: i32,
    error_msg: &str,
) -> bool {
    let provider_cost = calculate_token_cost(
        input_tokens,
        output_tokens,
        context.upstream_input_rate,
        context.upstream_output_rate,
    );
    let succeeded = (200..300).contains(&status_code);
    let retryable = status_code == 429 || status_code >= 500;
    let error_code = (!succeeded).then_some(if status_code == 429 {
        "RATE_LIMITED"
    } else if status_code >= 500 {
        "UPSTREAM_FAILED"
    } else {
        "REQUEST_REJECTED"
    });
    let Some(api_key_id) = api_key_id else {
        tracing::error!(request_id = %context.request_id, "Cannot complete gateway request without API key");
        return false;
    };
    let budget_context = context.budget_context(api_key_id, user_id, model, provider_id);
    let completed = budget::CompletedRequest {
        input_tokens,
        output_tokens,
        user_cost_usd: if succeeded { cost } else { 0.0 },
        provider_cost_usd: provider_cost,
        latency_ms,
        status_code,
        error_message: (!error_msg.is_empty()).then(|| error_msg.to_string()),
        error_code: error_code.map(str::to_string),
        retryable,
    };
    match budget::complete_request(pool, &budget_context, &completed).await {
        Ok(()) => true,
        Err(error) => {
            tracing::error!(
                request_id = %context.request_id,
                error = %error,
                "Failed to atomically complete API request"
            );
            false
        }
    }
}

fn calculate_token_cost(
    input_tokens: i32,
    output_tokens: i32,
    input_rate_per_million: f64,
    output_rate_per_million: f64,
) -> f64 {
    let raw = ((input_tokens as f64) * input_rate_per_million
        + (output_tokens as f64) * output_rate_per_million)
        / 1_000_000.0;
    (raw * 100_000_000.0).round() / 100_000_000.0
}

fn api_error_response(
    status: StatusCode,
    message: impl Into<String>,
    error_type: &str,
    code: &str,
) -> Response {
    (
        status,
        Json(json!({
            "error": {
                "message": message.into(),
                "type": error_type,
                "code": code
            }
        })),
    )
        .into_response()
}

fn protocol_error_response(error: protocol::ProtocolError) -> Response {
    api_error_response(
        StatusCode::BAD_REQUEST,
        error.message,
        "invalid_request_error",
        error.code,
    )
}

fn upstream_status_error_response(status: reqwest::StatusCode) -> Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(json!({
            "error": {
                "message": "Upstream API error",
                "type": "upstream_api_error",
                "code": "UPSTREAM_API_ERROR",
                "upstream_status": status.as_u16()
            }
        })),
    )
        .into_response()
}

fn upstream_request_failed_response() -> Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(json!({
            "error": {
                "message": "Upstream API request failed",
                "type": "upstream_request_failed",
                "code": "UPSTREAM_REQUEST_FAILED"
            }
        })),
    )
        .into_response()
}

async fn malformed_upstream_response(
    pool: &PgPool,
    context: &GatewayLogContext,
    api_key_id: Option<i64>,
    user_id: i64,
    model: &str,
    model_db_id: Option<i64>,
    provider_id: i64,
    latency: i32,
    internal_reason: &str,
) -> Response {
    log_api_call(
        pool,
        context,
        api_key_id,
        user_id,
        model,
        model_db_id,
        provider_id,
        0,
        0,
        0.0,
        latency,
        502,
        internal_reason,
    )
    .await;
    api_error_response(
        StatusCode::BAD_GATEWAY,
        "Upstream returned an invalid response",
        "upstream_response_invalid",
        "UPSTREAM_RESPONSE_INVALID",
    )
}

// ── OpenAI ──────────────────────────────────────────────
async fn call_openai(
    pool: &PgPool,
    context: &GatewayLogContext,
    client: &reqwest::Client,
    user_id: i64,
    api_key_id: Option<i64>,
    model: &str,
    input: &serde_json::Value,
    input_rate: f64,
    output_rate: f64,
    start_time: std::time::Instant,
    model_db_id: Option<i64>,
    provider_id: i64,
    provider_key: &str,
) -> Response {
    call_openai_compatible(
        pool,
        context,
        client,
        user_id,
        api_key_id,
        model,
        input,
        input_rate,
        output_rate,
        start_time,
        model_db_id,
        provider_id,
        "OpenAI",
        "https://api.openai.com/v1",
        provider_key,
    )
    .await
}

// ── OpenAI-compatible providers ───────────────────────
async fn call_openai_compatible(
    pool: &PgPool,
    context: &GatewayLogContext,
    client: &reqwest::Client,
    user_id: i64,
    api_key_id: Option<i64>,
    model: &str,
    input: &serde_json::Value,
    input_rate: f64,
    output_rate: f64,
    start_time: std::time::Instant,
    model_db_id: Option<i64>,
    provider_id: i64,
    provider_name: &str,
    base_url: &str,
    provider_key: &str,
) -> Response {
    if provider_key.is_empty() {
        let latency = start_time.elapsed().as_millis() as i32;
        let error_msg = format!("{} provider key is not configured", provider_name);
        log_api_call(
            pool,
            context,
            api_key_id,
            user_id,
            model,
            model_db_id,
            provider_id,
            0,
            0,
            0.0,
            latency,
            503,
            &error_msg,
        )
        .await;
        return api_error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            error_msg,
            "provider_not_configured",
            "PROVIDER_NOT_CONFIGURED",
        );
    }

    if input.get("stream").and_then(serde_json::Value::as_bool) == Some(true) {
        if !provider_name.eq_ignore_ascii_case("openai") {
            let latency = start_time.elapsed().as_millis() as i32;
            let error_message = format!(
                "Streaming has not been verified for OpenAI-compatible provider {provider_name}"
            );
            log_api_call(
                pool,
                context,
                api_key_id,
                user_id,
                model,
                model_db_id,
                provider_id,
                0,
                0,
                0.0,
                latency,
                501,
                &error_message,
            )
            .await;
            return api_error_response(
                StatusCode::NOT_IMPLEMENTED,
                error_message,
                "capability_not_available",
                "STREAMING_NOT_AVAILABLE_FOR_PROVIDER",
            );
        }
        return call_openai_stream(
            pool,
            context,
            client,
            user_id,
            api_key_id,
            model,
            input,
            input_rate,
            output_rate,
            start_time,
            model_db_id,
            provider_id,
            base_url,
            provider_key,
        )
        .await;
    }

    let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let response = client
        .post(endpoint)
        .header("Authorization", format!("Bearer {}", provider_key))
        .header("Content-Type", "application/json")
        .json(input)
        .send()
        .await;

    let latency = start_time.elapsed().as_millis() as i32;

    match response {
        Ok(resp) => {
            let status = resp.status();
            if !status.is_success() {
                let internal_error = format!("Upstream returned HTTP {}", status.as_u16());
                log_api_call(
                    pool,
                    context,
                    api_key_id,
                    user_id,
                    model,
                    model_db_id,
                    provider_id,
                    0,
                    0,
                    0.0,
                    latency,
                    status.as_u16() as i32,
                    &internal_error,
                )
                .await;
                return upstream_status_error_response(status);
            }

            let json: serde_json::Value = match resp.json().await {
                Ok(json) => json,
                Err(error) => {
                    tracing::error!(error = %error, "OpenAI-compatible upstream returned invalid JSON");
                    return malformed_upstream_response(
                        pool,
                        context,
                        api_key_id,
                        user_id,
                        model,
                        model_db_id,
                        provider_id,
                        latency,
                        "OpenAI-compatible upstream returned invalid JSON",
                    )
                    .await;
                }
            };
            let usage = json.get("usage");
            let Some(prompt_tokens) = usage
                .and_then(|usage| usage.get("prompt_tokens"))
                .and_then(serde_json::Value::as_i64)
                .filter(|tokens| *tokens >= 0)
                .and_then(|tokens| i32::try_from(tokens).ok())
            else {
                return malformed_upstream_response(
                    pool,
                    context,
                    api_key_id,
                    user_id,
                    model,
                    model_db_id,
                    provider_id,
                    latency,
                    "OpenAI-compatible upstream response is missing valid prompt usage",
                )
                .await;
            };
            let Some(completion_tokens) = usage
                .and_then(|usage| usage.get("completion_tokens"))
                .and_then(serde_json::Value::as_i64)
                .filter(|tokens| *tokens >= 0)
                .and_then(|tokens| i32::try_from(tokens).ok())
            else {
                return malformed_upstream_response(
                    pool,
                    context,
                    api_key_id,
                    user_id,
                    model,
                    model_db_id,
                    provider_id,
                    latency,
                    "OpenAI-compatible upstream response is missing valid completion usage",
                )
                .await;
            };
            let cost =
                calculate_token_cost(prompt_tokens, completion_tokens, input_rate, output_rate);

            if !log_api_call(
                pool,
                context,
                api_key_id,
                user_id,
                model,
                model_db_id,
                provider_id,
                prompt_tokens,
                completion_tokens,
                cost,
                latency,
                200,
                "",
            )
            .await
            {
                return api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Request completed upstream but billing could not be settled",
                    "billing_settlement_failed",
                    "BILLING_SETTLEMENT_FAILED",
                );
            }

            Json(json).into_response()
        }
        Err(e) => {
            let detail = e.to_string();
            log_api_call(
                pool,
                context,
                api_key_id,
                user_id,
                model,
                model_db_id,
                provider_id,
                0,
                0,
                0.0,
                latency,
                502,
                &detail,
            )
            .await;
            upstream_request_failed_response()
        }
    }
}

async fn call_openai_stream(
    pool: &PgPool,
    context: &GatewayLogContext,
    client: &reqwest::Client,
    user_id: i64,
    api_key_id: Option<i64>,
    model: &str,
    input: &serde_json::Value,
    input_rate: f64,
    output_rate: f64,
    start_time: std::time::Instant,
    model_db_id: Option<i64>,
    provider_id: i64,
    base_url: &str,
    provider_key: &str,
) -> Response {
    let mut upstream_input = input.clone();
    upstream_input["stream"] = json!(true);
    upstream_input["stream_options"] = json!({"include_usage": true});
    let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let response = client
        .post(endpoint)
        .header("Authorization", format!("Bearer {provider_key}"))
        .header("Content-Type", "application/json")
        .json(&upstream_input)
        .send()
        .await;
    let upstream = match response {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            let status = response.status();
            let latency = start_time.elapsed().as_millis() as i32;
            let message = format!("Upstream returned HTTP {}", status.as_u16());
            log_api_call(
                pool,
                context,
                api_key_id,
                user_id,
                model,
                model_db_id,
                provider_id,
                0,
                0,
                0.0,
                latency,
                status.as_u16() as i32,
                &message,
            )
            .await;
            return upstream_status_error_response(status);
        }
        Err(error) => {
            let latency = start_time.elapsed().as_millis() as i32;
            log_api_call(
                pool,
                context,
                api_key_id,
                user_id,
                model,
                model_db_id,
                provider_id,
                0,
                0,
                0.0,
                latency,
                502,
                &error.to_string(),
            )
            .await;
            return upstream_request_failed_response();
        }
    };

    let pool = pool.clone();
    let context = context.clone();
    let model = model.to_string();
    let mut upstream_stream = upstream.bytes_stream();
    let (sender, receiver) =
        tokio::sync::mpsc::channel::<Result<bytes::Bytes, std::convert::Infallible>>(16);
    tokio::spawn(async move {
        let mut buffer = Vec::new();
        let mut usage: Option<(i32, i32)> = None;
        let mut done_seen = false;
        let mut stream_error: Option<String> = None;
        while let Some(chunk) = upstream_stream.next().await {
            match chunk {
                Ok(bytes) => {
                    buffer.extend_from_slice(&bytes);
                    while let Some(event) = take_sse_event(&mut buffer) {
                        match inspect_openai_sse_event(&event) {
                            Ok(SseEvent::Done) => done_seen = true,
                            Ok(SseEvent::Data(event_usage)) => {
                                if let Some(event_usage) = event_usage {
                                    usage = Some(event_usage);
                                }
                                let _ = sender.send(Ok(bytes::Bytes::from(event))).await;
                            }
                            Err(message) => {
                                stream_error = Some(message);
                                break;
                            }
                        }
                    }
                    if stream_error.is_some() {
                        break;
                    }
                }
                Err(error) => {
                    stream_error = Some(format!("Upstream stream failed: {error}"));
                    break;
                }
            }
        }
        if stream_error.is_none() && !buffer.is_empty() {
            stream_error = Some("Upstream ended with an incomplete SSE event".to_string());
        }
        if stream_error.is_none() && !done_seen {
            stream_error = Some("Upstream stream ended without [DONE]".to_string());
        }
        if stream_error.is_none() && usage.is_none() {
            stream_error = Some("Upstream stream did not return token usage".to_string());
        }

        if let Some(message) = stream_error {
            let latency = start_time.elapsed().as_millis() as i32;
            log_api_call(
                &pool,
                &context,
                api_key_id,
                user_id,
                &model,
                model_db_id,
                provider_id,
                0,
                0,
                0.0,
                latency,
                502,
                &message,
            )
            .await;
            let event = sse_gateway_error("UPSTREAM_STREAM_INVALID", &message);
            let _ = sender.send(Ok(bytes::Bytes::from(event))).await;
            let _ = sender
                .send(Ok(bytes::Bytes::from_static(b"data: [DONE]\n\n")))
                .await;
            return;
        }

        let (prompt_tokens, completion_tokens) = usage.expect("usage checked above");
        let cost = calculate_token_cost(prompt_tokens, completion_tokens, input_rate, output_rate);
        let latency = start_time.elapsed().as_millis() as i32;
        let settled = log_api_call(
            &pool,
            &context,
            api_key_id,
            user_id,
            &model,
            model_db_id,
            provider_id,
            prompt_tokens,
            completion_tokens,
            cost,
            latency,
            200,
            "",
        )
        .await;
        if !settled {
            let event = sse_gateway_error(
                "BILLING_SETTLEMENT_FAILED",
                "Stream completed upstream but billing could not be settled",
            );
            let _ = sender.send(Ok(bytes::Bytes::from(event))).await;
        }
        let _ = sender
            .send(Ok(bytes::Bytes::from_static(b"data: [DONE]\n\n")))
            .await;
    });

    let body = Body::from_stream(tokio_stream::wrappers::ReceiverStream::new(receiver));
    let mut response = Response::new(body);
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream; charset=utf-8"),
    );
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-transform"),
    );
    response.headers_mut().insert(
        axum::http::header::CONNECTION,
        HeaderValue::from_static("keep-alive"),
    );
    response
}

#[derive(Debug)]
enum SseEvent {
    Done,
    Data(Option<(i32, i32)>),
}

fn take_sse_event(buffer: &mut Vec<u8>) -> Option<Vec<u8>> {
    let lf = buffer.windows(2).position(|window| window == b"\n\n");
    let crlf = buffer.windows(4).position(|window| window == b"\r\n\r\n");
    let (position, separator_len) = match (lf, crlf) {
        (Some(left), Some(right)) if left <= right => (left, 2),
        (Some(_), Some(right)) => (right, 4),
        (Some(left), None) => (left, 2),
        (None, Some(right)) => (right, 4),
        (None, None) => return None,
    };
    Some(buffer.drain(..position + separator_len).collect())
}

fn inspect_openai_sse_event(event: &[u8]) -> Result<SseEvent, String> {
    let text = std::str::from_utf8(event).map_err(|_| "Upstream SSE is not UTF-8".to_string())?;
    let data = text
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() {
        return Ok(SseEvent::Data(None));
    }
    if data.trim() == "[DONE]" {
        return Ok(SseEvent::Done);
    }
    let value: serde_json::Value = serde_json::from_str(&data)
        .map_err(|_| "Upstream SSE data is not valid JSON".to_string())?;
    if value.get("error").is_some() {
        return Err("Upstream returned an error event".to_string());
    }
    let usage = value.get("usage").and_then(|usage| {
        let prompt = usage.get("prompt_tokens")?.as_i64()?;
        let completion = usage.get("completion_tokens")?.as_i64()?;
        Some((prompt as i32, completion as i32))
    });
    Ok(SseEvent::Data(usage))
}

fn sse_gateway_error(code: &str, message: &str) -> String {
    format!(
        "data: {}\n\n",
        json!({
            "error": {
                "code": code,
                "message": message,
                "type": "gateway_stream_error",
                "retryable": false
            }
        })
    )
}

// ── Anthropic (Claude) ──────────────────────────────────
async fn call_anthropic(
    pool: &PgPool,
    context: &GatewayLogContext,
    client: &reqwest::Client,
    user_id: i64,
    api_key_id: Option<i64>,
    model: &str,
    input: &serde_json::Value,
    input_rate: f64,
    output_rate: f64,
    start_time: std::time::Instant,
    model_db_id: Option<i64>,
    provider_id: i64,
    api_key: &str,
) -> Response {
    let upstream_model = input
        .get("model")
        .and_then(|value| value.as_str())
        .unwrap_or(model);
    if api_key.is_empty() {
        let latency = start_time.elapsed().as_millis() as i32;
        let error_msg = "Anthropic provider key is not configured";
        log_api_call(
            pool,
            context,
            api_key_id,
            user_id,
            model,
            model_db_id,
            provider_id,
            0,
            0,
            0.0,
            latency,
            503,
            error_msg,
        )
        .await;
        return api_error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            error_msg,
            "provider_not_configured",
            "PROVIDER_NOT_CONFIGURED",
        );
    }
    if input.get("stream").and_then(serde_json::Value::as_bool) == Some(true) {
        let latency = start_time.elapsed().as_millis() as i32;
        let message = "Anthropic SSE conversion is not implemented";
        log_api_call(
            pool,
            context,
            api_key_id,
            user_id,
            model,
            model_db_id,
            provider_id,
            0,
            0,
            0.0,
            latency,
            501,
            message,
        )
        .await;
        return api_error_response(
            StatusCode::NOT_IMPLEMENTED,
            message,
            "capability_not_available",
            "STREAMING_NOT_AVAILABLE_FOR_PROVIDER",
        );
    }

    let body = match protocol::openai_to_anthropic(input, upstream_model) {
        Ok(body) => body,
        Err(error) => {
            let latency = start_time.elapsed().as_millis() as i32;
            log_api_call(
                pool,
                context,
                api_key_id,
                user_id,
                model,
                model_db_id,
                provider_id,
                0,
                0,
                0.0,
                latency,
                400,
                &error.message,
            )
            .await;
            return protocol_error_response(error);
        }
    };

    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await;

    let latency = start_time.elapsed().as_millis() as i32;

    match response {
        Ok(resp) => {
            let status = resp.status();
            if !status.is_success() {
                let internal_error = format!("Upstream returned HTTP {}", status.as_u16());
                log_api_call(
                    pool,
                    context,
                    api_key_id,
                    user_id,
                    model,
                    model_db_id,
                    provider_id,
                    0,
                    0,
                    0.0,
                    latency,
                    status.as_u16() as i32,
                    &internal_error,
                )
                .await;
                return upstream_status_error_response(status);
            }

            let json: serde_json::Value = match resp.json().await {
                Ok(json) => json,
                Err(error) => {
                    tracing::error!(error = %error, "Anthropic upstream returned invalid JSON");
                    return malformed_upstream_response(
                        pool,
                        context,
                        api_key_id,
                        user_id,
                        model,
                        model_db_id,
                        provider_id,
                        latency,
                        "Anthropic upstream returned invalid JSON",
                    )
                    .await;
                }
            };

            let usage = json.get("usage");
            let Some(input_tokens) = usage
                .and_then(|usage| usage.get("input_tokens"))
                .and_then(serde_json::Value::as_i64)
                .filter(|tokens| *tokens >= 0)
                .and_then(|tokens| i32::try_from(tokens).ok())
            else {
                return malformed_upstream_response(
                    pool,
                    context,
                    api_key_id,
                    user_id,
                    model,
                    model_db_id,
                    provider_id,
                    latency,
                    "Anthropic upstream response is missing valid input usage",
                )
                .await;
            };
            let Some(output_tokens) = usage
                .and_then(|usage| usage.get("output_tokens"))
                .and_then(serde_json::Value::as_i64)
                .filter(|tokens| *tokens >= 0)
                .and_then(|tokens| i32::try_from(tokens).ok())
            else {
                return malformed_upstream_response(
                    pool,
                    context,
                    api_key_id,
                    user_id,
                    model,
                    model_db_id,
                    provider_id,
                    latency,
                    "Anthropic upstream response is missing valid output usage",
                )
                .await;
            };
            let cost = calculate_token_cost(input_tokens, output_tokens, input_rate, output_rate);

            if !log_api_call(
                pool,
                context,
                api_key_id,
                user_id,
                model,
                model_db_id,
                provider_id,
                input_tokens,
                output_tokens,
                cost,
                latency,
                200,
                "",
            )
            .await
            {
                return api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Request completed upstream but billing could not be settled",
                    "billing_settlement_failed",
                    "BILLING_SETTLEMENT_FAILED",
                );
            }

            Json(protocol::anthropic_to_openai(&json, model)).into_response()
        }
        Err(e) => {
            let detail = e.to_string();
            log_api_call(
                pool,
                context,
                api_key_id,
                user_id,
                model,
                model_db_id,
                provider_id,
                0,
                0,
                0.0,
                latency,
                502,
                &detail,
            )
            .await;
            upstream_request_failed_response()
        }
    }
}

// ── Google Gemini ───────────────────────────────────────
async fn call_google(
    pool: &PgPool,
    context: &GatewayLogContext,
    client: &reqwest::Client,
    user_id: i64,
    api_key_id: Option<i64>,
    model: &str,
    input: &serde_json::Value,
    input_rate: f64,
    output_rate: f64,
    start_time: std::time::Instant,
    model_db_id: Option<i64>,
    provider_id: i64,
    api_key: &str,
) -> Response {
    let upstream_model = input
        .get("model")
        .and_then(|value| value.as_str())
        .unwrap_or(model);
    if api_key.is_empty() {
        let latency = start_time.elapsed().as_millis() as i32;
        let error_msg = "Google provider key is not configured";
        log_api_call(
            pool,
            context,
            api_key_id,
            user_id,
            model,
            model_db_id,
            provider_id,
            0,
            0,
            0.0,
            latency,
            503,
            error_msg,
        )
        .await;
        return api_error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            error_msg,
            "provider_not_configured",
            "PROVIDER_NOT_CONFIGURED",
        );
    }
    if input.get("stream").and_then(serde_json::Value::as_bool) == Some(true) {
        let latency = start_time.elapsed().as_millis() as i32;
        let message = "Gemini SSE conversion is not implemented";
        log_api_call(
            pool,
            context,
            api_key_id,
            user_id,
            model,
            model_db_id,
            provider_id,
            0,
            0,
            0.0,
            latency,
            501,
            message,
        )
        .await;
        return api_error_response(
            StatusCode::NOT_IMPLEMENTED,
            message,
            "capability_not_available",
            "STREAMING_NOT_AVAILABLE_FOR_PROVIDER",
        );
    }

    let body = match protocol::openai_to_gemini(input) {
        Ok(body) => body,
        Err(error) => {
            let latency = start_time.elapsed().as_millis() as i32;
            log_api_call(
                pool,
                context,
                api_key_id,
                user_id,
                model,
                model_db_id,
                provider_id,
                0,
                0,
                0.0,
                latency,
                400,
                &error.message,
            )
            .await;
            return protocol_error_response(error);
        }
    };

    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
        upstream_model
    );

    let response = client
        .post(&url)
        .header("x-goog-api-key", api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await;

    let latency = start_time.elapsed().as_millis() as i32;

    match response {
        Ok(resp) => {
            let status = resp.status();
            if !status.is_success() {
                let internal_error = format!("Upstream returned HTTP {}", status.as_u16());
                log_api_call(
                    pool,
                    context,
                    api_key_id,
                    user_id,
                    model,
                    model_db_id,
                    provider_id,
                    0,
                    0,
                    0.0,
                    latency,
                    status.as_u16() as i32,
                    &internal_error,
                )
                .await;
                return upstream_status_error_response(status);
            }

            let json: serde_json::Value = match resp.json().await {
                Ok(json) => json,
                Err(error) => {
                    tracing::error!(error = %error, "Gemini upstream returned invalid JSON");
                    return malformed_upstream_response(
                        pool,
                        context,
                        api_key_id,
                        user_id,
                        model,
                        model_db_id,
                        provider_id,
                        latency,
                        "Gemini upstream returned invalid JSON",
                    )
                    .await;
                }
            };

            let usage = json.get("usageMetadata");
            let Some(prompt_tokens) = usage
                .and_then(|usage| usage.get("promptTokenCount"))
                .and_then(serde_json::Value::as_i64)
                .filter(|tokens| *tokens >= 0)
                .and_then(|tokens| i32::try_from(tokens).ok())
            else {
                return malformed_upstream_response(
                    pool,
                    context,
                    api_key_id,
                    user_id,
                    model,
                    model_db_id,
                    provider_id,
                    latency,
                    "Gemini upstream response is missing valid prompt usage",
                )
                .await;
            };
            let Some(completion_tokens) = usage
                .and_then(|usage| usage.get("candidatesTokenCount"))
                .and_then(serde_json::Value::as_i64)
                .filter(|tokens| *tokens >= 0)
                .and_then(|tokens| i32::try_from(tokens).ok())
            else {
                return malformed_upstream_response(
                    pool,
                    context,
                    api_key_id,
                    user_id,
                    model,
                    model_db_id,
                    provider_id,
                    latency,
                    "Gemini upstream response is missing valid completion usage",
                )
                .await;
            };
            let cost =
                calculate_token_cost(prompt_tokens, completion_tokens, input_rate, output_rate);

            if !log_api_call(
                pool,
                context,
                api_key_id,
                user_id,
                model,
                model_db_id,
                provider_id,
                prompt_tokens,
                completion_tokens,
                cost,
                latency,
                200,
                "",
            )
            .await
            {
                return api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Request completed upstream but billing could not be settled",
                    "billing_settlement_failed",
                    "BILLING_SETTLEMENT_FAILED",
                );
            }

            Json(protocol::gemini_to_openai(&json, model)).into_response()
        }
        Err(e) => {
            let detail = e.to_string();
            log_api_call(
                pool,
                context,
                api_key_id,
                user_id,
                model,
                model_db_id,
                provider_id,
                0,
                0,
                0.0,
                latency,
                502,
                &detail,
            )
            .await;
            upstream_request_failed_response()
        }
    }
}

// Rate
async fn rate_limit_check(Extension(user): Extension<CurrentUser>) -> Response {
    let request_id = crate::utils::id_generator::generate_request_id();
    let Some(api_key_id) = user.api_key_id else {
        return with_nexus_headers(
            api_error_response(
                StatusCode::UNAUTHORIZED,
                "Use X-API-Key to read rate limit state",
                "api_key_required",
                "API_KEY_REQUIRED",
            ),
            &request_id,
            None,
            None,
        );
    };
    let Some(limit) = user.rate_limit else {
        return with_nexus_headers(
            api_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "API key rate limit is not configured",
                "rate_limit_configuration_invalid",
                "RATE_LIMIT_CONFIGURATION_INVALID",
            ),
            &request_id,
            None,
            None,
        );
    };
    let key = format!("nexus:rate_limit:api_key:{}", api_key_id);
    let count = match redis_optional_integer_command(vec!["GET".to_string(), key.clone()]).await {
        Ok(value) => value.unwrap_or(0),
        Err(error) => {
            tracing::error!(error = %error, "Failed to read rate limit state");
            return with_nexus_headers(
                api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Rate limiter unavailable",
                    "rate_limiter_unavailable",
                    "RATE_LIMITER_UNAVAILABLE",
                ),
                &request_id,
                None,
                None,
            );
        }
    };
    let reset_secs = if count == 0 {
        0
    } else {
        match redis_integer_command(vec!["TTL".to_string(), key]).await {
            Ok(value) if value >= 0 => value,
            Ok(value) => {
                tracing::error!(ttl = value, "Rate limit state has no valid expiry");
                return with_nexus_headers(
                    api_error_response(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Rate limiter unavailable",
                        "rate_limiter_unavailable",
                        "RATE_LIMITER_UNAVAILABLE",
                    ),
                    &request_id,
                    None,
                    None,
                );
            }
            Err(error) => {
                tracing::error!(error = %error, "Failed to read rate limit expiry");
                return with_nexus_headers(
                    api_error_response(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Rate limiter unavailable",
                        "rate_limiter_unavailable",
                        "RATE_LIMITER_UNAVAILABLE",
                    ),
                    &request_id,
                    None,
                    None,
                );
            }
        }
    };
    let remaining = (i64::from(limit) - count).max(0);
    with_nexus_headers(
        Json(json!({
            "code": 0,
            "data": {
                "limit": limit,
                "used": count,
                "remaining": remaining,
                "reset_seconds": reset_secs,
                "window_seconds": 3600
            }
        }))
        .into_response(),
        &request_id,
        None,
        None,
    )
}

// Logs - 调用统计 (Stage 3.1)
async fn call_logs(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Json<serde_json::Value> {
    use sqlx::Row;
    let result = sqlx::query(
        "SELECT ac.id, ac.request_id, ac.model_id, ac.input_tokens, ac.output_tokens,
                CAST(ac.cost_points AS VARCHAR), ac.latency_ms, ac.status_code, ac.created_at
         FROM api_calls ac
         WHERE ac.workspace_id = $1
         ORDER BY ac.created_at DESC LIMIT 20",
    )
    .bind(user.workspace_id)
    .fetch_all(&pool)
    .await;

    match result {
        Ok(rows) => {
            let items: Vec<_> = rows
                .iter()
                .map(|r| {
                    json!({
                        "id": r.get::<i64, _>(0),
                        "request_id": r.get::<String, _>(1),
                        "model_id": r.get::<String, _>(2),
                        "input_tokens": r.get::<i32, _>(3),
                        "output_tokens": r.get::<i32, _>(4),
                        "cost_points": r.get::<String, _>(5),
                        "latency_ms": r.get::<i32, _>(6),
                        "status_code": r.get::<i32, _>(7),
                        "created_at": r.get::<chrono::NaiveDateTime, _>(8)
                    })
                })
                .collect();
            Json(
                json!({"code": 0, "data": {"items": items, "pagination": {"page": 1, "limit": 20, "total": items.len()}}}),
            )
        }
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn call_stats(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Json<serde_json::Value> {
    use sqlx::Row;
    let result = sqlx::query(
        "SELECT COUNT(*), COALESCE(SUM(input_tokens + output_tokens), 0):: INTEGER
         FROM api_calls WHERE workspace_id = $1 AND created_at > NOW() - INTERVAL '24 hours'",
    )
    .bind(user.workspace_id)
    .fetch_optional(&pool)
    .await;

    match result {
        Ok(Some(row)) => Json(json!({
            "code": 0,
            "data": {
                "total_calls": row.get::<i64, _>(0),
                "total_tokens": row.get::<i32, _>(1),
                "period": "24h"
            }
        })),
        Ok(None) => Json(json!({"code": 0, "data": {"total_calls": 0, "total_tokens": 0}})),
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

// Docs - API 文档页 (Stage 3.2)
async fn api_docs() -> Json<serde_json::Value> {
    Json(json!({
        "code": 0,
        "message": "API Documentation",
        "data": {
            "title": "Nexus Gateway API",
            "version": "1.0",
            "base_url": "/api",
            "sections": [
                {
                    "id": "auth",
                    "title": "Authentication",
                    "description": "Model routes use X-API-Key. Console routes use the login JWT.",
                    "examples": [
                        "X-API-Key: nr-sk-...",
                        "Authorization: Bearer LOGIN_JWT"
                    ]
                },
                {
                    "id": "endpoints",
                    "title": "API Endpoints",
                    "items": [
                        {"method": "POST", "path": "/v1/chat/completions", "description": "AI chat"},
                        {"method": "POST", "path": "/v1/responses", "description": "Non-streaming Responses API"},
                        {"method": "GET", "path": "/v1/models", "description": "List models and capabilities"},
                        {"method": "GET", "path": "/balance", "description": "Console balance, login JWT required"},
                        {"method": "GET", "path": "/logs/calls", "description": "Console request ledger, login JWT required"}
                    ]
                },
                {
                    "id": "errors",
                    "title": "Error Codes",
                    "codes": [
                        {"code": 0, "message": "Success"},
                        {"code": 400, "message": "Invalid parameter"},
                        {"code": 401, "message": "Unauthorized"},
                        {"code": 402, "message": "Insufficient balance"},
                        {"code": 429, "message": "Rate limited"},
                        {"code": 500, "message": "Server error"}
                    ]
                }
            ]
        }
    }))
}

async fn openapi_spec() -> Json<serde_json::Value> {
    Json(json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Nexus Gateway API",
            "version": "1.0.0",
            "description": "OpenAI-compatible model gateway with one platform balance and one API key."
        },
        "servers": [
            { "url": "https://openbridgetech.ca/api", "description": "Production" },
            { "url": "http://127.0.0.1:8080", "description": "Local development" }
        ],
        "security": [{ "ApiKeyAuth": [] }],
        "paths": {
            "/v1/chat/completions": {
                "post": {
                    "summary": "Create a chat completion",
                    "description": "Choose any enabled model in the request. Nexus Gateway routes the call and deducts the unified balance after success.",
                    "operationId": "createChatCompletion",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/ChatCompletionRequest" },
                                "example": {
                                    "model": "gpt-4.1-mini",
                                    "messages": [
                                        { "role": "user", "content": "Hello" }
                                    ],
                                    "temperature": 0.7,
                                    "max_tokens": 512
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "OpenAI-compatible chat completion response",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/ChatCompletionResponse" }
                                }
                            }
                        },
                        "401": { "$ref": "#/components/responses/Unauthorized" },
                        "402": { "$ref": "#/components/responses/InsufficientBalance" },
                        "403": { "$ref": "#/components/responses/ModelNotAllowed" },
                        "429": { "$ref": "#/components/responses/RateLimited" },
                        "502": { "$ref": "#/components/responses/UpstreamFailed" },
                        "503": { "$ref": "#/components/responses/ProviderUnavailable" }
                    }
                }
            },
            "/v1/responses": {
                "post": {
                    "summary": "Create a non-streaming response",
                    "description": "Adapts the Responses request to the selected provider. Streaming Responses events are not supported.",
                    "operationId": "createResponse",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"type": "object", "required": ["model", "input"]}}}},
                    "responses": {
                        "200": {"description": "Responses-compatible completed response"},
                        "400": {"description": "Invalid or unsupported Responses parameter"},
                        "501": {"description": "Requested protocol combination is not available"}
                    }
                }
            },
            "/v1/embeddings": {
                "post": {
                    "summary": "Create embeddings",
                    "description": "Returns 501 until a real enabled embedding provider endpoint is configured.",
                    "operationId": "createEmbedding",
                    "responses": {"501": {"description": "No real embedding endpoint is configured"}}
                }
            },
            "/v1/images/generations": {
                "post": {"summary": "Create an asynchronous image task", "responses": {"501": {"description": "No real asynchronous provider endpoint is configured"}}}
            },
            "/v1/audio/generations": {
                "post": {"summary": "Create an asynchronous audio task", "responses": {"501": {"description": "No real asynchronous provider endpoint is configured"}}}
            },
            "/v1/videos/generations": {
                "post": {"summary": "Create an asynchronous video task", "responses": {"501": {"description": "No real asynchronous provider endpoint is configured"}}}
            },
            "/v1/tasks/{uid}": {
                "get": {"summary": "Read asynchronous task state", "parameters": [{"name":"uid","in":"path","required":true,"schema":{"type":"string"}}], "responses": {"200":{"description":"Task state"},"404":{"description":"Task not found"}}},
                "delete": {"summary": "Cancel a queued asynchronous task", "parameters": [{"name":"uid","in":"path","required":true,"schema":{"type":"string"}}], "responses": {"200":{"description":"Task cancelled"},"409":{"description":"Task cannot be cancelled"}}}
            },
            "/v1/models": {
                "get": {
                    "summary": "List enabled models",
                    "operationId": "listModels",
                    "responses": {
                        "200": {
                            "description": "Enabled model list",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/ModelListResponse" }
                                }
                            }
                        }
                    }
                }
            },
            "/balance": {
                "get": {
                    "summary": "Get account balance",
                    "operationId": "getBalance",
                    "responses": {
                        "200": {
                            "description": "Current user balance",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/BalanceResponse" }
                                }
                            }
                        },
                        "401": { "$ref": "#/components/responses/Unauthorized" }
                    }
                }
            },
            "/keys": {
                "get": {
                    "summary": "List API keys",
                    "operationId": "listKeys",
                    "responses": {
                        "200": {
                            "description": "API key list",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/ApiKeyListResponse" }
                                }
                            }
                        },
                        "401": { "$ref": "#/components/responses/Unauthorized" }
                    }
                },
                "post": {
                    "summary": "Create an API key",
                    "operationId": "createKey",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": { "$ref": "#/components/schemas/CreateKeyRequest" },
                                "example": {
                                    "name": "Production API",
                                    "rate_limit": 500,
                                    "daily_spend_limit": 0.5,
                                    "models_allowed": ["gpt-4.1-mini"]
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "Created API key. Plain key is only returned once.",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/CreateKeyResponse" }
                                }
                            }
                        },
                        "401": { "$ref": "#/components/responses/Unauthorized" }
                    }
                }
            },
            "/logs/calls": {
                "get": {
                    "summary": "List call logs",
                    "operationId": "listCallLogs",
                    "responses": {
                        "200": {
                            "description": "Recent call logs",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/CallLogListResponse" }
                                }
                            }
                        },
                        "401": { "$ref": "#/components/responses/Unauthorized" }
                    }
                }
            }
        },
        "components": {
            "securitySchemes": {
                "ApiKeyAuth": {
                    "type": "apiKey",
                    "in": "header",
                    "name": "X-API-Key",
                    "description": "Use the platform API key created in Nexus Gateway."
                }
            },
            "responses": {
                "Unauthorized": {
                    "description": "Missing or invalid API key",
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/ErrorResponse" },
                            "example": { "code": 401, "error": "UNAUTHORIZED", "message": "Unauthorized" }
                        }
                    }
                },
                "InsufficientBalance": {
                    "description": "Unified balance is not enough",
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/ErrorResponse" },
                            "example": { "code": 402, "error": "INSUFFICIENT_BALANCE", "message": "Insufficient balance" }
                        }
                    }
                },
                "ModelNotAllowed": {
                    "description": "The API key is not allowed to call this model",
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/ErrorResponse" },
                            "example": { "code": 403, "error": "MODEL_NOT_ALLOWED", "message": "Model not allowed for this API key" }
                        }
                    }
                },
                "RateLimited": {
                    "description": "Hourly rate limit or daily spend limit reached",
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/ErrorResponse" },
                            "examples": {
                                "rateLimit": { "value": { "code": 429, "error": "RATE_LIMIT_EXCEEDED", "message": "Rate limit exceeded" } },
                                "dailyLimit": { "value": { "code": 429, "error": "KEY_DAILY_LIMIT_EXCEEDED", "message": "API key daily spend limit exceeded" } }
                            }
                        }
                    }
                },
                "UpstreamFailed": {
                    "description": "Upstream model provider request failed",
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/ErrorResponse" },
                            "examples": {
                                "upstreamApi": { "value": { "code": 502, "error": "UPSTREAM_API_ERROR", "message": "Upstream API returned an error" } },
                                "requestFailed": { "value": { "code": 502, "error": "UPSTREAM_REQUEST_FAILED", "message": "Upstream request failed" } }
                            }
                        }
                    }
                },
                "ProviderUnavailable": {
                    "description": "Provider key missing or circuit breaker opened",
                    "content": {
                        "application/json": {
                            "schema": { "$ref": "#/components/schemas/ErrorResponse" },
                            "examples": {
                                "notConfigured": { "value": { "code": 503, "error": "PROVIDER_NOT_CONFIGURED", "message": "Provider API key not configured" } },
                                "circuitOpen": { "value": { "code": 503, "error": "PROVIDER_CIRCUIT_OPEN", "message": "Provider circuit is open" } }
                            }
                        }
                    }
                }
            },
            "schemas": {
                "ChatCompletionRequest": {
                    "type": "object",
                    "required": ["model", "messages"],
                    "properties": {
                        "model": { "type": "string", "example": "gpt-4.1-mini" },
                        "messages": {
                            "type": "array",
                            "items": { "$ref": "#/components/schemas/ChatMessage" }
                        },
                        "temperature": { "type": "number", "minimum": 0, "maximum": 2, "default": 1 },
                        "max_tokens": { "type": "integer", "minimum": 1, "default": 512 },
                        "stream": { "type": "boolean", "default": false }
                    }
                },
                "ChatMessage": {
                    "type": "object",
                    "required": ["role", "content"],
                    "properties": {
                        "role": { "type": "string", "enum": ["system", "user", "assistant"] },
                        "content": { "type": "string" }
                    }
                },
                "ChatCompletionResponse": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "object": { "type": "string", "example": "chat.completion" },
                        "model": { "type": "string" },
                        "choices": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "index": { "type": "integer" },
                                    "message": { "$ref": "#/components/schemas/ChatMessage" },
                                    "finish_reason": { "type": "string" }
                                }
                            }
                        },
                        "usage": {
                            "type": "object",
                            "properties": {
                                "prompt_tokens": { "type": "integer" },
                                "completion_tokens": { "type": "integer" },
                                "total_tokens": { "type": "integer" }
                            }
                        }
                    }
                },
                "ModelListResponse": {
                    "type": "object",
                    "properties": {
                        "code": { "type": "integer", "example": 0 },
                        "data": {
                            "type": "object",
                            "properties": {
                                "items": {
                                    "type": "array",
                                    "items": { "type": "object" }
                                }
                            }
                        }
                    }
                },
                "BalanceResponse": {
                    "type": "object",
                    "properties": {
                        "code": { "type": "integer", "example": 0 },
                        "data": {
                            "type": "object",
                            "properties": {
                                "balance": { "type": "number" },
                                "total_consumed": { "type": "number" },
                                "total_recharged": { "type": "number" }
                            }
                        }
                    }
                },
                "CreateKeyRequest": {
                    "type": "object",
                    "required": ["name"],
                    "properties": {
                        "name": { "type": "string" },
                        "rate_limit": { "type": "integer", "default": 500 },
                        "daily_spend_limit": { "type": "number" },
                        "models_allowed": {
                            "type": "array",
                            "items": { "type": "string" }
                        }
                    }
                },
                "CreateKeyResponse": {
                    "type": "object",
                    "properties": {
                        "code": { "type": "integer", "example": 0 },
                        "data": {
                            "type": "object",
                            "properties": {
                                "key": { "type": "string", "description": "Plain key is shown once." },
                                "uid": { "type": "string" },
                                "key_prefix": { "type": "string" }
                            }
                        }
                    }
                },
                "ApiKeyListResponse": {
                    "type": "object",
                    "properties": {
                        "code": { "type": "integer", "example": 0 },
                        "data": {
                            "type": "object",
                            "properties": {
                                "items": {
                                    "type": "array",
                                    "items": { "type": "object" }
                                }
                            }
                        }
                    }
                },
                "CallLogListResponse": {
                    "type": "object",
                    "properties": {
                        "code": { "type": "integer", "example": 0 },
                        "data": {
                            "type": "object",
                            "properties": {
                                "items": {
                                    "type": "array",
                                    "items": { "type": "object" }
                                }
                            }
                        }
                    }
                },
                "ErrorResponse": {
                    "type": "object",
                    "properties": {
                        "code": { "type": "integer" },
                        "error": { "type": "string" },
                        "message": { "type": "string" },
                        "upstream_status": { "type": "integer" }
                    }
                }
            }
        }
    }))
}

// Admin - 用户管理 (Stage 3.3)
fn read_required_f64(input: &serde_json::Value, field: &str) -> Result<f64, String> {
    let value = input
        .get(field)
        .and_then(|item| item.as_f64())
        .ok_or_else(|| format!("{} is required", field))?;

    if !value.is_finite() {
        return Err(format!("{} must be a finite number", field));
    }

    Ok(value)
}

fn read_required_i16(input: &serde_json::Value, field: &str) -> Result<i16, String> {
    let value = input
        .get(field)
        .and_then(|item| item.as_i64())
        .ok_or_else(|| format!("{} is required", field))?;

    i16::try_from(value).map_err(|_| format!("{} is out of range", field))
}

fn validate_pricing_number(value: f64, field: &str) -> Result<(), String> {
    if !(0.0..=9999.999999).contains(&value) {
        return Err(format!("{} must be between 0 and 9999.999999", field));
    }

    Ok(())
}

fn parse_decimal_text(value: String, field: &str) -> Result<f64, String> {
    value
        .parse::<f64>()
        .map_err(|_| format!("Invalid {} in database", field))
}

fn provider_health_label(
    status: i16,
    key_configured: bool,
    total_calls: i64,
    failed_calls: i64,
) -> &'static str {
    if status != 0 {
        return "disabled";
    }
    if !key_configured {
        return "key_missing";
    }
    if total_calls == 0 {
        return "unverified";
    }

    let fail_rate = failed_calls as f64 / total_calls as f64;
    if total_calls >= 5 && fail_rate >= 0.6 {
        "down"
    } else if fail_rate >= 0.2 {
        "degraded"
    } else {
        "healthy"
    }
}

async fn log_admin_action(
    pool: &PgPool,
    admin: &CurrentUser,
    action: &str,
    target_type: &str,
    target_id: &str,
    before_data: serde_json::Value,
    after_data: serde_json::Value,
) {
    sqlx::query(
        "INSERT INTO admin_audit_logs (admin_email, action, target_type, target_id, before_data, after_data)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&admin.email)
    .bind(action)
    .bind(target_type)
    .bind(target_id)
    .bind(before_data)
    .bind(after_data)
    .execute(pool)
    .await
    .ok();
}

async fn provider_circuit_open(pool: &PgPool, provider_id: i64) -> Result<Option<String>, String> {
    use sqlx::Row;

    let row = sqlx::query(
        "SELECT COUNT(*)::BIGINT,
                COALESCE(SUM(CASE WHEN status_code >= 500 THEN 1 ELSE 0 END), 0)::BIGINT
         FROM api_calls
         WHERE provider_id = $1 AND created_at >= NOW() - INTERVAL '10 minutes'",
    )
    .bind(provider_id)
    .fetch_one(pool)
    .await
    .map_err(|e| sanitize_error(e))?;

    let total: i64 = row.get(0);
    let failed: i64 = row.get(1);
    if total >= 5 && (failed as f64 / total as f64) >= 0.6 {
        Ok(Some(format!(
            "Provider circuit open: {} of {} recent calls failed",
            failed, total
        )))
    } else {
        Ok(None)
    }
}

async fn admin_models(State(pool): State<PgPool>) -> Json<serde_json::Value> {
    use sqlx::Row;

    let result = sqlx::query(
        "SELECT pm.id, pm.model_id, pm.name, pm.display_name,
                CAST(COALESCE(pm.upstream_input_rate, pm.input_rate) AS VARCHAR),
                CAST(COALESCE(pm.upstream_output_rate, pm.output_rate) AS VARCHAR),
                CAST(COALESCE(pm.margin_rate, 0) AS VARCHAR),
                CAST(pm.input_rate AS VARCHAR),
                CAST(pm.output_rate AS VARCHAR),
                pm.context_len, pm.max_tokens, pm.status, pm.sort,
                p.provider_id, p.name
         FROM provider_models pm
         JOIN providers p ON pm.provider_id = p.id
         ORDER BY p.sort, pm.sort, pm.id",
    )
    .fetch_all(&pool)
    .await;

    match result {
        Ok(rows) => {
            let items = rows
                .iter()
                .map(|r| {
                    let upstream_input_rate =
                        parse_decimal_text(r.get::<String, _>(4), "upstream_input_rate")?;
                    let upstream_output_rate =
                        parse_decimal_text(r.get::<String, _>(5), "upstream_output_rate")?;
                    let margin_rate = parse_decimal_text(r.get::<String, _>(6), "margin_rate")?;
                    let input_rate = parse_decimal_text(r.get::<String, _>(7), "input_rate")?;
                    let output_rate = parse_decimal_text(r.get::<String, _>(8), "output_rate")?;

                    Ok(json!({
                        "id": r.get::<i64, _>(0),
                        "model_id": r.get::<String, _>(1),
                        "name": r.get::<String, _>(2),
                        "display_name": r.get::<Option<String>, _>(3),
                        "upstream_input_rate": upstream_input_rate,
                        "upstream_output_rate": upstream_output_rate,
                        "margin_rate": margin_rate,
                        "input_rate": input_rate,
                        "output_rate": output_rate,
                        "context_len": r.get::<i32, _>(9),
                        "max_tokens": r.get::<i32, _>(10),
                        "status": r.get::<i16, _>(11),
                        "sort": r.get::<i32, _>(12),
                        "provider_id": r.get::<String, _>(13),
                        "provider": r.get::<String, _>(14)
                    }))
                })
                .collect::<Result<Vec<_>, String>>();

            match items {
                Ok(items) => Json(json!({"code": 0, "data": {"items": items}})),
                Err(message) => Json(json!({"code": 500, "message": message})),
            }
        }
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn update_admin_model_pricing(
    Path(id): Path<i64>,
    State(pool): State<PgPool>,
    Extension(admin): Extension<CurrentUser>,
    Json(input): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    use sqlx::Row;

    let upstream_input_rate = match read_required_f64(&input, "upstream_input_rate") {
        Ok(value) => value,
        Err(message) => return Json(json!({"code": 400, "message": message})),
    };
    let upstream_output_rate = match read_required_f64(&input, "upstream_output_rate") {
        Ok(value) => value,
        Err(message) => return Json(json!({"code": 400, "message": message})),
    };
    let margin_rate = match read_required_f64(&input, "margin_rate") {
        Ok(value) => value,
        Err(message) => return Json(json!({"code": 400, "message": message})),
    };
    let status = match read_required_i16(&input, "status") {
        Ok(value @ (0 | 1)) => value,
        Ok(_) => return Json(json!({"code": 400, "message": "status must be 0 or 1"})),
        Err(message) => return Json(json!({"code": 400, "message": message})),
    };

    for (value, field) in [
        (upstream_input_rate, "upstream_input_rate"),
        (upstream_output_rate, "upstream_output_rate"),
        (margin_rate, "margin_rate"),
    ] {
        if let Err(message) = validate_pricing_number(value, field) {
            return Json(json!({"code": 400, "message": message}));
        }
    }

    let multiplier = 1.0 + margin_rate / 100.0;
    let input_rate = upstream_input_rate * multiplier;
    let output_rate = upstream_output_rate * multiplier;
    for (value, field) in [(input_rate, "input_rate"), (output_rate, "output_rate")] {
        if let Err(message) = validate_pricing_number(value, field) {
            return Json(json!({"code": 400, "message": message}));
        }
    }

    let before = match sqlx::query(
        "SELECT id, model_id, CAST(upstream_input_rate AS VARCHAR), CAST(upstream_output_rate AS VARCHAR),
                CAST(margin_rate AS VARCHAR), CAST(input_rate AS VARCHAR), CAST(output_rate AS VARCHAR), status
         FROM provider_models WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&pool)
    .await
    {
        Ok(Some(row)) => json!({
            "id": row.get::<i64, _>(0),
            "model_id": row.get::<String, _>(1),
            "upstream_input_rate": row.get::<Option<String>, _>(2),
            "upstream_output_rate": row.get::<Option<String>, _>(3),
            "margin_rate": row.get::<Option<String>, _>(4),
            "input_rate": row.get::<String, _>(5),
            "output_rate": row.get::<String, _>(6),
            "status": row.get::<i16, _>(7)
        }),
        Ok(None) => return Json(json!({"code": 404, "message": "Model not found"})),
        Err(e) => return Json(json!({"code": 500, "message": sanitize_error(e)})),
    };

    let result = sqlx::query(
        "UPDATE provider_models
         SET upstream_input_rate = $1,
             upstream_output_rate = $2,
             margin_rate = $3,
             input_rate = $4,
             output_rate = $5,
             status = $6,
             updated_at = NOW()
         WHERE id = $7
         RETURNING id, model_id, display_name,
                   CAST(upstream_input_rate AS VARCHAR),
                   CAST(upstream_output_rate AS VARCHAR),
                   CAST(margin_rate AS VARCHAR),
                   CAST(input_rate AS VARCHAR),
                   CAST(output_rate AS VARCHAR),
                   status",
    )
    .bind(upstream_input_rate)
    .bind(upstream_output_rate)
    .bind(margin_rate)
    .bind(input_rate)
    .bind(output_rate)
    .bind(status)
    .bind(id)
    .fetch_optional(&pool)
    .await;

    match result {
        Ok(Some(row)) => {
            let upstream_input_rate =
                match parse_decimal_text(row.get::<String, _>(3), "upstream_input_rate") {
                    Ok(value) => value,
                    Err(message) => return Json(json!({"code": 500, "message": message})),
                };
            let upstream_output_rate =
                match parse_decimal_text(row.get::<String, _>(4), "upstream_output_rate") {
                    Ok(value) => value,
                    Err(message) => return Json(json!({"code": 500, "message": message})),
                };
            let margin_rate = match parse_decimal_text(row.get::<String, _>(5), "margin_rate") {
                Ok(value) => value,
                Err(message) => return Json(json!({"code": 500, "message": message})),
            };
            let input_rate = match parse_decimal_text(row.get::<String, _>(6), "input_rate") {
                Ok(value) => value,
                Err(message) => return Json(json!({"code": 500, "message": message})),
            };
            let output_rate = match parse_decimal_text(row.get::<String, _>(7), "output_rate") {
                Ok(value) => value,
                Err(message) => return Json(json!({"code": 500, "message": message})),
            };

            let after = json!({
                "id": row.get::<i64, _>(0),
                "model_id": row.get::<String, _>(1),
                "display_name": row.get::<Option<String>, _>(2),
                "upstream_input_rate": upstream_input_rate,
                "upstream_output_rate": upstream_output_rate,
                "margin_rate": margin_rate,
                "input_rate": input_rate,
                "output_rate": output_rate,
                "status": row.get::<i16, _>(8)
            });

            log_admin_action(
                &pool,
                &admin,
                "update_model_pricing",
                "provider_model",
                &id.to_string(),
                before,
                after.clone(),
            )
            .await;

            Json(json!({
                "code": 0,
                "data": after
            }))
        }
        Ok(None) => Json(json!({"code": 404, "message": "Model not found"})),
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn admin_providers(State(pool): State<PgPool>) -> Json<serde_json::Value> {
    use sqlx::Row;

    let result = sqlx::query(
        "SELECT p.id, p.provider_id, p.name, p.base_url, p.api_key_env, p.status, p.sort,
                model_stats.model_count, model_stats.active_model_count,
                call_stats.calls_24h, call_stats.failed_calls_24h,
                call_stats.avg_latency_ms, call_stats.last_call_at
         FROM providers p
         LEFT JOIN LATERAL (
             SELECT COUNT(*)::BIGINT AS model_count,
                    COUNT(*) FILTER (WHERE status = 0)::BIGINT AS active_model_count
             FROM provider_models
             WHERE provider_id = p.id
         ) model_stats ON TRUE
         LEFT JOIN LATERAL (
             SELECT COUNT(*)::BIGINT AS calls_24h,
                    COUNT(*) FILTER (WHERE status_code >= 400)::BIGINT AS failed_calls_24h,
                    COALESCE(CAST(AVG(latency_ms) AS VARCHAR), '0') AS avg_latency_ms,
                    MAX(created_at) AS last_call_at
             FROM api_calls
             WHERE provider_id = p.id AND created_at >= NOW() - INTERVAL '24 hours'
         ) call_stats ON TRUE
         ORDER BY p.sort, p.id",
    )
    .fetch_all(&pool)
    .await;

    match result {
        Ok(rows) => {
            let items: Vec<_> = rows
                .iter()
                .map(|r| {
                    let api_key_env = r.get::<Option<String>, _>(4).unwrap_or_default();
                    let key_configured = !api_key_env.is_empty()
                        && !crate::config::provider_api_key(&api_key_env).is_empty();
                    let total_calls = r.get::<i64, _>(9);
                    let failed_calls = r.get::<i64, _>(10);
                    let status = r.get::<i16, _>(5);

                    json!({
                        "id": r.get::<i64, _>(0),
                        "provider_id": r.get::<String, _>(1),
                        "name": r.get::<String, _>(2),
                        "base_url": r.get::<Option<String>, _>(3),
                        "api_key_env": api_key_env,
                        "key_configured": key_configured,
                        "status": status,
                        "sort": r.get::<i32, _>(6),
                        "model_count": r.get::<i64, _>(7),
                        "active_model_count": r.get::<i64, _>(8),
                        "calls_24h": total_calls,
                        "failed_calls_24h": failed_calls,
                        "avg_latency_ms": r.get::<String, _>(11).parse::<f64>().expect("database average latency must parse").round() as i64,
                        "last_call_at": r.get::<Option<chrono::NaiveDateTime>, _>(12),
                        "health": provider_health_label(status, key_configured, total_calls, failed_calls),
                    })
                })
                .collect();
            Json(json!({"code": 0, "data": {"items": items}}))
        }
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn update_admin_provider(
    Path(id): Path<i64>,
    State(pool): State<PgPool>,
    Extension(admin): Extension<CurrentUser>,
    Json(input): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    use sqlx::Row;

    let name = match input.get("name").and_then(|value| value.as_str()) {
        Some(value) if !value.trim().is_empty() => value.trim(),
        _ => return Json(json!({"code": 400, "message": "name is required"})),
    };
    let base_url = match input.get("base_url").and_then(|value| value.as_str()) {
        Some(value) if !value.trim().is_empty() => value.trim(),
        _ => return Json(json!({"code": 400, "message": "base_url is required"})),
    };
    let api_key_env = match input.get("api_key_env").and_then(|value| value.as_str()) {
        Some(value) if !value.trim().is_empty() => value.trim(),
        _ => return Json(json!({"code": 400, "message": "api_key_env is required"})),
    };
    let status = match read_required_i16(&input, "status") {
        Ok(value @ (0 | 1)) => value,
        Ok(_) => return Json(json!({"code": 400, "message": "status must be 0 or 1"})),
        Err(message) => return Json(json!({"code": 400, "message": message})),
    };
    let sort = input
        .get("sort")
        .and_then(|value| value.as_i64())
        .unwrap_or(0) as i32;

    let before = match sqlx::query(
        "SELECT id, provider_id, name, base_url, api_key_env, status, sort FROM providers WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&pool)
    .await
    {
        Ok(Some(row)) => json!({
            "id": row.get::<i64, _>(0),
            "provider_id": row.get::<String, _>(1),
            "name": row.get::<String, _>(2),
            "base_url": row.get::<Option<String>, _>(3),
            "api_key_env": row.get::<Option<String>, _>(4),
            "status": row.get::<i16, _>(5),
            "sort": row.get::<i32, _>(6),
        }),
        Ok(None) => return Json(json!({"code": 404, "message": "Provider not found"})),
        Err(e) => return Json(json!({"code": 500, "message": sanitize_error(e)})),
    };

    let result = sqlx::query(
        "UPDATE providers
         SET name = $1, base_url = $2, api_key_env = $3, status = $4, sort = $5, updated_at = NOW()
         WHERE id = $6
         RETURNING id, provider_id, name, base_url, api_key_env, status, sort",
    )
    .bind(name)
    .bind(base_url)
    .bind(api_key_env)
    .bind(status)
    .bind(sort)
    .bind(id)
    .fetch_optional(&pool)
    .await;

    match result {
        Ok(Some(row)) => {
            let after = json!({
                "id": row.get::<i64, _>(0),
                "provider_id": row.get::<String, _>(1),
                "name": row.get::<String, _>(2),
                "base_url": row.get::<Option<String>, _>(3),
                "api_key_env": row.get::<Option<String>, _>(4),
                "status": row.get::<i16, _>(5),
                "sort": row.get::<i32, _>(6),
            });
            log_admin_action(
                &pool,
                &admin,
                "update_provider",
                "provider",
                &id.to_string(),
                before,
                after.clone(),
            )
            .await;
            Json(json!({"code": 0, "data": after}))
        }
        Ok(None) => Json(json!({"code": 404, "message": "Provider not found"})),
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn admin_profit_report(State(pool): State<PgPool>) -> Json<serde_json::Value> {
    use sqlx::Row;

    let summary = sqlx::query(
        "SELECT COUNT(ac.id)::BIGINT,
                COALESCE(CAST(SUM(ac.user_cost_usd) AS VARCHAR), '0'),
                COALESCE(CAST(SUM(ac.provider_cost_usd) AS VARCHAR), '0')
         FROM api_calls ac
         WHERE ac.state = 'succeeded' AND ac.created_at >= NOW() - INTERVAL '30 days'",
    )
    .fetch_one(&pool)
    .await;

    let models = sqlx::query(
        "SELECT ac.model_id, COALESCE(p.name, 'Unknown'),
                COUNT(ac.id)::BIGINT,
                COALESCE(CAST(SUM(ac.user_cost_usd) AS VARCHAR), '0'),
                COALESCE(CAST(SUM(ac.provider_cost_usd) AS VARCHAR), '0')
         FROM api_calls ac
         LEFT JOIN providers p ON p.id = ac.provider_id
         WHERE ac.state = 'succeeded' AND ac.created_at >= NOW() - INTERVAL '30 days'
         GROUP BY ac.model_id, p.name
         ORDER BY SUM(ac.user_cost_usd) DESC NULLS LAST
         LIMIT 50",
    )
    .fetch_all(&pool)
    .await;

    match (summary, models) {
        (Ok(summary), Ok(rows)) => {
            let revenue = summary
                .get::<String, _>(1)
                .parse::<f64>()
                .expect("database revenue must parse");
            let cost = summary
                .get::<String, _>(2)
                .parse::<f64>()
                .expect("database provider cost must parse");
            let profit = revenue - cost;
            let margin_rate = if revenue > 0.0 {
                profit / revenue * 100.0
            } else {
                0.0
            };
            let items: Vec<_> = rows
                .iter()
                .map(|r| {
                    let revenue = r
                        .get::<String, _>(3)
                        .parse::<f64>()
                        .expect("database revenue must parse");
                    let cost = r
                        .get::<String, _>(4)
                        .parse::<f64>()
                        .expect("database provider cost must parse");
                    let profit = revenue - cost;
                    json!({
                        "model_id": r.get::<String, _>(0),
                        "provider": r.get::<String, _>(1),
                        "calls": r.get::<i64, _>(2),
                        "revenue": revenue,
                        "upstream_cost": cost,
                        "gross_profit": profit,
                        "margin_rate": if revenue > 0.0 { profit / revenue * 100.0 } else { 0.0 }
                    })
                })
                .collect();

            Json(json!({
                "code": 0,
                "data": {
                    "window": "30d",
                    "calls": summary.get::<i64, _>(0),
                    "revenue": revenue,
                    "upstream_cost": cost,
                    "gross_profit": profit,
                    "margin_rate": margin_rate,
                    "items": items
                }
            }))
        }
        (Err(e), _) | (_, Err(e)) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn admin_audit_logs(State(pool): State<PgPool>) -> Json<serde_json::Value> {
    use sqlx::Row;

    let result = sqlx::query(
        "SELECT id, admin_email, action, target_type, target_id, before_data, after_data, created_at
         FROM admin_audit_logs
         ORDER BY created_at DESC
         LIMIT 100",
    )
    .fetch_all(&pool)
    .await;

    match result {
        Ok(rows) => {
            let items: Vec<_> = rows
                .iter()
                .map(|r| {
                    json!({
                        "id": r.get::<i64, _>(0),
                        "admin_email": r.get::<String, _>(1),
                        "action": r.get::<String, _>(2),
                        "target_type": r.get::<String, _>(3),
                        "target_id": r.get::<String, _>(4),
                        "before_data": r.get::<Option<serde_json::Value>, _>(5),
                        "after_data": r.get::<Option<serde_json::Value>, _>(6),
                        "created_at": r.get::<chrono::NaiveDateTime, _>(7),
                    })
                })
                .collect();
            Json(json!({"code": 0, "data": {"items": items}}))
        }
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn admin_risk_users(State(pool): State<PgPool>) -> Json<serde_json::Value> {
    use sqlx::Row;

    let result = sqlx::query(
        "WITH key_stats AS (
             SELECT user_id,
                    COUNT(*)::BIGINT AS key_count,
                    COALESCE(SUM(CASE WHEN status = 0 THEN 1 ELSE 0 END), 0)::BIGINT AS active_key_count
             FROM api_keys
             WHERE status IN (0, 2)
             GROUP BY user_id
         ),
         call_stats AS (
             SELECT user_id,
                    COUNT(*)::BIGINT AS calls_24h,
                    COALESCE(SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END), 0)::BIGINT AS failed_calls_24h,
                    COALESCE(SUM(CASE WHEN status_code = 200 THEN cost_points ELSE 0 END), 0) AS spend_24h_sort,
                    COALESCE(CAST(SUM(CASE WHEN status_code = 200 THEN cost_points ELSE 0 END) AS VARCHAR), '0') AS spend_24h,
                    MAX(created_at) AS last_call_at
             FROM api_calls
             WHERE created_at >= NOW() - INTERVAL '24 hours'
             GROUP BY user_id
         )
         SELECT u.id, u.uid, u.email, u.nickname, u.status, u.created_at,
                CAST(COALESCE(b.balance, 0) AS VARCHAR),
                COALESCE(ks.key_count, 0)::BIGINT,
                COALESCE(ks.active_key_count, 0)::BIGINT,
                COALESCE(cs.calls_24h, 0)::BIGINT,
                COALESCE(cs.failed_calls_24h, 0)::BIGINT,
                COALESCE(cs.spend_24h, '0'),
                cs.last_call_at
         FROM overseas_users u
         LEFT JOIN balances b ON b.user_id = u.id
         LEFT JOIN key_stats ks ON ks.user_id = u.id
         LEFT JOIN call_stats cs ON cs.user_id = u.id
         ORDER BY COALESCE(cs.calls_24h, 0) DESC,
                  COALESCE(cs.failed_calls_24h, 0) DESC,
                  COALESCE(cs.spend_24h_sort, 0) DESC,
                  u.created_at DESC
         LIMIT 100",
    )
    .fetch_all(&pool)
    .await;

    match result {
        Ok(rows) => {
            let mut high_count = 0;
            let mut watch_count = 0;
            let mut disabled_count = 0;
            let mut total_calls_24h = 0_i64;
            let mut total_spend_24h = 0.0_f64;
            let mut items = Vec::new();

            for r in rows {
                let status = r.get::<i16, _>(4);
                let balance = r
                    .get::<String, _>(6)
                    .parse::<f64>()
                    .expect("database balance must parse");
                let calls_24h = r.get::<i64, _>(9);
                let failed_calls_24h = r.get::<i64, _>(10);
                let spend_24h = r
                    .get::<String, _>(11)
                    .parse::<f64>()
                    .expect("database spend must parse");
                let fail_rate = if calls_24h > 0 {
                    failed_calls_24h as f64 / calls_24h as f64
                } else {
                    0.0
                };

                let mut reasons = Vec::new();
                let risk_level = if status != 0 {
                    disabled_count += 1;
                    "disabled"
                } else if calls_24h >= 1000
                    || spend_24h >= 50.0
                    || (calls_24h >= 20 && fail_rate >= 0.5)
                {
                    high_count += 1;
                    if calls_24h >= 1000 {
                        reasons.push("high_call_volume");
                    }
                    if spend_24h >= 50.0 {
                        reasons.push("high_spend_24h");
                    }
                    if calls_24h >= 20 && fail_rate >= 0.5 {
                        reasons.push("high_failure_rate");
                    }
                    "high"
                } else if calls_24h >= 300
                    || spend_24h >= 10.0
                    || (calls_24h >= 10 && fail_rate >= 0.25)
                {
                    watch_count += 1;
                    if calls_24h >= 300 {
                        reasons.push("call_volume_watch");
                    }
                    if spend_24h >= 10.0 {
                        reasons.push("spend_watch");
                    }
                    if calls_24h >= 10 && fail_rate >= 0.25 {
                        reasons.push("failure_rate_watch");
                    }
                    "watch"
                } else {
                    "normal"
                };

                total_calls_24h += calls_24h;
                total_spend_24h += spend_24h;

                items.push(json!({
                    "id": r.get::<i64, _>(0),
                    "uid": r.get::<String, _>(1),
                    "email": r.get::<String, _>(2),
                    "nickname": r.get::<String, _>(3),
                    "status": status,
                    "created_at": r.get::<chrono::NaiveDateTime, _>(5),
                    "balance": balance,
                    "key_count": r.get::<i64, _>(7),
                    "active_key_count": r.get::<i64, _>(8),
                    "calls_24h": calls_24h,
                    "failed_calls_24h": failed_calls_24h,
                    "fail_rate": fail_rate,
                    "spend_24h": spend_24h,
                    "last_call_at": r.get::<Option<chrono::NaiveDateTime>, _>(12),
                    "risk_level": risk_level,
                    "risk_reasons": reasons,
                }));
            }

            Json(json!({
                "code": 0,
                "data": {
                    "window": "24h",
                    "total_users": items.len(),
                    "high_count": high_count,
                    "watch_count": watch_count,
                    "disabled_count": disabled_count,
                    "calls_24h": total_calls_24h,
                    "spend_24h": total_spend_24h,
                    "items": items
                }
            }))
        }
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn admin_users(State(pool): State<PgPool>) -> Json<serde_json::Value> {
    use sqlx::Row;
    let result = sqlx::query(
        "SELECT id, uid, email, nickname, status, created_at FROM overseas_users ORDER BY created_at DESC LIMIT 50"
    )
    .fetch_all(&pool)
    .await;

    match result {
        Ok(rows) => {
            let items: Vec<_> = rows
                .iter()
                .map(|r| {
                    json!({
                        "id": r.get::<i64, _>(0),
                        "uid": r.get::<String, _>(1),
                        "email": r.get::<String, _>(2),
                        "nickname": r.get::<String, _>(3),
                        "status": r.get::<i16, _>(4),
                        "created_at": r.get::<chrono::NaiveDateTime, _>(5)
                    })
                })
                .collect();
            Json(json!({"code": 0, "data": {"items": items}}))
        }
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn admin_user_detail(
    Path(id): Path<i64>,
    State(pool): State<PgPool>,
) -> Json<serde_json::Value> {
    use sqlx::Row;
    let result = sqlx::query(
        "SELECT id, uid, email, nickname, status, created_at FROM overseas_users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&pool)
    .await;

    match result {
        Ok(Some(row)) => Json(json!({
            "code": 0,
            "data": {
                "id": row.get::<i64, _>(0),
                "uid": row.get::<String, _>(1),
                "email": row.get::<String, _>(2),
                "nickname": row.get::<String, _>(3),
                "status": row.get::<i16, _>(4),
                "created_at": row.get::<chrono::NaiveDateTime, _>(5)
            }
        })),
        Ok(None) => Json(json!({"code": 404, "message": "User not found"})),
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn disable_user(Path(id): Path<i64>, State(pool): State<PgPool>) -> Json<serde_json::Value> {
    let result =
        sqlx::query("UPDATE overseas_users SET status = 1, updated_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await;

    match result {
        Ok(_) => {
            Json(json!({"code": 0, "message": "User disabled", "data": {"id": id, "status": 1}}))
        }
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn enable_user(Path(id): Path<i64>, State(pool): State<PgPool>) -> Json<serde_json::Value> {
    let result =
        sqlx::query("UPDATE overseas_users SET status = 0, updated_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await;

    match result {
        Ok(_) => {
            Json(json!({"code": 0, "message": "User enabled", "data": {"id": id, "status": 0}}))
        }
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

// Admin - 订单管理 (Stage 3.4)
async fn admin_orders(State(pool): State<PgPool>) -> Json<serde_json::Value> {
    use sqlx::Row;
    let result = sqlx::query(
        "SELECT o.id, o.order_no, CAST(o.amount AS VARCHAR), o.payment_status, o.created_at, u.email as user_email, p.name as package_name
         FROM overseas_orders o
         JOIN overseas_users u ON o.user_id = u.id
         JOIN packages p ON o.package_id = p.id
         ORDER BY o.created_at DESC LIMIT 50"
    )
    .fetch_all(&pool)
    .await;

    match result {
        Ok(rows) => {
            let items: Vec<_> = rows
                .iter()
                .map(|r| {
                    json!({
                        "id": r.get::<i64, _>(0),
                        "order_no": r.get::<String, _>(1),
                        "amount": r.get::<String, _>(2),
                        "payment_status": r.get::<i16, _>(3),
                        "created_at": r.get::<chrono::NaiveDateTime, _>(4),
                        "user_email": r.get::<String, _>(5),
                        "package_name": r.get::<String, _>(6)
                    })
                })
                .collect();
            Json(json!({"code": 0, "data": {"items": items}}))
        }
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

async fn admin_order_detail(
    Path(id): Path<i64>,
    State(pool): State<PgPool>,
) -> Json<serde_json::Value> {
    use sqlx::Row;
    let result = sqlx::query(
        "SELECT o.order_no, CAST(o.amount AS VARCHAR), o.payment_status, o.created_at, o.paid_at, u.email as user_email, p.name as package_name
         FROM overseas_orders o
         JOIN overseas_users u ON o.user_id = u.id
         JOIN packages p ON o.package_id = p.id
         WHERE o.id = $1"
    )
    .bind(id)
    .fetch_optional(&pool)
    .await;

    match result {
        Ok(Some(row)) => Json(json!({
            "code": 0,
            "data": {
                "order_no": row.get::<String, _>(0),
                "amount": row.get::<String, _>(1),
                "payment_status": row.get::<i16, _>(2),
                "created_at": row.get::<chrono::NaiveDateTime, _>(3),
                "paid_at": row.get::<Option<chrono::NaiveDateTime>, _>(4),
                "user_email": row.get::<String, _>(5),
                "package_name": row.get::<String, _>(6)
            }
        })),
        Ok(None) => Json(json!({"code": 404, "message": "Order not found"})),
        Err(e) => Json(json!({"code": 500, "message": sanitize_error(e)})),
    }
}

#[derive(Debug)]
struct PaymentSettlement {
    order_id: i64,
    amount: f64,
}

#[derive(Debug)]
enum SettlementError {
    NotFound,
    AlreadyPaid,
    NotPending,
    AmountMismatch,
    Internal,
}

fn settlement_error_response(error: SettlementError) -> Response {
    match error {
        SettlementError::NotFound => {
            webhook_response(StatusCode::NOT_FOUND, "ORDER_NOT_FOUND", "Order not found")
        }
        SettlementError::AlreadyPaid => webhook_response(
            StatusCode::CONFLICT,
            "ORDER_ALREADY_PAID",
            "Order already paid",
        ),
        SettlementError::NotPending => webhook_response(
            StatusCode::CONFLICT,
            "ORDER_NOT_PENDING",
            "Order is not pending",
        ),
        SettlementError::AmountMismatch => webhook_response(
            StatusCode::CONFLICT,
            "PAYMENT_AMOUNT_MISMATCH",
            "Payment amount does not match the order",
        ),
        SettlementError::Internal => webhook_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "PAYMENT_FAILED",
            "Payment processing failed",
        ),
    }
}

async fn settle_order_payment_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    order_no: &str,
    expected_amount: Option<&str>,
) -> Result<PaymentSettlement, SettlementError> {
    use sqlx::Row;

    let order = sqlx::query(
        "UPDATE overseas_orders
         SET payment_status = 1, paid_at = NOW(), updated_at = NOW()
         WHERE order_no = $1 AND payment_status = 0
           AND ($2::TEXT IS NULL OR actual_amount = $2::NUMERIC)
         RETURNING id, user_id, workspace_id,
                   CAST(actual_amount + COALESCE(bonus_amount, 0) AS VARCHAR)",
    )
    .bind(order_no)
    .bind(expected_amount)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to transition payment order");
        SettlementError::Internal
    })?;

    let Some(order) = order else {
        let status = sqlx::query(
            "SELECT payment_status,
                    ($2::TEXT IS NULL OR actual_amount = $2::NUMERIC) AS amount_matches
             FROM overseas_orders WHERE order_no = $1",
        )
        .bind(order_no)
        .bind(expected_amount)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to inspect payment order");
            SettlementError::Internal
        })?;
        return match status {
            Some(row) if row.get::<i16, _>(0) == 1 => Err(SettlementError::AlreadyPaid),
            Some(row) if !row.get::<bool, _>(1) => Err(SettlementError::AmountMismatch),
            Some(_) => Err(SettlementError::NotPending),
            None => Err(SettlementError::NotFound),
        };
    };

    let order_id: i64 = order.get(0);
    let user_id: i64 = order.get(1);
    let workspace_id: i64 = order.get(2);
    let amount_text: String = order.get(3);
    let amount = amount_text.parse::<f64>().map_err(|_| {
        tracing::error!(order_id, "Failed to parse settled order amount");
        SettlementError::Internal
    })?;

    sqlx::query(
        "INSERT INTO balances (user_id, workspace_id, balance, frozen_balance, total_recharged, total_consumed)
         VALUES ($1, $2, 0, 0, 0, 0)
         ON CONFLICT (workspace_id) DO NOTHING",
    )
    .bind(user_id)
    .bind(workspace_id)
    .execute(&mut **tx)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to initialize user balance");
        SettlementError::Internal
    })?;

    let balance = sqlx::query(
        "UPDATE balances
         SET balance = balance + $1::NUMERIC,
             total_recharged = total_recharged + $1::NUMERIC,
             updated_at = NOW()
         WHERE workspace_id = $2
         RETURNING CAST(balance - $1::NUMERIC AS VARCHAR), CAST(balance AS VARCHAR)",
    )
    .bind(&amount_text)
    .bind(workspace_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to credit user balance");
        SettlementError::Internal
    })?;
    let balance_before: String = balance.get(0);
    let balance_after: String = balance.get(1);

    sqlx::query(
        "INSERT INTO balance_logs
         (user_id, workspace_id, order_id, change_amount, balance_before, balance_after, change_type, note)
         VALUES ($1, $2, $3, $4::NUMERIC, $5::NUMERIC, $6::NUMERIC, 'recharge', 'Order payment credited')",
    )
    .bind(user_id)
    .bind(workspace_id)
    .bind(order_id)
    .bind(&amount_text)
    .bind(&balance_before)
    .bind(&balance_after)
    .execute(&mut **tx)
    .await
    .map_err(|e| {
        tracing::error!(error = %e, "Failed to write recharge balance log");
        SettlementError::Internal
    })?;

    Ok(PaymentSettlement { order_id, amount })
}

async fn settle_order_payment(
    pool: &PgPool,
    order_no: &str,
) -> Result<PaymentSettlement, SettlementError> {
    let mut tx = pool.begin().await.map_err(|e| {
        tracing::error!(error = %e, "Failed to start manual payment transaction");
        SettlementError::Internal
    })?;
    let settlement = settle_order_payment_tx(&mut tx, order_no, None).await?;
    tx.commit().await.map_err(|e| {
        tracing::error!(error = %e, "Failed to commit manual payment transaction");
        SettlementError::Internal
    })?;
    Ok(settlement)
}

async fn confirm_order_payment(Path(id): Path<i64>, State(pool): State<PgPool>) -> Response {
    use sqlx::Row;

    let order = match sqlx::query("SELECT order_no FROM overseas_orders WHERE id = $1")
        .bind(id)
        .fetch_optional(&pool)
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return settlement_error_response(SettlementError::NotFound),
        Err(e) => {
            tracing::error!(error = %e, "Failed to find order for manual confirmation");
            return settlement_error_response(SettlementError::Internal);
        }
    };

    let order_no: String = order.get(0);
    match settle_order_payment(&pool, &order_no).await {
        Ok(settlement) => Json(json!({
            "code": 0,
            "message": "Payment confirmed",
            "data": {
                "id": id,
                "order_no": order_no,
                "payment_status": 1,
                "balance_added": settlement.amount,
                "currency": "USD"
            }
        }))
        .into_response(),
        Err(error) => settlement_error_response(error),
    }
}

async fn refund_order(Path(id): Path<i64>, State(pool): State<PgPool>) -> Response {
    use sqlx::Row;

    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            tracing::error!(error = %e, "Failed to start refund transaction");
            return webhook_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "REFUND_FAILED",
                "Refund failed",
            );
        }
    };
    let order = match sqlx::query(
        "UPDATE overseas_orders
         SET payment_status = 4, updated_at = NOW()
         WHERE id = $1 AND payment_status = 1
         RETURNING user_id, workspace_id,
                   CAST(actual_amount + COALESCE(bonus_amount, 0) AS VARCHAR)",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(Some(order)) => order,
        Ok(None) => {
            let _ = tx.rollback().await;
            return webhook_response(
                StatusCode::CONFLICT,
                "ORDER_NOT_REFUNDABLE",
                "Order not found or cannot be refunded",
            );
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to transition refund order");
            let _ = tx.rollback().await;
            return webhook_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "REFUND_FAILED",
                "Refund failed",
            );
        }
    };
    let user_id: i64 = order.get(0);
    let workspace_id: i64 = order.get(1);
    let amount_text: String = order.get(2);
    let amount = match amount_text.parse::<f64>() {
        Ok(amount) => amount,
        Err(_) => {
            let _ = tx.rollback().await;
            return webhook_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "REFUND_FAILED",
                "Refund failed",
            );
        }
    };
    let balance = match sqlx::query(
        "UPDATE balances
         SET balance = balance - $1::NUMERIC,
             total_recharged = GREATEST(total_recharged - $1::NUMERIC, 0),
             updated_at = NOW()
         WHERE workspace_id = $2 AND balance >= $1::NUMERIC
         RETURNING CAST(balance + $1::NUMERIC AS VARCHAR), CAST(balance AS VARCHAR)",
    )
    .bind(&amount_text)
    .bind(workspace_id)
    .fetch_optional(&mut *tx)
    .await
    {
        Ok(Some(balance)) => balance,
        Ok(None) => {
            let _ = tx.rollback().await;
            return webhook_response(
                StatusCode::CONFLICT,
                "INSUFFICIENT_REFUNDABLE_BALANCE",
                "Current balance is lower than the refundable amount",
            );
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to deduct refunded balance");
            let _ = tx.rollback().await;
            return webhook_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "REFUND_FAILED",
                "Refund failed",
            );
        }
    };
    let balance_before: String = balance.get(0);
    let balance_after: String = balance.get(1);
    if let Err(e) = sqlx::query(
        "INSERT INTO balance_logs
         (user_id, workspace_id, order_id, change_amount, balance_before, balance_after, change_type, note)
         VALUES ($1, $2, $3, -($4::NUMERIC), $5::NUMERIC, $6::NUMERIC, 'refund', 'Order payment refunded')",
    )
    .bind(user_id)
    .bind(workspace_id)
    .bind(id)
    .bind(&amount_text)
    .bind(&balance_before)
    .bind(&balance_after)
    .execute(&mut *tx)
    .await
    {
        tracing::error!(error = %e, "Failed to write refund balance log");
        let _ = tx.rollback().await;
        return webhook_response(StatusCode::INTERNAL_SERVER_ERROR, "REFUND_FAILED", "Refund failed");
    }
    if let Err(e) = tx.commit().await {
        tracing::error!(error = %e, "Failed to commit refund transaction");
        return webhook_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "REFUND_FAILED",
            "Refund failed",
        );
    }

    Json(json!({
        "code": 0,
        "message": "Refund processed",
        "data": {"id": id, "payment_status": 4, "balance_removed": amount, "currency": "USD"}
    }))
    .into_response()
}

// Metrics - 监控端点 (Stage 4.4)
async fn metrics() -> Json<serde_json::Value> {
    let uptime_seconds = SERVER_STARTED_AT
        .get()
        .expect("Server start time must be initialized before serving requests")
        .elapsed()
        .as_secs();

    Json(json!({
        "code": 0,
        "data": {
            "uptime_seconds": uptime_seconds,
            "version": "1.0.0",
            "environment": std::env::var("RUST_ENV").unwrap_or_else(|_| "development".to_string()),
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_cost_uses_usd_per_million_unit() {
        let cost = calculate_token_cost(1_000, 0, 0.4, 1.6);
        assert!((cost - 0.0004).abs() < f64::EPSILON);
    }

    #[test]
    fn token_cost_combines_input_and_output() {
        let cost = calculate_token_cost(2_000, 500, 0.4, 1.6);
        assert!((cost - 0.0016).abs() < f64::EPSILON);
    }

    #[test]
    fn hmac_sha256_matches_standard_vector() {
        let signature = hmac_sha256(b"key", b"The quick brown fox jumps over the lazy dog");
        assert_eq!(
            hex::encode(signature),
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
    }

    #[test]
    fn signature_comparison_rejects_changes() {
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"tampered"));
    }

    #[test]
    fn sse_parser_handles_split_events_and_usage() {
        let mut buffer = b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\ndata: {\"usage\":{\"prompt_tokens\":4,".to_vec();
        let first = take_sse_event(&mut buffer).expect("first event");
        assert!(matches!(
            inspect_openai_sse_event(&first),
            Ok(SseEvent::Data(None))
        ));
        buffer.extend_from_slice(b"\"completion_tokens\":2}}\n\ndata: [DONE]\n\n");
        let usage = take_sse_event(&mut buffer).expect("usage event");
        assert!(matches!(
            inspect_openai_sse_event(&usage),
            Ok(SseEvent::Data(Some((4, 2))))
        ));
        let done = take_sse_event(&mut buffer).expect("done event");
        assert!(matches!(
            inspect_openai_sse_event(&done),
            Ok(SseEvent::Done)
        ));
        assert!(buffer.is_empty());
    }

    #[test]
    fn sse_parser_rejects_invalid_json() {
        let error = inspect_openai_sse_event(b"data: not-json\n\n").unwrap_err();
        assert_eq!(error, "Upstream SSE data is not valid JSON");
    }
}
