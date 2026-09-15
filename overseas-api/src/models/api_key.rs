use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct ApiKey {
    pub id: i64,
    pub uid: String,
    pub user_id: i64,
    pub key_prefix: String,
    pub key_hash: String,
    pub name: Option<String>,
    pub rate_limit: i32,
    pub models_allowed: Option<serde_json::Value>,
    pub status: i16,
    pub last_used_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Deserialize)]
pub struct CreateKeyInput {
    pub name: String,
    pub rate_limit: Option<i32>,
    pub models: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct KeyResponse {
    pub uid: String,
    pub key_prefix: String,
    pub name: String,
    pub rate_limit: i32,
    pub status: i16,
}
