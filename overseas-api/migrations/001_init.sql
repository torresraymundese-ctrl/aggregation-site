-- Overseas API Relay Station - Initial Database Schema
-- 9 tables for overseas API relay service

-- ========================================
-- Users Table
-- ========================================
CREATE TABLE IF NOT EXISTS overseas_users (
    id BIGSERIAL PRIMARY KEY,
    uid VARCHAR(20) UNIQUE NOT NULL,
    email VARCHAR(255) UNIQUE NOT NULL,
    nickname VARCHAR(50) NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    referrer_id BIGINT,
    source_channel VARCHAR(20),
    status SMALLINT DEFAULT 0,
    last_login_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_overseas_users_email ON overseas_users(email);
CREATE INDEX IF NOT EXISTS idx_overseas_users_status ON overseas_users(status);

-- ========================================
-- API Keys Table
-- ========================================
CREATE TABLE IF NOT EXISTS api_keys (
    id BIGSERIAL PRIMARY KEY,
    uid VARCHAR(20) UNIQUE NOT NULL,
    user_id BIGINT NOT NULL REFERENCES overseas_users(id),
    key_prefix VARCHAR(20) NOT NULL,
    key_hash VARCHAR(255) NOT NULL,
    name VARCHAR(50),
    rate_limit INT DEFAULT 500,
    models_allowed JSONB,
    status SMALLINT DEFAULT 0,
    last_used_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_api_keys_user_id ON api_keys(user_id);
CREATE INDEX IF NOT EXISTS idx_api_keys_key_hash ON api_keys(key_hash);

-- ========================================
-- Packages Table
-- ========================================
CREATE TABLE IF NOT EXISTS packages (
    id BIGSERIAL PRIMARY KEY,
    package_id VARCHAR(20) UNIQUE NOT NULL,
    name VARCHAR(50) NOT NULL,
    price DECIMAL(10,2) NOT NULL,
    token_quota INT NOT NULL,
    bonus_token INT DEFAULT 0,
    duration_days INT NOT NULL,
    priority SMALLINT DEFAULT 0,
    sort INT DEFAULT 0,
    status SMALLINT DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_packages_status ON packages(status);

-- ========================================
-- Providers Table
-- ========================================
CREATE TABLE IF NOT EXISTS providers (
    id BIGSERIAL PRIMARY KEY,
    provider_id VARCHAR(20) UNIQUE NOT NULL,
    name VARCHAR(50) NOT NULL,
    base_url VARCHAR(255),
    api_key_env VARCHAR(50),
    status SMALLINT DEFAULT 0,
    sort INT DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

-- ========================================
-- Provider Models Table
-- ========================================
CREATE TABLE IF NOT EXISTS provider_models (
    id BIGSERIAL PRIMARY KEY,
    provider_id BIGINT NOT NULL REFERENCES providers(id),
    model_id VARCHAR(50) NOT NULL,
    name VARCHAR(50) NOT NULL,
    display_name VARCHAR(100),
    input_rate DECIMAL(18,6) NOT NULL,
    output_rate DECIMAL(18,6) NOT NULL,
    rate_unit VARCHAR(32) NOT NULL DEFAULT 'usd_per_million_tokens',
    context_len INT DEFAULT 4096,
    max_tokens INT DEFAULT 4096,
    status SMALLINT DEFAULT 0,
    sort INT DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    UNIQUE(provider_id, model_id)
);

-- ========================================
-- Orders Table
-- ========================================
CREATE TABLE IF NOT EXISTS overseas_orders (
    id BIGSERIAL PRIMARY KEY,
    order_no VARCHAR(32) UNIQUE NOT NULL,
    user_id BIGINT NOT NULL REFERENCES overseas_users(id),
    package_id BIGINT NOT NULL REFERENCES packages(id),
    amount DECIMAL(10,2) NOT NULL,
    actual_amount DECIMAL(10,2) NOT NULL,
    bonus_amount DECIMAL(10,2) DEFAULT 0,
    payment_method VARCHAR(20),
    payment_status SMALLINT DEFAULT 0,
    paid_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_overseas_orders_user_id ON overseas_orders(user_id);
CREATE INDEX IF NOT EXISTS idx_overseas_orders_payment_status ON overseas_orders(payment_status);

-- ========================================
-- Balances Table
-- ========================================
CREATE TABLE IF NOT EXISTS balances (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT UNIQUE NOT NULL REFERENCES overseas_users(id),
    balance DECIMAL(18,8) DEFAULT 0,
    frozen_balance DECIMAL(18,8) DEFAULT 0,
    total_recharged DECIMAL(18,8) DEFAULT 0,
    total_consumed DECIMAL(18,8) DEFAULT 0,
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

-- ========================================
-- Balance Logs Table
-- ========================================
CREATE TABLE IF NOT EXISTS balance_logs (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES overseas_users(id),
    order_id BIGINT REFERENCES overseas_orders(id),
    change_amount DECIMAL(18,8) NOT NULL,
    balance_before DECIMAL(18,8) NOT NULL,
    balance_after DECIMAL(18,8) NOT NULL,
    change_type VARCHAR(20) NOT NULL,
    note TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_balance_logs_user_id ON balance_logs(user_id);
CREATE INDEX IF NOT EXISTS idx_balance_logs_order_id ON balance_logs(order_id);
CREATE INDEX IF NOT EXISTS idx_balance_logs_change_type ON balance_logs(change_type);

-- ========================================
-- API Calls Log Table
-- ========================================
CREATE TABLE IF NOT EXISTS api_calls (
    id BIGSERIAL PRIMARY KEY,
    request_id VARCHAR(32) UNIQUE NOT NULL,
    api_key_id BIGINT NOT NULL REFERENCES api_keys(id),
    user_id BIGINT NOT NULL REFERENCES overseas_users(id),
    model_id VARCHAR(50) NOT NULL,
    provider_id BIGINT REFERENCES providers(id),
    input_tokens INT DEFAULT 0,
    output_tokens INT DEFAULT 0,
    cost_points DECIMAL(18,8) DEFAULT 0,
    latency_ms INT DEFAULT 0,
    status_code INT,
    error_msg TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_api_calls_api_key_id ON api_calls(api_key_id);
CREATE INDEX IF NOT EXISTS idx_api_calls_user_id ON api_calls(user_id);
CREATE INDEX IF NOT EXISTS idx_api_calls_created_at ON api_calls(created_at);
