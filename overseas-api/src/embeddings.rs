use axum::{
    extract::{Extension, State},
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};
use std::time::{Duration, Instant};

use crate::{budget, byok, CurrentUser, GatewayLogContext, RateLimitState};

const MAX_EMBEDDING_BATCH_ITEMS: usize = 2_048;
const MAX_EMBEDDING_TOTAL_TOKENS: usize = 300_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InputBounds {
    item_count: usize,
    maximum_tokens: usize,
}

#[derive(Debug)]
struct EmbeddingRoute {
    logical_model_id: i64,
    logical_model: String,
    provider_model_id: i64,
    upstream_model: String,
    provider_id: i64,
    provider_name: String,
    provider_code: String,
    base_url: String,
    api_key_env: String,
    credential_mode: String,
    provider_credential_id: Option<i64>,
    routing_strategy: String,
    context_len: i32,
    input_rate: f64,
    upstream_input_rate: f64,
}

fn invalid_request(message: impl Into<String>, code: &'static str) -> Response {
    crate::api_error_response(
        StatusCode::BAD_REQUEST,
        message,
        "invalid_request_error",
        code,
    )
}

fn decorate(
    response: Response,
    request_id: &str,
    route: Option<&EmbeddingRoute>,
    rate_limit: Option<&RateLimitState>,
) -> Response {
    let response = crate::with_rate_limit_headers(response, rate_limit, false);
    crate::with_nexus_headers(
        response,
        request_id,
        route.map(|route| route.provider_name.as_str()),
        route.map(|route| route.routing_strategy.as_str()),
    )
}

fn token_id(value: &Value) -> bool {
    value.as_u64().is_some_and(|token| token <= u32::MAX as u64)
}

fn validate_embedding_input(
    input: Option<&Value>,
    context_len: usize,
) -> Result<InputBounds, String> {
    let input = input.ok_or_else(|| "input is required".to_string())?;
    match input {
        Value::String(text) => {
            if text.is_empty() {
                return Err("input cannot be an empty string".to_string());
            }
            Ok(InputBounds {
                item_count: 1,
                maximum_tokens: context_len.min(MAX_EMBEDDING_TOTAL_TOKENS),
            })
        }
        Value::Array(items) if items.is_empty() => Err("input cannot be empty".to_string()),
        Value::Array(items) if items.iter().all(token_id) => {
            if items.len() > context_len {
                return Err(format!(
                    "a token-array input cannot exceed {context_len} tokens"
                ));
            }
            Ok(InputBounds {
                item_count: 1,
                maximum_tokens: items.len(),
            })
        }
        Value::Array(items) if items.iter().all(Value::is_string) => {
            if items.len() > MAX_EMBEDDING_BATCH_ITEMS {
                return Err(format!(
                    "input cannot contain more than {MAX_EMBEDDING_BATCH_ITEMS} items"
                ));
            }
            if items
                .iter()
                .any(|item| item.as_str().is_some_and(str::is_empty))
            {
                return Err("input cannot contain an empty string".to_string());
            }
            Ok(InputBounds {
                item_count: items.len(),
                maximum_tokens: items
                    .len()
                    .saturating_mul(context_len)
                    .min(MAX_EMBEDDING_TOTAL_TOKENS),
            })
        }
        Value::Array(items) if items.iter().all(Value::is_array) => {
            if items.len() > MAX_EMBEDDING_BATCH_ITEMS {
                return Err(format!(
                    "input cannot contain more than {MAX_EMBEDDING_BATCH_ITEMS} items"
                ));
            }
            let mut total_tokens = 0usize;
            for item in items {
                let tokens = item.as_array().expect("array shape checked above");
                if tokens.is_empty() {
                    return Err("input cannot contain an empty token array".to_string());
                }
                if !tokens.iter().all(token_id) {
                    return Err("token arrays must contain only non-negative integers".to_string());
                }
                if tokens.len() > context_len {
                    return Err(format!(
                        "a token-array input cannot exceed {context_len} tokens"
                    ));
                }
                total_tokens = total_tokens.saturating_add(tokens.len());
            }
            if total_tokens > MAX_EMBEDDING_TOTAL_TOKENS {
                return Err(format!(
                    "all inputs combined cannot exceed {MAX_EMBEDDING_TOTAL_TOKENS} tokens"
                ));
            }
            Ok(InputBounds {
                item_count: items.len(),
                maximum_tokens: total_tokens,
            })
        }
        Value::Array(_) => Err(
            "input must be a string, an array of strings, a token array, or an array of token arrays"
                .to_string(),
        ),
        _ => Err(
            "input must be a string, an array of strings, a token array, or an array of token arrays"
                .to_string(),
        ),
    }
}

fn validate_embedding_request(input: &Value, context_len: usize) -> Result<InputBounds, String> {
    let object = input
        .as_object()
        .ok_or_else(|| "request body must be a JSON object".to_string())?;
    const SUPPORTED_FIELDS: [&str; 5] = ["model", "input", "encoding_format", "dimensions", "user"];
    if let Some(field) = object
        .keys()
        .find(|field| !SUPPORTED_FIELDS.contains(&field.as_str()))
    {
        return Err(format!("unsupported parameter: {field}"));
    }
    if let Some(format) = object.get("encoding_format") {
        if !matches!(format.as_str(), Some("float" | "base64")) {
            return Err("encoding_format must be either float or base64".to_string());
        }
    }
    if let Some(dimensions) = object.get("dimensions") {
        if !dimensions.as_u64().is_some_and(|value| value > 0) {
            return Err("dimensions must be a positive integer".to_string());
        }
    }
    if let Some(user) = object.get("user") {
        if !user.is_string() {
            return Err("user must be a string".to_string());
        }
    }
    validate_embedding_input(object.get("input"), context_len)
}

fn parse_price(value: String, field: &str) -> Result<f64, String> {
    value
        .parse::<f64>()
        .map_err(|_| format!("invalid {field} in database"))
}

async fn select_route(
    pool: &PgPool,
    workspace_id: i64,
    model: &str,
) -> Result<Option<EmbeddingRoute>, String> {
    let row = sqlx::query(
        "SELECT lm.id, lm.model_id, CAST(lm.input_price_per_million AS VARCHAR),
                lm.context_len, selected.id, selected.model_id, selected.provider_id,
                CAST(selected.upstream_input_price_per_million AS VARCHAR),
                p.name, COALESCE(p.base_url, ''), COALESCE(p.api_key_env, ''),
                p.provider_id, p.credential_mode, rp.strategy,
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
               AND pm.protocol = 'openai_embeddings'
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
               CASE WHEN rp.strategy = 'lowest_price' THEN pm.upstream_input_price_per_million END ASC NULLS LAST,
               CASE WHEN rp.strategy = 'lowest_latency' AND stats.samples >= rp.minimum_samples THEN stats.avg_latency END ASC NULLS LAST,
               CASE WHEN rp.strategy = 'highest_stability' AND stats.samples >= rp.minimum_samples THEN stats.stability END DESC NULLS LAST,
               pm.route_priority, pm.id
             LIMIT 1
         ) selected ON TRUE
         JOIN providers p ON p.id = selected.provider_id
         WHERE lm.model_id = $1 AND lm.status = 0",
    )
    .bind(model)
    .bind(workspace_id)
    .fetch_optional(pool)
    .await
    .map_err(|error| error.to_string())?;

    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some(EmbeddingRoute {
        logical_model_id: row.get(0),
        logical_model: row.get(1),
        input_rate: parse_price(row.get(2), "logical input price")?,
        context_len: row.get(3),
        provider_model_id: row.get(4),
        upstream_model: row.get(5),
        provider_id: row.get(6),
        upstream_input_rate: parse_price(row.get(7), "upstream input price")?,
        provider_name: row.get(8),
        base_url: row.get(9),
        api_key_env: row.get(10),
        provider_code: row.get(11),
        credential_mode: row.get(12),
        routing_strategy: row.get(13),
        provider_credential_id: row.get(14),
    }))
}

async fn embedding_model_exists(pool: &PgPool, model: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(
             SELECT 1
             FROM logical_models lm
             JOIN provider_models pm ON pm.logical_model_id = lm.id
             WHERE lm.model_id = $1 AND lm.status = 0
               AND pm.protocol = 'openai_embeddings'
         )",
    )
    .bind(model)
    .fetch_one(pool)
    .await
}

async fn load_provider_key(
    pool: &PgPool,
    route: &EmbeddingRoute,
    workspace_id: i64,
) -> Result<String, Response> {
    match route.credential_mode.as_str() {
        "platform_authorized" => {
            let key = crate::config::provider_api_key(&route.api_key_env);
            if key.is_empty() {
                Err(crate::api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Provider key is not configured",
                    "provider_not_configured",
                    "PROVIDER_NOT_CONFIGURED",
                ))
            } else {
                Ok(key)
            }
        }
        "byok" => {
            let credential_id = route.provider_credential_id.ok_or_else(|| {
                crate::api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "No active BYOK credential is configured for this provider",
                    "byok_credential_unavailable",
                    "BYOK_CREDENTIAL_UNAVAILABLE",
                )
            })?;
            let credential = sqlx::query(
                "SELECT ciphertext, nonce, encryption_key_id
                 FROM provider_credentials
                 WHERE id = $1 AND workspace_id = $2 AND provider_id = $3 AND status = 0",
            )
            .bind(credential_id)
            .bind(workspace_id)
            .bind(route.provider_id)
            .fetch_optional(pool)
            .await
            .map_err(|error| {
                tracing::error!(error = %error, "Failed to load embedding BYOK credential");
                crate::api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "BYOK credential is unavailable",
                    "byok_credential_unavailable",
                    "BYOK_CREDENTIAL_UNAVAILABLE",
                )
            })?
            .ok_or_else(|| {
                crate::api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "No active BYOK credential is configured for this provider",
                    "byok_credential_unavailable",
                    "BYOK_CREDENTIAL_UNAVAILABLE",
                )
            })?;
            let ciphertext: Vec<u8> = credential.get(0);
            let nonce: Vec<u8> = credential.get(1);
            let encryption_key_id: String = credential.get(2);
            let cipher = byok::ByokCipher::from_env().map_err(|error| {
                tracing::error!(error = %error, "Embedding BYOK master key is unavailable");
                crate::api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "BYOK credential cannot be decrypted",
                    "byok_decryption_unavailable",
                    "BYOK_DECRYPTION_UNAVAILABLE",
                )
            })?;
            if cipher.key_id() != encryption_key_id {
                return Err(crate::api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "BYOK credential cannot be decrypted",
                    "byok_decryption_unavailable",
                    "BYOK_DECRYPTION_UNAVAILABLE",
                ));
            }
            let key = cipher.decrypt(&ciphertext, &nonce).map_err(|error| {
                tracing::error!(error = %error, credential_id, "Failed to decrypt embedding BYOK credential");
                crate::api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "BYOK credential cannot be decrypted",
                    "byok_decryption_unavailable",
                    "BYOK_DECRYPTION_UNAVAILABLE",
                )
            })?;
            let changed = sqlx::query(
                "UPDATE provider_credentials SET last_used_at = NOW(), updated_at = NOW()
                 WHERE id = $1 AND workspace_id = $2 AND status = 0",
            )
            .bind(credential_id)
            .bind(workspace_id)
            .execute(pool)
            .await
            .map_err(|error| {
                tracing::error!(error = %error, credential_id, "Failed to update embedding BYOK credential usage");
                crate::api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "BYOK credential state could not be updated",
                    "byok_credential_unavailable",
                    "BYOK_CREDENTIAL_UNAVAILABLE",
                )
            })?
            .rows_affected();
            if changed != 1 {
                return Err(crate::api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "BYOK credential state could not be updated",
                    "byok_credential_unavailable",
                    "BYOK_CREDENTIAL_UNAVAILABLE",
                ));
            }
            Ok(key)
        }
        _ => Err(crate::api_error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "Provider credential mode is invalid",
            "provider_configuration_invalid",
            "PROVIDER_CONFIGURATION_INVALID",
        )),
    }
}

fn valid_embedding_response(value: &Value, expected_items: usize) -> Option<i32> {
    if value.get("object").and_then(Value::as_str) != Some("list") {
        return None;
    }
    let data = value.get("data")?.as_array()?;
    if data.len() != expected_items {
        return None;
    }
    for (position, item) in data.iter().enumerate() {
        if item.get("object").and_then(Value::as_str) != Some("embedding")
            || item.get("index").and_then(Value::as_u64) != Some(position as u64)
        {
            return None;
        }
        let embedding = item.get("embedding")?;
        let valid_float = embedding
            .as_array()
            .is_some_and(|values| !values.is_empty() && values.iter().all(Value::is_number));
        let valid_base64 = embedding.as_str().is_some_and(|value| !value.is_empty());
        if !valid_float && !valid_base64 {
            return None;
        }
    }
    let usage = value.get("usage")?;
    let prompt_tokens = usage
        .get("prompt_tokens")?
        .as_u64()
        .and_then(|value| i32::try_from(value).ok())?;
    let total_tokens = usage.get("total_tokens")?.as_u64()?;
    if total_tokens < prompt_tokens as u64 {
        return None;
    }
    Some(prompt_tokens)
}

async fn record(
    pool: &PgPool,
    context: &GatewayLogContext,
    user: &CurrentUser,
    route: &EmbeddingRoute,
    input_tokens: i32,
    cost: f64,
    latency_ms: i32,
    status: StatusCode,
    error: &str,
) -> bool {
    crate::log_api_call(
        pool,
        context,
        user.api_key_id,
        user.user_id,
        &route.logical_model,
        Some(route.provider_model_id),
        route.provider_id,
        input_tokens,
        0,
        cost,
        latency_ms,
        status.as_u16() as i32,
        error,
    )
    .await
}

pub(crate) async fn proxy(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
    Json(mut input): Json<Value>,
) -> Response {
    let request_id = crate::utils::id_generator::generate_request_id();
    let start_time = Instant::now();
    if user.api_key_id.is_none() {
        return decorate(
            crate::api_error_response(
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
    let rate_limit = match crate::enforce_rate_limit(&user).await {
        Ok(state) => state,
        Err(response) => return decorate(response, &request_id, None, None),
    };
    let model = match input.get("model").and_then(Value::as_str) {
        Some(model) if !model.is_empty() => model.to_string(),
        _ => {
            return decorate(
                invalid_request("model is required", "INVALID_MODEL"),
                &request_id,
                None,
                rate_limit.as_ref(),
            )
        }
    };
    let route = match select_route(&pool, user.workspace_id, &model).await {
        Ok(Some(route)) => route,
        Ok(None) => {
            let exists = embedding_model_exists(&pool, &model).await;
            let response = match exists {
                Ok(false) => crate::api_error_response(
                    StatusCode::BAD_REQUEST,
                    format!("Unsupported embedding model: {model}"),
                    "unsupported_model",
                    "UNSUPPORTED_MODEL",
                ),
                Ok(true) => crate::api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "No healthy embedding provider endpoint is available for this workspace",
                    "provider_unavailable",
                    "NO_AVAILABLE_PROVIDER_ENDPOINT",
                ),
                Err(error) => {
                    tracing::error!(error = %error, "Failed to inspect embedding model routes");
                    crate::api_error_response(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Embedding route state is unavailable",
                        "route_state_unavailable",
                        "ROUTE_STATE_UNAVAILABLE",
                    )
                }
            };
            return decorate(response, &request_id, None, rate_limit.as_ref());
        }
        Err(error) => {
            tracing::error!(error = %error, "Failed to select embedding route");
            return decorate(
                crate::api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Embedding route state is unavailable",
                    "route_state_unavailable",
                    "ROUTE_STATE_UNAVAILABLE",
                ),
                &request_id,
                None,
                rate_limit.as_ref(),
            );
        }
    };
    let bounds = match validate_embedding_request(&input, route.context_len as usize) {
        Ok(bounds) => bounds,
        Err(message) => {
            return decorate(
                invalid_request(message, "INVALID_EMBEDDING_REQUEST"),
                &request_id,
                Some(&route),
                rate_limit.as_ref(),
            )
        }
    };
    if !crate::is_model_allowed_for_key(&user.models_allowed, &model) {
        let message = format!("Model {model} is not allowed for this API key");
        let context = GatewayLogContext {
            request_id: request_id.clone(),
            workspace_id: user.workspace_id,
            project_id: user.project_id,
            logical_model_id: route.logical_model_id,
            provider_model_id: route.provider_model_id,
            provider_credential_id: route.provider_credential_id,
            routing_strategy: route.routing_strategy.clone(),
            credential_source: route.credential_mode.clone(),
            upstream_input_rate: 0.0,
            upstream_output_rate: 0.0,
        };
        record(
            &pool,
            &context,
            &user,
            &route,
            0,
            0.0,
            start_time.elapsed().as_millis() as i32,
            StatusCode::FORBIDDEN,
            &message,
        )
        .await;
        return decorate(
            crate::api_error_response(
                StatusCode::FORBIDDEN,
                message,
                "model_not_allowed",
                "MODEL_NOT_ALLOWED",
            ),
            &request_id,
            Some(&route),
            rate_limit.as_ref(),
        );
    }
    if let Ok(Some(reason)) = crate::provider_circuit_open(&pool, route.provider_id).await {
        return decorate(
            crate::api_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                reason,
                "provider_circuit_open",
                "PROVIDER_CIRCUIT_OPEN",
            ),
            &request_id,
            Some(&route),
            rate_limit.as_ref(),
        );
    }
    if !crate::provider_base_url_is_allowed(&route.provider_code, &route.base_url) {
        return decorate(
            crate::api_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "Provider endpoint is not allowed",
                "provider_endpoint_rejected",
                "PROVIDER_ENDPOINT_REJECTED",
            ),
            &request_id,
            Some(&route),
            rate_limit.as_ref(),
        );
    }
    let provider_key = match load_provider_key(&pool, &route, user.workspace_id).await {
        Ok(key) => key,
        Err(response) => return decorate(response, &request_id, Some(&route), rate_limit.as_ref()),
    };

    let byok = route.credential_mode == "byok";
    let billable_input_rate = if byok { 0.0 } else { route.input_rate };
    let logged_upstream_input_rate = if byok { 0.0 } else { route.upstream_input_rate };
    let context = GatewayLogContext {
        request_id: request_id.clone(),
        workspace_id: user.workspace_id,
        project_id: user.project_id,
        logical_model_id: route.logical_model_id,
        provider_model_id: route.provider_model_id,
        provider_credential_id: route.provider_credential_id,
        routing_strategy: route.routing_strategy.clone(),
        credential_source: if byok { "byok" } else { "platform" }.to_string(),
        upstream_input_rate: logged_upstream_input_rate,
        upstream_output_rate: 0.0,
    };
    let maximum_cost =
        crate::calculate_token_cost(bounds.maximum_tokens as i32, 0, billable_input_rate, 0.0);
    let budget_context = context.budget_context(
        user.api_key_id.expect("API key checked above"),
        user.user_id,
        &model,
        route.provider_id,
    );
    if let Err(error) = budget::reserve_request_budget(&pool, &budget_context, maximum_cost).await {
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
        record(
            &pool,
            &context,
            &user,
            &route,
            0,
            0.0,
            start_time.elapsed().as_millis() as i32,
            status,
            &error.message,
        )
        .await;
        return decorate(
            crate::api_error_response(status, error.message, error_type, code),
            &request_id,
            Some(&route),
            rate_limit.as_ref(),
        );
    }

    input["model"] = json!(route.upstream_model);
    let endpoint = format!("{}/embeddings", route.base_url.trim_end_matches('/'));
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            tracing::error!(error = %error, "Failed to build embedding HTTP client");
            record(
                &pool,
                &context,
                &user,
                &route,
                0,
                0.0,
                start_time.elapsed().as_millis() as i32,
                StatusCode::SERVICE_UNAVAILABLE,
                "Embedding HTTP client is unavailable",
            )
            .await;
            return decorate(
                crate::api_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Embedding upstream client is unavailable",
                    "upstream_client_unavailable",
                    "UPSTREAM_CLIENT_UNAVAILABLE",
                ),
                &request_id,
                Some(&route),
                rate_limit.as_ref(),
            );
        }
    };
    let upstream = client
        .post(endpoint)
        .bearer_auth(provider_key)
        .json(&input)
        .send()
        .await;
    let latency = start_time.elapsed().as_millis() as i32;
    let response = match upstream {
        Err(error) => {
            tracing::error!(error = %error, "Embedding upstream request failed");
            record(
                &pool,
                &context,
                &user,
                &route,
                0,
                0.0,
                latency,
                StatusCode::BAD_GATEWAY,
                "Embedding upstream request failed",
            )
            .await;
            crate::upstream_request_failed_response()
        }
        Ok(response) if !response.status().is_success() => {
            let upstream_status = response.status();
            record(
                &pool,
                &context,
                &user,
                &route,
                0,
                0.0,
                latency,
                StatusCode::from_u16(upstream_status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
                &format!(
                    "Embedding upstream returned HTTP {}",
                    upstream_status.as_u16()
                ),
            )
            .await;
            crate::upstream_status_error_response(upstream_status)
        }
        Ok(response) => {
            let mut value: Value = match response.json().await {
                Ok(value) => value,
                Err(error) => {
                    tracing::error!(error = %error, "Embedding upstream returned invalid JSON");
                    record(
                        &pool,
                        &context,
                        &user,
                        &route,
                        0,
                        0.0,
                        latency,
                        StatusCode::BAD_GATEWAY,
                        "Embedding upstream returned invalid JSON",
                    )
                    .await;
                    return decorate(
                        crate::api_error_response(
                            StatusCode::BAD_GATEWAY,
                            "Upstream returned an invalid embedding response",
                            "upstream_response_invalid",
                            "UPSTREAM_RESPONSE_INVALID",
                        ),
                        &request_id,
                        Some(&route),
                        rate_limit.as_ref(),
                    );
                }
            };
            let Some(prompt_tokens) = valid_embedding_response(&value, bounds.item_count) else {
                record(
                    &pool,
                    &context,
                    &user,
                    &route,
                    0,
                    0.0,
                    latency,
                    StatusCode::BAD_GATEWAY,
                    "Embedding upstream response failed schema validation",
                )
                .await;
                return decorate(
                    crate::api_error_response(
                        StatusCode::BAD_GATEWAY,
                        "Upstream returned an invalid embedding response",
                        "upstream_response_invalid",
                        "UPSTREAM_RESPONSE_INVALID",
                    ),
                    &request_id,
                    Some(&route),
                    rate_limit.as_ref(),
                );
            };
            let cost = crate::calculate_token_cost(prompt_tokens, 0, billable_input_rate, 0.0);
            if !record(
                &pool,
                &context,
                &user,
                &route,
                prompt_tokens,
                cost,
                latency,
                StatusCode::OK,
                "",
            )
            .await
            {
                return decorate(
                    crate::api_error_response(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Request completed upstream but billing could not be settled",
                        "billing_settlement_failed",
                        "BILLING_SETTLEMENT_FAILED",
                    ),
                    &request_id,
                    Some(&route),
                    rate_limit.as_ref(),
                );
            }
            value["model"] = json!(route.logical_model);
            Json(value).into_response()
        }
    };
    decorate(response, &request_id, Some(&route), rate_limit.as_ref())
}

#[cfg(test)]
mod tests {
    use super::{valid_embedding_response, validate_embedding_request, InputBounds};
    use serde_json::json;

    #[test]
    fn validates_supported_embedding_inputs_and_reservation_bounds() {
        assert_eq!(
            validate_embedding_request(
                &json!({"model":"text-embedding-3-small","input":"hello"}),
                8192,
            ),
            Ok(InputBounds {
                item_count: 1,
                maximum_tokens: 8192,
            })
        );
        assert_eq!(
            validate_embedding_request(
                &json!({"model":"text-embedding-3-small","input":[[1,2],[3]]}),
                8192,
            ),
            Ok(InputBounds {
                item_count: 2,
                maximum_tokens: 3,
            })
        );
    }

    #[test]
    fn rejects_unknown_parameters_and_mixed_input_shapes() {
        assert!(validate_embedding_request(
            &json!({"model":"text-embedding-3-small","input":"hello","stream":true}),
            8192,
        )
        .unwrap_err()
        .contains("unsupported parameter"));
        assert!(validate_embedding_request(
            &json!({"model":"text-embedding-3-small","input":["hello", 1]}),
            8192,
        )
        .is_err());
    }

    #[test]
    fn accepts_only_real_embedding_usage_and_vector_data() {
        let response = json!({
            "object": "list",
            "data": [{"object":"embedding","index":0,"embedding":[0.1,-0.2]}],
            "model": "text-embedding-3-small",
            "usage": {"prompt_tokens": 7, "total_tokens": 7}
        });
        assert_eq!(valid_embedding_response(&response, 1), Some(7));
        let missing_usage = json!({
            "object": "list",
            "data": [{"object":"embedding","index":0,"embedding":[0.1]}]
        });
        assert_eq!(valid_embedding_response(&missing_usage, 1), None);
    }
}
