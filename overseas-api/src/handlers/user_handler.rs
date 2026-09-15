use axum::{
    routing::{get, post},
    extract::{State, Extension, Path},
    response::Json,
};
use serde_json::json;

use crate::models::user::{CreateUserInput, LoginInput};
use crate::repository::user_repo::UserRepository;
use crate::utils::jwt::{generate_token, Claims};
use crate::utils::id_generator::generate_uid;
use crate::error::AppError;
use bcrypt::{hash, verify, DEFAULT_COST};

pub async fn register(
    State(pool): State<sqlx::PgPool>,
    Json(input): Json<CreateUserInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    let repo = UserRepository::new(&pool);

    if repo.find_by_email(&input.email).await?.is_some() {
        return Err(AppError::AlreadyExists("邮箱已被注册".to_string()));
    }

    let password_hash = hash(&input.password, DEFAULT_COST)
        .map_err(|_| AppError::ServerError("密码加密失败".to_string()))?;

    let uid = generate_uid();

    let user = repo.create(&uid, &input.email, &input.nickname, &password_hash)
        .await
        .map_err(|e| AppError::ServerError(e.to_string()))?;

    let claims = Claims::new(user.id, &user.uid, &user.email);
    let token = generate_token(&claims)
        .map_err(|_| AppError::ServerError("Token生成失败".to_string()))?;

    Ok(Json(json!({
        "code": 0,
        "message": "注册成功",
        "data": {
            "user": { "uid": user.uid, "email": user.email, "nickname": user.nickname },
            "token": token,
        }
    })))
}

pub async fn login(
    State(pool): State<sqlx::PgPool>,
    Json(input): Json<LoginInput>,
) -> Result<Json<serde_json::Value>, AppError> {
    let repo = UserRepository::new(&pool);

    let user = repo.find_by_email(&input.email)
        .await
        .map_err(|e| AppError::ServerError(e.to_string()))?
        .ok_or_else(|| AppError::NotFound("用户不存在".to_string()))?;

    let valid = verify(&input.password, &user.password_hash)
        .map_err(|_| AppError::ServerError("密码验证失败".to_string()))?;

    if !valid {
        return Err(AppError::NotFound("邮箱或密码错误".to_string()));
    }

    if user.status == 1 {
        return Err(AppError::Forbidden("账号已被封禁".to_string()));
    }

    repo.update_last_login(user.id).await.ok();

    let claims = Claims::new(user.id, &user.uid, &user.email);
    let token = generate_token(&claims)
        .map_err(|_| AppError::ServerError("Token生成失败".to_string()))?;

    Ok(Json(json!({
        "code": 0,
        "message": "登录成功",
        "data": {
            "user": { "uid": user.uid, "email": user.email, "nickname": user.nickname },
            "token": token,
        }
    })))
}

pub async fn logout() -> Json<serde_json::Value> {
    Json(json!({ "code": 0, "message": "登出成功" }))
}

pub async fn me(Extension(claims): Extension<Claims>) -> Json<serde_json::Value> {
    Json(json!({ "code": 0, "message": "success", "data": { "uid": claims.uid, "email": claims.email } }))
}