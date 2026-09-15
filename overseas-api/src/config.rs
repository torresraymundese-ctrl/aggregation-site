use std::env;

pub fn database_url() -> String {
    env::var("DATABASE_URL").expect("DATABASE_URL must be set")
}

pub fn server_port() -> u16 {
    env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse()
        .unwrap_or(8080)
}

#[allow(dead_code)]
pub fn redis_url() -> String {
    env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string())
}

#[allow(dead_code)]
pub fn jwt_secret() -> String {
    let secret = env::var("JWT_SECRET").expect("JWT_SECRET must be set");
    if secret.trim().len() < 32 {
        panic!("JWT_SECRET must contain at least 32 characters");
    }
    secret
}

#[allow(dead_code)]
pub fn is_development() -> bool {
    env::var("RUST_ENV").unwrap_or_else(|_| "development".to_string()) == "development"
}

#[allow(dead_code)]
pub fn openai_api_key() -> String {
    env::var("PROVIDER_OPENAI_API_KEY").unwrap_or_default()
}

#[allow(dead_code)]
pub fn anthropic_api_key() -> String {
    env::var("PROVIDER_ANTHROPIC_API_KEY").unwrap_or_default()
}

#[allow(dead_code)]
pub fn google_api_key() -> String {
    env::var("PROVIDER_GOOGLE_API_KEY").unwrap_or_default()
}

#[allow(dead_code)]
pub fn provider_api_key(api_key_env: &str) -> String {
    env::var(api_key_env).unwrap_or_default()
}

pub fn admin_emails() -> Vec<String> {
    env::var("ADMIN_EMAILS")
        .unwrap_or_default()
        .split(',')
        .map(|email| email.trim().to_lowercase())
        .filter(|email| !email.is_empty())
        .collect()
}

pub fn cors_allowed_origins() -> Vec<String> {
    let raw = match env::var("CORS_ALLOWED_ORIGINS") {
        Ok(value) => value,
        Err(_) if is_development() => {
            "http://localhost:5173,http://127.0.0.1:5173,http://127.0.0.1:5180".to_string()
        }
        Err(_) => panic!("CORS_ALLOWED_ORIGINS must be set in production"),
    };

    let origins: Vec<String> = raw
        .split(',')
        .map(|origin| origin.trim().trim_end_matches('/').to_string())
        .filter(|origin| !origin.is_empty())
        .collect();

    if origins.is_empty() {
        panic!("CORS_ALLOWED_ORIGINS must contain at least one origin");
    }

    origins
}

pub fn trust_proxy_headers() -> bool {
    match env::var("TRUST_PROXY_HEADERS")
        .unwrap_or_else(|_| "false".to_string())
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "true" => true,
        "false" => false,
        _ => panic!("TRUST_PROXY_HEADERS must be true or false"),
    }
}
