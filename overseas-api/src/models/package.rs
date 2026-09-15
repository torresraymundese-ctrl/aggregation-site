use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Package {
    pub id: i64,
    pub package_id: String,
    pub name: String,
    pub price: f64,
    pub token_quota: i32,
    pub bonus_token: i32,
    pub duration_days: i32,
    pub priority: i16,
    pub sort: i32,
    pub status: i16,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Order {
    pub id: i64,
    pub order_no: String,
    pub user_id: i64,
    pub package_id: i64,
    pub amount: f64,
    pub actual_amount: f64,
    pub bonus_amount: f64,
    pub payment_method: Option<String>,
    pub payment_status: i16,
    pub paid_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Balance {
    pub id: i64,
    pub user_id: i64,
    pub balance: f64,
    pub frozen_balance: f64,
    pub total_recharged: f64,
    pub total_consumed: f64,
    pub updated_at: DateTime<Utc>,
}
