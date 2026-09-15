use axum::{
    extract::State,
    extract::Extension,
    response::Json,
};
use serde_json::json;

use crate::models::api_key::CreateKeyInput;
use crate::repository::api_key_repo::ApiKeyRepository;
use crate::utils::key_generator::{generate_api_key, get_key_prefix};
use crate::utils::id_generator::generate_key_id;
use crate::utils::jwt::Claims;
use crate::error::AppError;

pub async fn create(
    State(pool): State<sqlx::PgPool>,
    Extension(claims): Extension<Claims>,
    Json(input): Json<CreateKeyInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    let repo = ApiKeyRepository::new(&pool);

    let prefix = &input.name[..8.min(input.name.len())];
    let (raw_key, key_hash) = generate_api_key(prefix);
    let key_prefix = get_key_prefix(&raw_key);
    let uid = generate_key_id();

    let api_key = repo.create(
        &uid,
        claims.sub,
        &key_prefix,
        &key_hash,
        &input.name,
        input.rate_limit.unwrap_or(500),
        serde_json::json!(input.models.unwrap_or_else(|| vec!["gpt-4.1-mini".to_string()])),
        None,
    ).await.map_err(|e| AppError::ServerError(e.to_string()))?;

    Ok(Json(json!({
        "code": 0,
        "message": "创建成功",
        "data": {
            "uid": api_key.uid,
            "key": raw_key,
            "name": api_key.name,
            "rate_limit": api_key.rate_limit,
            "models_allowed": api_key.models_allowed,
        }
    })))
}

pub async fn list(
    State(pool): State<sqlx::PgPool>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<serde_json::Value>, AppError> {
    let repo = ApiKeyRepository::new(&pool);

    let keys = repo.find_by_user(claims.sub)
        .await
        .map_err(|e| AppError::ServerError(e.to_string()))?;

    let items: Vec<_> = keys.into_iter().map(|k| json!({
        "uid": k.uid,
        "key_prefix": k.key_prefix,
        "name": k.name,
        "rate_limit": k.rate_limit,
        "status": k.status,
    })).collect();

    Ok(Json(json!({ "code": 0, "data": { "items": items } })))
}

pub async fn update(
    Path(uid): Path<String>,
    State(pool): State<sqlx::PgPool>,
    Extension(claims): Extension<Claims>,
    Json(input): Json<CreateKeyInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    let repo = ApiKeyRepository::new(&pool);

    let key = repo.find_by_uid(&uid, claims.sub)
        .await
        .map_err(|e| AppError::ServerError(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("Key不存在".to_string()))?;

    repo.update(key.id, &input.name, input.rate_limit.unwrap_or(500))
        .await
        .map_err(|e| AppError::ServerError(e.to_string()))?;

    Ok(Json(json!({ "code": 0, "message": "更新成功" })))
}

pub async fn delete(
    Path(uid): Path<String>,
    State(pool): State<sqlx::PgPool>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<serde_json::Value>, AppError> {
    let repo = ApiKeyRepository::new(&pool);

    let key = repo.find_by_uid(&uid, claims.sub)
        .await
        .map_err(|e| AppError::ServerError(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("Key不存在".to_string()))?;

    repo.soft_delete(key.id)
        .await
        .map_err(|e| AppError::ServerError(e.to_string()))?;

    Ok(Json(json!({ "code": 0, "message": "删除成功" })))
}
