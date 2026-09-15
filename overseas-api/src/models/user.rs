use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: i64,
    pub uid: String,
    pub email: String,
    pub nickname: String,
    pub password_hash: String,
    pub referrer_id: Option<i64>,
    pub source_channel: Option<String>,
    pub status: i16,
    pub failed_login_attempts: Option<i16>,  // 登录失败次数
    pub locked_until: Option<DateTime<Utc>>, // 账户锁定截止时间
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserInput {
    pub email: String,
    pub password: String,
    pub nickname: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginInput {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub uid: String,
    pub email: String,
    pub nickname: String,
}

#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub user: UserResponse,
    pub token: String,
}
