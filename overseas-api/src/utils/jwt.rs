use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

static JWT_SECRET: LazyLock<String> = LazyLock::new(|| crate::config::jwt_secret());

fn jwt_secret_config() -> &'static [u8] {
    JWT_SECRET.as_bytes()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: i64,
    pub uid: String,
    pub email: String,
    pub token_version: i64,
    pub exp: i64,
    pub iat: i64,
}

impl Claims {
    pub fn new(user_id: i64, uid: &str, email: &str, token_version: i64) -> Self {
        let now = Utc::now().timestamp();
        Self {
            sub: user_id,
            uid: uid.to_string(),
            email: email.to_string(),
            token_version,
            exp: now + 7 * 24 * 3600,
            iat: now,
        }
    }
}

pub fn generate_token(claims: &Claims) -> Result<String, jsonwebtoken::errors::Error> {
    encode(
        &Header::default(),
        claims,
        &EncodingKey::from_secret(jwt_secret_config()),
    )
}

pub fn verify_token(token: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret_config()),
        &Validation::default(),
    )
    .map(|t| t.claims)
}
