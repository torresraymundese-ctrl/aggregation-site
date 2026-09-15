use crate::utils::error::sanitize_error;
use crate::CurrentUser;
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use sqlx::{PgPool, Row};

pub async fn get_package(Path(id): Path<String>, State(pool): State<PgPool>) -> Response {
    match sqlx::query(
        "SELECT package_id, name, CAST(price AS VARCHAR), duration_days
         FROM packages WHERE package_id = $1 AND status = 0",
    )
    .bind(id)
    .fetch_optional(&pool)
    .await
    {
        Ok(Some(row)) => Json(json!({
            "code": 0,
            "data": {
                "id": row.get::<String, _>(0),
                "name": row.get::<String, _>(1),
                "price": row.get::<String, _>(2),
                "currency": "USD",
                "duration_days": row.get::<i32, _>(3),
            }
        }))
        .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"code": 404, "message": "Package not found"})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"code": 500, "message": sanitize_error(error)})),
        )
            .into_response(),
    }
}

pub async fn get_order(
    Path(id): Path<String>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Response {
    match sqlx::query(
        "SELECT o.order_no, p.name, CAST(o.amount AS VARCHAR),
                CAST(o.actual_amount AS VARCHAR), o.payment_status,
                o.payment_method, o.paid_at, o.created_at
         FROM overseas_orders o
         JOIN packages p ON p.id = o.package_id
         WHERE o.workspace_id = $1
           AND (o.order_no = $2 OR o.id::TEXT = $2)",
    )
    .bind(user.workspace_id)
    .bind(id)
    .fetch_optional(&pool)
    .await
    {
        Ok(Some(row)) => Json(json!({
            "code": 0,
            "data": {
                "order_no": row.get::<String, _>(0),
                "package_name": row.get::<String, _>(1),
                "amount": row.get::<String, _>(2),
                "actual_amount": row.get::<String, _>(3),
                "currency": "USD",
                "status": row.get::<i16, _>(4),
                "payment_method": row.get::<Option<String>, _>(5),
                "paid_at": row.get::<Option<chrono::NaiveDateTime>, _>(6),
                "created_at": row.get::<chrono::NaiveDateTime, _>(7),
            }
        }))
        .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"code": 404, "message": "Order not found"})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"code": 500, "message": sanitize_error(error)})),
        )
            .into_response(),
    }
}

pub async fn billing_usage(
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Response {
    match sqlx::query(
        "SELECT COUNT(*),
                COALESCE(SUM(input_tokens), 0),
                COALESCE(SUM(output_tokens), 0),
                CAST(COALESCE(SUM(user_cost_usd), 0) AS VARCHAR),
                CAST(COALESCE(SUM(provider_cost_usd), 0) AS VARCHAR)
         FROM api_calls
         WHERE workspace_id = $1
           AND created_at >= date_trunc('month', NOW())",
    )
    .bind(user.workspace_id)
    .fetch_one(&pool)
    .await
    {
        Ok(row) => Json(json!({
            "code": 0,
            "data": {
                "period": "current_month",
                "calls": row.get::<i64, _>(0),
                "input_tokens": row.get::<i64, _>(1),
                "output_tokens": row.get::<i64, _>(2),
                "user_cost_usd": row.get::<String, _>(3),
                "provider_cost_usd": row.get::<String, _>(4),
            }
        }))
        .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"code": 500, "message": sanitize_error(error)})),
        )
            .into_response(),
    }
}

pub async fn call_log_detail(
    Path(id): Path<i64>,
    State(pool): State<PgPool>,
    Extension(user): Extension<CurrentUser>,
) -> Response {
    match sqlx::query(
        "SELECT ac.request_id, ac.model_id, lm.display_name, p.name,
                ac.routing_strategy, ac.credential_source,
                ac.input_tokens, ac.output_tokens,
                CAST(ac.provider_cost_usd AS VARCHAR),
                CAST(ac.user_cost_usd AS VARCHAR),
                ac.first_token_latency_ms, ac.latency_ms,
                ac.status_code, ac.state, ac.error_code, ac.error_msg,
                ac.retryable, ac.created_at, ac.completed_at
         FROM api_calls ac
         LEFT JOIN logical_models lm ON lm.id = ac.logical_model_id
         LEFT JOIN providers p ON p.id = ac.provider_id
         WHERE ac.id = $1 AND ac.workspace_id = $2",
    )
    .bind(id)
    .bind(user.workspace_id)
    .fetch_optional(&pool)
    .await
    {
        Ok(Some(row)) => Json(json!({
            "code": 0,
            "data": {
                "id": id,
                "request_id": row.get::<String, _>(0),
                "model_id": row.get::<String, _>(1),
                "model_name": row.get::<Option<String>, _>(2),
                "provider": row.get::<Option<String>, _>(3),
                "routing_policy": row.get::<Option<String>, _>(4),
                "credential_source": row.get::<Option<String>, _>(5),
                "input_tokens": row.get::<i32, _>(6),
                "output_tokens": row.get::<i32, _>(7),
                "provider_cost_usd": row.get::<String, _>(8),
                "user_cost_usd": row.get::<String, _>(9),
                "first_token_latency_ms": row.get::<Option<i32>, _>(10),
                "latency_ms": row.get::<i32, _>(11),
                "status_code": row.get::<i32, _>(12),
                "state": row.get::<String, _>(13),
                "error_code": row.get::<Option<String>, _>(14),
                "error_message": row.get::<Option<String>, _>(15),
                "retryable": row.get::<Option<bool>, _>(16),
                "created_at": row.get::<chrono::NaiveDateTime, _>(17),
                "completed_at": row.get::<Option<chrono::NaiveDateTime>, _>(18),
            }
        }))
        .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({"code": 404, "message": "Call log not found"})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"code": 500, "message": sanitize_error(error)})),
        )
            .into_response(),
    }
}
