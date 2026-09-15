use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Provider {
    pub id: i64,
    pub provider_id: String,
    pub name: String,
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
    pub status: i16,
    pub sort: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct ProviderModel {
    pub id: i64,
    pub provider_id: i64,
    pub model_id: String,
    pub name: String,
    pub display_name: Option<String>,
    pub input_rate: f64,
    pub output_rate: f64,
    pub context_len: i32,
    pub max_tokens: i32,
    pub status: i16,
    pub sort: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
