use crate::{budget, byok, CurrentUser};
use axum::{
    body::Bytes,
    extract::{Extension, Path, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{sse::Event, sse::KeepAlive, IntoResponse, Json, Response, Sse},
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{Duration as ChronoDuration, Utc};
use futures_util::stream;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::{convert::Infallible, time::Duration};

const REPLICATE_HOST: &str = "api.replicate.com";
const MAX_UPSTREAM_BODY_BYTES: usize = 1024 * 1024;
const WEBHOOK_TIMESTAMP_TOLERANCE_SECONDS: i64 = 300;

#[derive(Clone)]
struct SelectedRoute {
    logical_model_id: i64,
    provider_model_id: i64,
    upstream_version: String,
    provider_id: i64,
    provider_name: String,
    base_url: String,
    routing_strategy: String,
    provider_credential_id: i64,
    output_url_pointer: String,
    api_token: String,
}

struct PredictionState {
    state: &'static str,
    progress: i16,
    result_url: Option<String>,
    error_code: Option<&'static str>,
    retryable: bool,
    terminal: bool,
}

#[derive(Clone)]
struct TaskAccess {
    state: String,
    upstream_task_id: String,
    upstream_get_url: String,
    upstream_cancel_url: String,
    api_token: String,
    output_url_pointer: String,
}

fn api_error(status: StatusCode, message: impl Into<String>, code: &str) -> Response {
    (
        status,
        Json(json!({
            "error": {
                "message": message.into(),
                "type": "async_inference_error",
                "code": code
            }
        })),
    )
        .into_response()
}

fn nexus_headers(
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

fn model_allowed(models_allowed: &Option<Value>, model: &str) -> bool {
    match models_allowed {
        None => true,
        Some(Value::Array(models)) => models.iter().any(|item| item.as_str() == Some(model)),
        _ => false,
    }
}

fn replicate_url(value: &str, expected_path_prefix: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(value).map_err(|_| "Replicate URL is invalid".to_string())?;
    if url.scheme() != "https"
        || url.host_str() != Some(REPLICATE_HOST)
        || url.port_or_known_default() != Some(443)
        || !url.path().starts_with(expected_path_prefix)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Replicate URL is outside the allowed endpoint".to_string());
    }
    Ok(url)
}

fn public_webhook_base() -> Result<String, String> {
    let value = std::env::var("ASYNC_WEBHOOK_PUBLIC_BASE_URL")
        .map_err(|_| "ASYNC_WEBHOOK_PUBLIC_BASE_URL is not configured".to_string())?;
    let trimmed = value.trim_end_matches('/');
    let url = reqwest::Url::parse(trimmed)
        .map_err(|_| "ASYNC_WEBHOOK_PUBLIC_BASE_URL is invalid".to_string())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("ASYNC_WEBHOOK_PUBLIC_BASE_URL must be a public HTTPS URL".to_string());
    }
    Ok(trimmed.to_string())
}

fn task_ttl_hours() -> Result<i64, String> {
    match std::env::var("ASYNC_TASK_TTL_HOURS") {
        Ok(value) => value
            .parse::<i64>()
            .ok()
            .filter(|hours| (1..=720).contains(hours))
            .ok_or_else(|| "ASYNC_TASK_TTL_HOURS must be between 1 and 720".to_string()),
        Err(std::env::VarError::NotPresent) => Ok(168),
        Err(_) => Err("ASYNC_TASK_TTL_HOURS is invalid".to_string()),
    }
}

fn decrypt_credential(
    credential_id: i64,
    ciphertext: &[u8],
    nonce: &[u8],
    encryption_key_id: &str,
) -> Result<String, String> {
    let cipher =
        byok::ByokCipher::from_env().map_err(|_| "BYOK encryption is unavailable".to_string())?;
    if cipher.key_id() != encryption_key_id {
        tracing::error!(credential_id, "BYOK encryption key does not match");
        return Err("BYOK encryption key does not match".to_string());
    }
    cipher
        .decrypt(ciphertext, nonce)
        .map_err(|_| "BYOK credential cannot be decrypted".to_string())
}

async fn select_route(
    pool: &PgPool,
    user: &CurrentUser,
    model: &str,
    task_type: &str,
) -> Result<Option<SelectedRoute>, String> {
    let row = sqlx::query(
        "SELECT lm.id, selected.id, selected.model_id, selected.provider_id,
                p.name, p.base_url, rp.strategy, pc.id, pc.ciphertext, pc.nonce,
                pc.encryption_key_id, selected.async_output_url_pointer
         FROM logical_models lm
         JOIN routing_policies rp ON rp.logical_model_id = lm.id
         JOIN LATERAL (
             SELECT pm.*
             FROM provider_models pm
             JOIN providers candidate ON candidate.id = pm.provider_id
             WHERE pm.logical_model_id = lm.id
               AND pm.protocol = 'replicate_predictions'
               AND pm.status = 0
               AND candidate.status = 0
               AND candidate.provider_id = 'replicate'
               AND candidate.credential_mode = 'byok'
               AND EXISTS (
                   SELECT 1 FROM provider_credentials available_pc
                   WHERE available_pc.workspace_id = $2
                     AND available_pc.provider_id = candidate.id
                     AND available_pc.status = 0
               )
               AND (rp.strategy <> 'fixed' OR pm.id = rp.fixed_provider_model_id)
             ORDER BY pm.route_priority, pm.id
             LIMIT 1
         ) selected ON TRUE
         JOIN providers p ON p.id = selected.provider_id
         JOIN provider_credentials pc
           ON pc.workspace_id = $2 AND pc.provider_id = p.id AND pc.status = 0
         WHERE lm.model_id = $1 AND lm.status = 0 AND lm.output_modalities ? $3",
    )
    .bind(model)
    .bind(user.workspace_id)
    .bind(task_type)
    .fetch_optional(pool)
    .await
    .map_err(|error| {
        tracing::error!(error = %error, "Failed to select asynchronous provider route");
        "Asynchronous route state is unavailable".to_string()
    })?;

    let Some(row) = row else { return Ok(None) };
    let credential_id: i64 = row.get(7);
    let ciphertext: Vec<u8> = row.get(8);
    let nonce: Vec<u8> = row.get(9);
    let key_id: String = row.get(10);
    let api_token = decrypt_credential(credential_id, &ciphertext, &nonce, &key_id)?;
    let base_url: String = row.get(5);
    let parsed = replicate_url(
        &format!("{}/predictions", base_url.trim_end_matches('/')),
        "/v1/predictions",
    )?;
    if parsed.path() != "/v1/predictions" {
        return Err("Replicate provider base URL must be https://api.replicate.com/v1".to_string());
    }

    Ok(Some(SelectedRoute {
        logical_model_id: row.get(0),
        provider_model_id: row.get(1),
        upstream_version: row.get(2),
        provider_id: row.get(3),
        provider_name: row.get(4),
        base_url,
        routing_strategy: row.get(6),
        provider_credential_id: credential_id,
        output_url_pointer: row.get(11),
        api_token,
    }))
}

async fn response_json(response: reqwest::Response) -> Result<Value, String> {
    let status = response.status();
    if !status.is_success() {
        return Err(format!("Replicate returned HTTP {}", status.as_u16()));
    }
    let body = response
        .bytes()
        .await
        .map_err(|_| "Replicate response could not be read".to_string())?;
    if body.len() > MAX_UPSTREAM_BODY_BYTES {
        return Err("Replicate response exceeded the size limit".to_string());
    }
    serde_json::from_slice(&body).map_err(|_| "Replicate response is not valid JSON".to_string())
}

async fn fetch_webhook_secret(
    client: &reqwest::Client,
    route: &SelectedRoute,
) -> Result<String, String> {
    let endpoint = format!(
        "{}/webhooks/default/secret",
        route.base_url.trim_end_matches('/')
    );
    replicate_url(&endpoint, "/v1/webhooks/default/secret")?;
    let response = client
        .get(endpoint)
        .bearer_auth(&route.api_token)
        .send()
        .await
        .map_err(|_| "Replicate webhook secret request failed".to_string())?;
    let body = response_json(response).await?;
    let key = body
        .get("key")
        .and_then(Value::as_str)
        .filter(|value| value.starts_with("whsec_") && value.len() <= 512)
        .ok_or_else(|| "Replicate webhook secret response is invalid".to_string())?;
    Ok(key.to_string())
}

fn output_url(prediction: &Value, pointer: &str) -> Result<String, String> {
    let value = prediction
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| "Configured output URL pointer did not resolve to a string".to_string())?;
    let url =
        reqwest::Url::parse(value).map_err(|_| "Prediction output URL is invalid".to_string())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Prediction output URL must use HTTPS".to_string());
    }
    Ok(value.to_string())
}

fn prediction_state(prediction: &Value, pointer: &str) -> Result<PredictionState, String> {
    let status = prediction
        .get("status")
        .and_then(Value::as_str)
        .ok_or_else(|| "Replicate response is missing status".to_string())?;
    match status {
        "starting" => Ok(PredictionState {
            state: "queued",
            progress: 0,
            result_url: None,
            error_code: None,
            retryable: false,
            terminal: false,
        }),
        "processing" => Ok(PredictionState {
            state: "running",
            progress: 0,
            result_url: None,
            error_code: None,
            retryable: false,
            terminal: false,
        }),
        "succeeded" => Ok(PredictionState {
            state: "succeeded",
            progress: 100,
            result_url: Some(output_url(prediction, pointer)?),
            error_code: None,
            retryable: false,
            terminal: true,
        }),
        "failed" => Ok(PredictionState {
            state: "failed",
            progress: 0,
            result_url: None,
            error_code: Some("UPSTREAM_TASK_FAILED"),
            retryable: false,
            terminal: true,
        }),
        "canceled" => Ok(PredictionState {
            state: "cancelled",
            progress: 0,
            result_url: None,
            error_code: Some("TASK_CANCELLED"),
            retryable: false,
            terminal: true,
        }),
        _ => Err("Replicate response contains an unsupported status".to_string()),
    }
}

fn budget_context(
    user: &CurrentUser,
    request_id: &str,
    model: &str,
    route: &SelectedRoute,
) -> budget::RequestBudgetContext {
    budget::RequestBudgetContext {
        request_id: request_id.to_string(),
        api_key_id: user
            .api_key_id
            .expect("API key middleware must set api_key_id"),
        user_id: user.user_id,
        workspace_id: user.workspace_id,
        project_id: user.project_id,
        logical_model_id: route.logical_model_id,
        provider_model_id: route.provider_model_id,
        provider_id: route.provider_id,
        provider_credential_id: Some(route.provider_credential_id),
        model: model.to_string(),
        routing_strategy: route.routing_strategy.clone(),
        credential_source: "byok".to_string(),
    }
}

async fn complete_context(
    pool: &PgPool,
    context: &budget::RequestBudgetContext,
    status_code: i32,
    error_code: Option<&str>,
    latency_ms: i32,
) -> Result<(), String> {
    budget::complete_request(
        pool,
        context,
        &budget::CompletedRequest {
            input_tokens: 0,
            output_tokens: 0,
            user_cost_usd: 0.0,
            provider_cost_usd: 0.0,
            latency_ms,
            status_code,
            error_message: error_code.map(str::to_string),
            error_code: error_code.map(str::to_string),
            retryable: status_code >= 500,
        },
    )
    .await
}

async fn complete_task_ledger(pool: &PgPool, uid: &str) -> Result<(), String> {
    let row = sqlx::query(
        "SELECT ac.request_id, ac.api_key_id, ac.user_id, ac.workspace_id, ac.project_id,
                ac.logical_model_id, ac.provider_model_id, ac.provider_id,
                ac.provider_credential_id, ac.model_id, ac.routing_strategy,
                ac.credential_source, t.state, t.error_code,
                LEAST(EXTRACT(EPOCH FROM (NOW() - t.created_at)) * 1000, 2147483647)::INT
         FROM async_inference_tasks t
         JOIN api_calls ac ON ac.id = t.api_call_id
         WHERE t.uid = $1 AND t.state IN ('succeeded', 'failed', 'cancelled')",
    )
    .bind(uid)
    .fetch_optional(pool)
    .await
    .map_err(|error| error.to_string())?;
    let Some(row) = row else { return Ok(()) };
    let state: String = row.get(12);
    let status_code = match state.as_str() {
        "succeeded" => 200,
        "cancelled" => 409,
        _ => 502,
    };
    let context = budget::RequestBudgetContext {
        request_id: row.get(0),
        api_key_id: row.get(1),
        user_id: row.get(2),
        workspace_id: row.get(3),
        project_id: row.get(4),
        logical_model_id: row.get(5),
        provider_model_id: row.get(6),
        provider_id: row.get(7),
        provider_credential_id: row.get(8),
        model: row.get(9),
        routing_strategy: row.get(10),
        credential_source: row.get(11),
    };
    let error_code: Option<String> = row.get(13);
    complete_context(
        pool,
        &context,
        status_code,
        error_code.as_deref(),
        row.get(14),
    )
    .await
}

async fn persist_prediction(
    pool: &PgPool,
    uid: &str,
    prediction: &Value,
    pointer: &str,
) -> Result<bool, String> {
    let state = prediction_state(prediction, pointer)?;
    let upstream_id = prediction
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() <= 255)
        .ok_or_else(|| "Replicate response is missing prediction id".to_string())?;
    let changed = sqlx::query(
        "UPDATE async_inference_tasks
         SET state = $1, progress = $2, result_url = $3, error_code = $4,
             retryable = $5, updated_at = NOW(),
             started_at = CASE WHEN $1 = 'running' THEN COALESCE(started_at, NOW()) ELSE started_at END,
             completed_at = CASE WHEN $6 THEN COALESCE(completed_at, NOW()) ELSE NULL END
         WHERE uid = $7 AND upstream_task_id = $8
           AND state NOT IN ('succeeded', 'failed', 'cancelled')",
    )
    .bind(state.state)
    .bind(state.progress)
    .bind(&state.result_url)
    .bind(state.error_code)
    .bind(state.retryable)
    .bind(state.terminal)
    .bind(uid)
    .bind(upstream_id)
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?
    .rows_affected();
    if state.terminal {
        complete_task_ledger(pool, uid).await?;
    }
    Ok(changed == 1)
}

async fn fail_task(pool: &PgPool, uid: &str, error_code: &str) {
    if let Err(error) = sqlx::query(
        "UPDATE async_inference_tasks
         SET state = 'failed', error_code = $2, retryable = TRUE,
             completed_at = NOW(), updated_at = NOW()
         WHERE uid = $1 AND state NOT IN ('succeeded', 'failed', 'cancelled')",
    )
    .bind(uid)
    .bind(error_code)
    .execute(pool)
    .await
    {
        tracing::error!(error = %error, task_uid = uid, "Failed to persist asynchronous task failure");
    }
    if let Err(error) = complete_task_ledger(pool, uid).await {
        tracing::error!(error = %error, task_uid = uid, "Failed to complete asynchronous task ledger");
    }
}

async fn create_task(
    task_type: &'static str,
    pool: PgPool,
    user: CurrentUser,
    input: Value,
) -> Response {
    let request_id = crate::utils::id_generator::generate_request_id();
    let Some(api_key_id) = user.api_key_id else {
        return nexus_headers(
            api_error(
                StatusCode::UNAUTHORIZED,
                "Use X-API-Key for model calls",
                "API_KEY_REQUIRED",
            ),
            &request_id,
            None,
            None,
        );
    };
    let model = match input.get("model").and_then(Value::as_str) {
        Some(value) if !value.is_empty() && value.len() <= 100 => value,
        _ => {
            return nexus_headers(
                api_error(
                    StatusCode::BAD_REQUEST,
                    "model is required",
                    "INVALID_REQUEST",
                ),
                &request_id,
                None,
                None,
            )
        }
    };
    if !model_allowed(&user.models_allowed, model) {
        return nexus_headers(
            api_error(
                StatusCode::FORBIDDEN,
                "Model is not allowed for this API key",
                "MODEL_NOT_ALLOWED",
            ),
            &request_id,
            None,
            None,
        );
    }
    let provider_input = match input.get("input") {
        Some(Value::Object(_)) => &input["input"],
        _ => {
            return nexus_headers(
                api_error(
                    StatusCode::BAD_REQUEST,
                    "input must be an object",
                    "INVALID_REQUEST",
                ),
                &request_id,
                None,
                None,
            )
        }
    };
    let input_bytes = match serde_json::to_vec(provider_input) {
        Ok(value) if value.len() <= MAX_UPSTREAM_BODY_BYTES => value,
        Ok(_) => {
            return nexus_headers(
                api_error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "input exceeds 1 MiB",
                    "INPUT_TOO_LARGE",
                ),
                &request_id,
                None,
                None,
            )
        }
        Err(_) => {
            return nexus_headers(
                api_error(
                    StatusCode::BAD_REQUEST,
                    "input is invalid",
                    "INVALID_REQUEST",
                ),
                &request_id,
                None,
                None,
            )
        }
    };

    let route = match select_route(&pool, &user, model, task_type).await {
        Ok(Some(route)) => route,
        Ok(None) => return nexus_headers(
            api_error(
                StatusCode::NOT_IMPLEMENTED,
                "No enabled Replicate BYOK endpoint is configured for this model and capability",
                "ASYNC_MEDIA_NOT_AVAILABLE",
            ),
            &request_id,
            None,
            None,
        ),
        Err(message) => {
            return nexus_headers(
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    message,
                    "ASYNC_PROVIDER_UNAVAILABLE",
                ),
                &request_id,
                Some("Replicate"),
                None,
            )
        }
    };
    let webhook_base = match public_webhook_base() {
        Ok(value) => value,
        Err(message) => {
            return nexus_headers(
                api_error(
                    StatusCode::NOT_IMPLEMENTED,
                    message,
                    "ASYNC_WEBHOOK_NOT_CONFIGURED",
                ),
                &request_id,
                Some(&route.provider_name),
                Some(&route.routing_strategy),
            )
        }
    };
    let ttl_hours = match task_ttl_hours() {
        Ok(value) => value,
        Err(message) => {
            return nexus_headers(
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    message,
                    "ASYNC_CONFIGURATION_INVALID",
                ),
                &request_id,
                Some(&route.provider_name),
                Some(&route.routing_strategy),
            )
        }
    };
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(value) => value,
        Err(_) => {
            return nexus_headers(
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "HTTP client is unavailable",
                    "ASYNC_PROVIDER_UNAVAILABLE",
                ),
                &request_id,
                Some(&route.provider_name),
                Some(&route.routing_strategy),
            )
        }
    };
    let webhook_secret = match fetch_webhook_secret(&client, &route).await {
        Ok(value) => value,
        Err(message) => {
            return nexus_headers(
                api_error(
                    StatusCode::BAD_GATEWAY,
                    message,
                    "UPSTREAM_WEBHOOK_SECRET_FAILED",
                ),
                &request_id,
                Some(&route.provider_name),
                Some(&route.routing_strategy),
            )
        }
    };
    let cipher = match byok::ByokCipher::from_env() {
        Ok(value) => value,
        Err(_) => {
            return nexus_headers(
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "BYOK encryption is unavailable",
                    "BYOK_DECRYPTION_UNAVAILABLE",
                ),
                &request_id,
                Some(&route.provider_name),
                Some(&route.routing_strategy),
            )
        }
    };
    let encrypted_secret = match cipher.encrypt(&webhook_secret) {
        Ok(value) => value,
        Err(_) => {
            return nexus_headers(
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Webhook secret could not be encrypted",
                    "WEBHOOK_SECRET_ENCRYPTION_FAILED",
                ),
                &request_id,
                Some(&route.provider_name),
                Some(&route.routing_strategy),
            )
        }
    };

    let context = budget_context(&user, &request_id, model, &route);
    if let Err(error) = budget::reserve_request_budget(&pool, &context, 0.0).await {
        return nexus_headers(
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                error.message,
                "BUDGET_STATE_UNAVAILABLE",
            ),
            &request_id,
            Some(&route.provider_name),
            Some(&route.routing_strategy),
        );
    }

    let task_uid = format!("TSK{}", request_id);
    let webhook_url = format!("{}/webhooks/replicate/{}", webhook_base, task_uid);
    let expires_at = Utc::now().naive_utc() + ChronoDuration::hours(ttl_hours);
    let input_hash = hex::encode(Sha256::digest(&input_bytes));
    let mut tx = match pool.begin().await {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(error = %error, "Failed to begin asynchronous task transaction");
            let _ = complete_context(&pool, &context, 503, Some("TASK_STATE_UNAVAILABLE"), 0).await;
            return nexus_headers(
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Task state is unavailable",
                    "TASK_STATE_UNAVAILABLE",
                ),
                &request_id,
                Some(&route.provider_name),
                Some(&route.routing_strategy),
            );
        }
    };
    let inserted = sqlx::query(
        "INSERT INTO async_inference_tasks
         (uid, request_id, workspace_id, project_id, api_key_id, user_id,
          task_type, model_id, provider_model_id, provider_credential_id,
          input_sha256, state, progress, upstream_webhook_url, expires_at,
          webhook_secret_ciphertext, webhook_secret_nonce, webhook_secret_key_id,
          api_call_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                 $11, 'queued', 0, $12, $13, $14, $15, $16,
                 (SELECT id FROM api_calls WHERE request_id = $2))",
    )
    .bind(&task_uid)
    .bind(&request_id)
    .bind(user.workspace_id)
    .bind(user.project_id)
    .bind(api_key_id)
    .bind(user.user_id)
    .bind(task_type)
    .bind(model)
    .bind(route.provider_model_id)
    .bind(route.provider_credential_id)
    .bind(input_hash)
    .bind(&webhook_url)
    .bind(expires_at)
    .bind(encrypted_secret.ciphertext)
    .bind(encrypted_secret.nonce.as_slice())
    .bind(cipher.key_id())
    .execute(&mut *tx)
    .await;
    if let Err(error) = inserted {
        tracing::error!(error = %error, "Failed to create asynchronous task");
        let _ = tx.rollback().await;
        let _ = complete_context(&pool, &context, 503, Some("TASK_STATE_UNAVAILABLE"), 0).await;
        return nexus_headers(
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Task state is unavailable",
                "TASK_STATE_UNAVAILABLE",
            ),
            &request_id,
            Some(&route.provider_name),
            Some(&route.routing_strategy),
        );
    }
    if let Err(error) = sqlx::query(
        "UPDATE spend_reservations
         SET expires_at = $2
         WHERE api_call_id = (SELECT id FROM api_calls WHERE request_id = $1)",
    )
    .bind(&request_id)
    .bind(expires_at)
    .execute(&mut *tx)
    .await
    {
        tracing::error!(error = %error, "Failed to extend asynchronous reservation");
        let _ = tx.rollback().await;
        let _ = complete_context(&pool, &context, 503, Some("TASK_STATE_UNAVAILABLE"), 0).await;
        return nexus_headers(
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Task state is unavailable",
                "TASK_STATE_UNAVAILABLE",
            ),
            &request_id,
            Some(&route.provider_name),
            Some(&route.routing_strategy),
        );
    }
    if let Err(error) = tx.commit().await {
        tracing::error!(error = %error, "Failed to commit asynchronous task");
        let _ = complete_context(&pool, &context, 503, Some("TASK_STATE_UNAVAILABLE"), 0).await;
        return nexus_headers(
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Task state is unavailable",
                "TASK_STATE_UNAVAILABLE",
            ),
            &request_id,
            Some(&route.provider_name),
            Some(&route.routing_strategy),
        );
    }

    let endpoint = format!("{}/predictions", route.base_url.trim_end_matches('/'));
    let prediction_response = client
        .post(endpoint)
        .bearer_auth(&route.api_token)
        .json(&json!({
            "version": route.upstream_version,
            "input": provider_input,
            "webhook": webhook_url,
            "webhook_events_filter": ["start", "completed"]
        }))
        .send()
        .await;
    let prediction = match prediction_response {
        Ok(response) => match response_json(response).await {
            Ok(value) => value,
            Err(message) => {
                fail_task(&pool, &task_uid, "UPSTREAM_CREATE_FAILED").await;
                return nexus_headers(
                    api_error(StatusCode::BAD_GATEWAY, message, "UPSTREAM_CREATE_FAILED"),
                    &request_id,
                    Some(&route.provider_name),
                    Some(&route.routing_strategy),
                );
            }
        },
        Err(_) => {
            fail_task(&pool, &task_uid, "UPSTREAM_CREATE_FAILED").await;
            return nexus_headers(
                api_error(
                    StatusCode::BAD_GATEWAY,
                    "Replicate prediction request failed",
                    "UPSTREAM_CREATE_FAILED",
                ),
                &request_id,
                Some(&route.provider_name),
                Some(&route.routing_strategy),
            );
        }
    };
    let upstream_id = match prediction.get("id").and_then(Value::as_str) {
        Some(value) if !value.is_empty() && value.len() <= 255 => value,
        _ => {
            fail_task(&pool, &task_uid, "MALFORMED_UPSTREAM_RESPONSE").await;
            return nexus_headers(
                api_error(
                    StatusCode::BAD_GATEWAY,
                    "Replicate response is missing prediction id",
                    "MALFORMED_UPSTREAM_RESPONSE",
                ),
                &request_id,
                Some(&route.provider_name),
                Some(&route.routing_strategy),
            );
        }
    };
    let get_url = prediction
        .pointer("/urls/get")
        .and_then(Value::as_str)
        .unwrap_or("");
    let cancel_url = prediction
        .pointer("/urls/cancel")
        .and_then(Value::as_str)
        .unwrap_or("");
    if replicate_url(get_url, "/v1/predictions/").is_err()
        || replicate_url(cancel_url, "/v1/predictions/").is_err()
    {
        fail_task(&pool, &task_uid, "MALFORMED_UPSTREAM_RESPONSE").await;
        return nexus_headers(
            api_error(
                StatusCode::BAD_GATEWAY,
                "Replicate response contains invalid task URLs",
                "MALFORMED_UPSTREAM_RESPONSE",
            ),
            &request_id,
            Some(&route.provider_name),
            Some(&route.routing_strategy),
        );
    }
    if let Err(error) = sqlx::query(
        "UPDATE async_inference_tasks
         SET upstream_task_id = $2, upstream_get_url = $3, upstream_cancel_url = $4,
             updated_at = NOW()
         WHERE uid = $1 AND upstream_task_id IS NULL",
    )
    .bind(&task_uid)
    .bind(upstream_id)
    .bind(get_url)
    .bind(cancel_url)
    .execute(&pool)
    .await
    {
        tracing::error!(error = %error, "Failed to persist Replicate prediction identifiers");
        let _ = client
            .post(cancel_url)
            .bearer_auth(&route.api_token)
            .send()
            .await;
        fail_task(&pool, &task_uid, "TASK_STATE_UNAVAILABLE").await;
        return nexus_headers(
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Task state is unavailable",
                "TASK_STATE_UNAVAILABLE",
            ),
            &request_id,
            Some(&route.provider_name),
            Some(&route.routing_strategy),
        );
    }
    if let Err(message) =
        persist_prediction(&pool, &task_uid, &prediction, &route.output_url_pointer).await
    {
        fail_task(&pool, &task_uid, "MALFORMED_UPSTREAM_RESPONSE").await;
        return nexus_headers(
            api_error(
                StatusCode::BAD_GATEWAY,
                message,
                "MALFORMED_UPSTREAM_RESPONSE",
            ),
            &request_id,
            Some(&route.provider_name),
            Some(&route.routing_strategy),
        );
    }

    nexus_headers(
        (
            StatusCode::CREATED,
            Json(json!({
                "id": task_uid,
                "object": "inference.task",
                "request_id": request_id,
                "type": task_type,
                "model": model,
                "status": prediction_state(&prediction, &route.output_url_pointer).map(|value| value.state).unwrap_or("failed"),
                "expires_at": expires_at
            })),
        )
            .into_response(),
        &request_id,
        Some(&route.provider_name),
        Some(&route.routing_strategy),
    )
}

pub(crate) async fn create_image_task(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
    Json(input): Json<Value>,
) -> Response {
    create_task("image", pool, user, input).await
}

pub(crate) async fn create_audio_task(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
    Json(input): Json<Value>,
) -> Response {
    create_task("audio", pool, user, input).await
}

pub(crate) async fn create_video_task(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
    Json(input): Json<Value>,
) -> Response {
    create_task("video", pool, user, input).await
}

async fn task_access(
    pool: &PgPool,
    uid: &str,
    user: &CurrentUser,
) -> Result<Option<TaskAccess>, String> {
    let row = sqlx::query(
        "SELECT t.state, t.upstream_task_id, t.upstream_get_url, t.upstream_cancel_url,
                pc.id, pc.ciphertext, pc.nonce, pc.encryption_key_id,
                pm.async_output_url_pointer
         FROM async_inference_tasks t
         JOIN provider_credentials pc ON pc.id = t.provider_credential_id
         JOIN provider_models pm ON pm.id = t.provider_model_id
         WHERE t.uid = $1 AND t.workspace_id = $2 AND t.api_key_id = $3",
    )
    .bind(uid)
    .bind(user.workspace_id)
    .bind(user.api_key_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| error.to_string())?;
    let Some(row) = row else { return Ok(None) };
    let credential_id: i64 = row.get(4);
    let ciphertext: Vec<u8> = row.get(5);
    let nonce: Vec<u8> = row.get(6);
    let key_id: String = row.get(7);
    Ok(Some(TaskAccess {
        state: row.get(0),
        upstream_task_id: row.get::<Option<String>, _>(1).unwrap_or_default(),
        upstream_get_url: row.get::<Option<String>, _>(2).unwrap_or_default(),
        upstream_cancel_url: row.get::<Option<String>, _>(3).unwrap_or_default(),
        api_token: decrypt_credential(credential_id, &ciphertext, &nonce, &key_id)?,
        output_url_pointer: row.get(8),
    }))
}

async fn refresh_task(pool: &PgPool, uid: &str, access: &TaskAccess) -> Result<(), String> {
    if matches!(access.state.as_str(), "succeeded" | "failed" | "cancelled") {
        return complete_task_ledger(pool, uid).await;
    }
    replicate_url(&access.upstream_get_url, "/v1/predictions/")?;
    let response = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| "HTTP client is unavailable".to_string())?
        .get(&access.upstream_get_url)
        .bearer_auth(&access.api_token)
        .send()
        .await
        .map_err(|_| "Replicate task query failed".to_string())?;
    let prediction = response_json(response).await?;
    if prediction.get("id").and_then(Value::as_str) != Some(access.upstream_task_id.as_str()) {
        return Err("Replicate task id does not match the stored task".to_string());
    }
    persist_prediction(pool, uid, &prediction, &access.output_url_pointer).await?;
    Ok(())
}

async fn task_snapshot(
    pool: &PgPool,
    uid: &str,
    user: &CurrentUser,
) -> Result<Option<Value>, String> {
    let row = sqlx::query(
        "SELECT uid, request_id, task_type, model_id, state, progress, result_url,
                error_code, retryable, created_at, started_at, completed_at, expires_at
         FROM async_inference_tasks
         WHERE uid = $1 AND workspace_id = $2 AND api_key_id = $3",
    )
    .bind(uid)
    .bind(user.workspace_id)
    .bind(user.api_key_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| error.to_string())?;
    Ok(row.map(|row| {
        json!({
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
        })
    }))
}

pub(crate) async fn get_task(
    Path(uid): Path<String>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Response {
    let request_id = crate::utils::id_generator::generate_request_id();
    let access = match task_access(&pool, &uid, &user).await {
        Ok(Some(value)) => value,
        Ok(None) => {
            return nexus_headers(
                api_error(StatusCode::NOT_FOUND, "Task not found", "TASK_NOT_FOUND"),
                &request_id,
                None,
                None,
            )
        }
        Err(message) => {
            return nexus_headers(
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    message,
                    "TASK_STATE_UNAVAILABLE",
                ),
                &request_id,
                None,
                None,
            )
        }
    };
    if let Err(message) = refresh_task(&pool, &uid, &access).await {
        return nexus_headers(
            api_error(
                StatusCode::BAD_GATEWAY,
                message,
                "UPSTREAM_TASK_QUERY_FAILED",
            ),
            &request_id,
            Some("Replicate"),
            None,
        );
    }
    match task_snapshot(&pool, &uid, &user).await {
        Ok(Some(value)) => nexus_headers(
            Json(value).into_response(),
            &request_id,
            Some("Replicate"),
            None,
        ),
        Ok(None) => nexus_headers(
            api_error(StatusCode::NOT_FOUND, "Task not found", "TASK_NOT_FOUND"),
            &request_id,
            None,
            None,
        ),
        Err(message) => nexus_headers(
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                message,
                "TASK_STATE_UNAVAILABLE",
            ),
            &request_id,
            None,
            None,
        ),
    }
}

pub(crate) async fn cancel_task(
    Path(uid): Path<String>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Response {
    let request_id = crate::utils::id_generator::generate_request_id();
    let access = match task_access(&pool, &uid, &user).await {
        Ok(Some(value)) => value,
        Ok(None) => {
            return nexus_headers(
                api_error(StatusCode::NOT_FOUND, "Task not found", "TASK_NOT_FOUND"),
                &request_id,
                None,
                None,
            )
        }
        Err(message) => {
            return nexus_headers(
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    message,
                    "TASK_STATE_UNAVAILABLE",
                ),
                &request_id,
                None,
                None,
            )
        }
    };
    if matches!(access.state.as_str(), "succeeded" | "failed" | "cancelled") {
        return nexus_headers(
            api_error(
                StatusCode::CONFLICT,
                "Task can no longer be cancelled",
                "TASK_NOT_CANCELLABLE",
            ),
            &request_id,
            Some("Replicate"),
            None,
        );
    }
    if replicate_url(&access.upstream_cancel_url, "/v1/predictions/").is_err() {
        return nexus_headers(
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Stored cancel endpoint is invalid",
                "TASK_STATE_INVALID",
            ),
            &request_id,
            Some("Replicate"),
            None,
        );
    }
    let response = match reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
    {
        Ok(client) => {
            client
                .post(&access.upstream_cancel_url)
                .bearer_auth(&access.api_token)
                .send()
                .await
        }
        Err(_) => {
            return nexus_headers(
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "HTTP client is unavailable",
                    "ASYNC_PROVIDER_UNAVAILABLE",
                ),
                &request_id,
                Some("Replicate"),
                None,
            )
        }
    };
    let prediction = match response {
        Ok(response) => match response_json(response).await {
            Ok(value) => value,
            Err(message) => {
                return nexus_headers(
                    api_error(StatusCode::BAD_GATEWAY, message, "UPSTREAM_CANCEL_FAILED"),
                    &request_id,
                    Some("Replicate"),
                    None,
                )
            }
        },
        Err(_) => {
            return nexus_headers(
                api_error(
                    StatusCode::BAD_GATEWAY,
                    "Replicate cancellation request failed",
                    "UPSTREAM_CANCEL_FAILED",
                ),
                &request_id,
                Some("Replicate"),
                None,
            )
        }
    };
    if prediction.get("id").and_then(Value::as_str) != Some(access.upstream_task_id.as_str()) {
        return nexus_headers(
            api_error(
                StatusCode::BAD_GATEWAY,
                "Replicate task id does not match the stored task",
                "MALFORMED_UPSTREAM_RESPONSE",
            ),
            &request_id,
            Some("Replicate"),
            None,
        );
    }
    if let Err(message) =
        persist_prediction(&pool, &uid, &prediction, &access.output_url_pointer).await
    {
        return nexus_headers(
            api_error(
                StatusCode::BAD_GATEWAY,
                message,
                "MALFORMED_UPSTREAM_RESPONSE",
            ),
            &request_id,
            Some("Replicate"),
            None,
        );
    }
    match task_snapshot(&pool, &uid, &user).await {
        Ok(Some(value)) if value.get("status").and_then(Value::as_str) == Some("cancelled") => {
            nexus_headers(
                Json(value).into_response(),
                &request_id,
                Some("Replicate"),
                None,
            )
        }
        Ok(Some(_)) => nexus_headers(
            api_error(
                StatusCode::CONFLICT,
                "Replicate did not confirm cancellation",
                "TASK_NOT_CANCELLABLE",
            ),
            &request_id,
            Some("Replicate"),
            None,
        ),
        Ok(None) => nexus_headers(
            api_error(StatusCode::NOT_FOUND, "Task not found", "TASK_NOT_FOUND"),
            &request_id,
            None,
            None,
        ),
        Err(message) => nexus_headers(
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                message,
                "TASK_STATE_UNAVAILABLE",
            ),
            &request_id,
            None,
            None,
        ),
    }
}

pub(crate) async fn task_events(
    Path(uid): Path<String>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Response {
    if !matches!(task_snapshot(&pool, &uid, &user).await, Ok(Some(_))) {
        return api_error(StatusCode::NOT_FOUND, "Task not found", "TASK_NOT_FOUND");
    }
    let stream = stream::unfold(
        (pool, uid, user, false, false),
        |(pool, uid, user, first, done)| async move {
            if done {
                return None;
            }
            if first {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            let snapshot = task_snapshot(&pool, &uid, &user).await;
            let (event, terminal) = match snapshot {
                Ok(Some(value)) => {
                    let terminal = matches!(
                        value.get("status").and_then(Value::as_str),
                        Some("succeeded" | "failed" | "cancelled")
                    );
                    let data = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
                    (Event::default().event("task.status").data(data), terminal)
                }
                Ok(None) => (
                    Event::default()
                        .event("task.error")
                        .data("{\"code\":\"TASK_NOT_FOUND\"}"),
                    true,
                ),
                Err(_) => (
                    Event::default()
                        .event("task.error")
                        .data("{\"code\":\"TASK_STATE_UNAVAILABLE\"}"),
                    true,
                ),
            };
            Some((
                Ok::<Event, Infallible>(event),
                (pool, uid, user, true, terminal),
            ))
        },
    );
    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keep-alive"),
        )
        .into_response()
}

fn hmac_sha256(secret: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut key = [0_u8; BLOCK];
    if secret.len() > BLOCK {
        key[..32].copy_from_slice(&Sha256::digest(secret));
    } else {
        key[..secret.len()].copy_from_slice(secret);
    }
    let mut inner_key = [0x36_u8; BLOCK];
    let mut outer_key = [0x5c_u8; BLOCK];
    for index in 0..BLOCK {
        inner_key[index] ^= key[index];
        outer_key[index] ^= key[index];
    }
    let mut inner = Sha256::new();
    inner.update(inner_key);
    inner.update(message);
    let inner_hash = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_key);
    outer.update(inner_hash);
    outer.finalize().into()
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0_u8;
    for (a, b) in left.iter().zip(right) {
        difference |= a ^ b;
    }
    difference == 0
}

fn verify_standard_webhook(
    headers: &HeaderMap,
    body: &[u8],
    secret: &str,
) -> Result<String, String> {
    let event_id = headers
        .get("webhook-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= 255)
        .ok_or_else(|| "webhook-id is missing".to_string())?;
    let timestamp_text = headers
        .get("webhook-timestamp")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| "webhook-timestamp is missing".to_string())?;
    let timestamp = timestamp_text
        .parse::<i64>()
        .map_err(|_| "webhook-timestamp is invalid".to_string())?;
    if (Utc::now().timestamp() - timestamp).abs() > WEBHOOK_TIMESTAMP_TOLERANCE_SECONDS {
        return Err("webhook timestamp is outside the accepted window".to_string());
    }
    let signature_header = headers
        .get("webhook-signature")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| "webhook-signature is missing".to_string())?;
    let encoded_secret = secret
        .strip_prefix("whsec_")
        .ok_or_else(|| "webhook secret format is invalid".to_string())?;
    let secret_bytes = STANDARD
        .decode(encoded_secret)
        .map_err(|_| "webhook secret encoding is invalid".to_string())?;
    let mut signed = format!("{}.{}.", event_id, timestamp_text).into_bytes();
    signed.extend_from_slice(body);
    let expected = STANDARD.encode(hmac_sha256(&secret_bytes, &signed));
    let valid = signature_header.split_whitespace().any(|candidate| {
        candidate
            .strip_prefix("v1,")
            .is_some_and(|provided| constant_time_eq(provided.as_bytes(), expected.as_bytes()))
    });
    if !valid {
        return Err("webhook signature is invalid".to_string());
    }
    Ok(event_id.to_string())
}

pub(crate) async fn replicate_webhook(
    Path(uid): Path<String>,
    State(pool): State<PgPool>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if body.len() > MAX_UPSTREAM_BODY_BYTES {
        return api_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Webhook body exceeds 1 MiB",
            "WEBHOOK_TOO_LARGE",
        );
    }
    let task = sqlx::query(
        "SELECT t.id, t.upstream_task_id, t.webhook_secret_ciphertext,
                t.webhook_secret_nonce, t.webhook_secret_key_id,
                pm.async_output_url_pointer
         FROM async_inference_tasks t
         JOIN provider_models pm ON pm.id = t.provider_model_id
         WHERE t.uid = $1",
    )
    .bind(&uid)
    .fetch_optional(&pool)
    .await;
    let row = match task {
        Ok(Some(value)) => value,
        Ok(None) => return api_error(StatusCode::NOT_FOUND, "Task not found", "TASK_NOT_FOUND"),
        Err(error) => {
            tracing::error!(error = %error, "Failed to load webhook task");
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Task state is unavailable",
                "TASK_STATE_UNAVAILABLE",
            );
        }
    };
    let ciphertext: Vec<u8> = row.get(2);
    let nonce: Vec<u8> = row.get(3);
    let key_id: String = row.get(4);
    let cipher = match byok::ByokCipher::from_env() {
        Ok(value) if value.key_id() == key_id => value,
        _ => {
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Webhook verification is unavailable",
                "WEBHOOK_VERIFICATION_UNAVAILABLE",
            )
        }
    };
    let secret = match cipher.decrypt(&ciphertext, &nonce) {
        Ok(value) => value,
        Err(_) => {
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Webhook verification is unavailable",
                "WEBHOOK_VERIFICATION_UNAVAILABLE",
            )
        }
    };
    let event_id = match verify_standard_webhook(&headers, &body, &secret) {
        Ok(value) => value,
        Err(message) => {
            return api_error(
                StatusCode::UNAUTHORIZED,
                message,
                "INVALID_WEBHOOK_SIGNATURE",
            )
        }
    };
    let prediction: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return api_error(
                StatusCode::BAD_REQUEST,
                "Webhook body is not valid JSON",
                "INVALID_WEBHOOK_BODY",
            )
        }
    };
    let prediction_id = prediction.get("id").and_then(Value::as_str).unwrap_or("");
    let stored_id: Option<String> = row.get(1);
    if prediction_id.is_empty()
        || stored_id
            .as_deref()
            .is_some_and(|value| value != prediction_id)
    {
        return api_error(
            StatusCode::BAD_REQUEST,
            "Webhook prediction id does not match task",
            "WEBHOOK_TASK_MISMATCH",
        );
    }
    let inserted = sqlx::query(
        "INSERT INTO async_webhook_events (event_id, task_id) VALUES ($1, $2)
         ON CONFLICT (event_id) DO NOTHING",
    )
    .bind(&event_id)
    .bind(row.get::<i64, _>(0))
    .execute(&pool)
    .await;
    match inserted {
        Ok(result) if result.rows_affected() == 0 => {
            if let Err(error) = complete_task_ledger(&pool, &uid).await {
                tracing::error!(error = %error, task_uid = uid, "Failed to repair task ledger after webhook replay");
                return api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Task ledger is unavailable",
                    "TASK_LEDGER_UNAVAILABLE",
                );
            }
            return Json(json!({"received": true, "duplicate": true})).into_response();
        }
        Ok(_) => {}
        Err(error) => {
            tracing::error!(error = %error, "Failed to record webhook event");
            return api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Webhook state is unavailable",
                "WEBHOOK_STATE_UNAVAILABLE",
            );
        }
    }
    let pointer: String = row.get(5);
    match persist_prediction(&pool, &uid, &prediction, &pointer).await {
        Ok(_) => Json(json!({"received": true})).into_response(),
        Err(message) => api_error(StatusCode::BAD_REQUEST, message, "INVALID_WEBHOOK_BODY"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replicate_url_rejects_untrusted_hosts() {
        assert!(replicate_url(
            "https://api.replicate.com/v1/predictions/abc",
            "/v1/predictions/"
        )
        .is_ok());
        assert!(
            replicate_url("https://example.com/v1/predictions/abc", "/v1/predictions/").is_err()
        );
        assert!(replicate_url(
            "http://api.replicate.com/v1/predictions/abc",
            "/v1/predictions/"
        )
        .is_err());
    }

    #[test]
    fn output_pointer_is_exact_and_requires_https() {
        let prediction = json!({"output": ["https://cdn.example.com/result.png"]});
        assert_eq!(
            output_url(&prediction, "/output/0").unwrap(),
            "https://cdn.example.com/result.png"
        );
        assert!(output_url(&prediction, "/output").is_err());
        assert!(output_url(&json!({"output":"data:image/png;base64,AA"}), "/output").is_err());
    }

    #[test]
    fn standard_webhook_signature_is_verified() {
        let secret_bytes = b"01234567890123456789012345678901";
        let secret = format!("whsec_{}", STANDARD.encode(secret_bytes));
        let body = br#"{"id":"prediction"}"#;
        let timestamp = Utc::now().timestamp().to_string();
        let event_id = "evt_test";
        let mut signed = format!("{}.{}.", event_id, timestamp).into_bytes();
        signed.extend_from_slice(body);
        let signature = STANDARD.encode(hmac_sha256(secret_bytes, &signed));
        let mut headers = HeaderMap::new();
        headers.insert("webhook-id", HeaderValue::from_static("evt_test"));
        headers.insert(
            "webhook-timestamp",
            HeaderValue::from_str(&timestamp).unwrap(),
        );
        headers.insert(
            "webhook-signature",
            HeaderValue::from_str(&format!("v1,{signature}")).unwrap(),
        );
        assert_eq!(
            verify_standard_webhook(&headers, body, &secret).unwrap(),
            event_id
        );
        assert!(verify_standard_webhook(&headers, br#"{"id":"tampered"}"#, &secret).is_err());
    }

    #[test]
    fn prediction_status_does_not_invent_progress() {
        let running = prediction_state(&json!({"status":"processing"}), "/output").unwrap();
        assert_eq!(running.state, "running");
        assert_eq!(running.progress, 0);
        let succeeded = prediction_state(
            &json!({"status":"succeeded","output":"https://cdn.example.com/a.png"}),
            "/output",
        )
        .unwrap();
        assert_eq!(succeeded.progress, 100);
        assert_eq!(
            succeeded.result_url.as_deref(),
            Some("https://cdn.example.com/a.png")
        );
    }
}
