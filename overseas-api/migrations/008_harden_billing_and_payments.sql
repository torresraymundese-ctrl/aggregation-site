-- 统一价格为 USD/每百万 Token，并补齐资金精度和支付事件账本。
-- 旧数据的价格单位是 USD/每千 Token；rate_unit 只为空一次，因此重复执行不会重复放大。

ALTER TABLE provider_models
    ALTER COLUMN input_rate TYPE DECIMAL(18,6),
    ALTER COLUMN output_rate TYPE DECIMAL(18,6),
    ALTER COLUMN upstream_input_rate TYPE DECIMAL(18,6),
    ALTER COLUMN upstream_output_rate TYPE DECIMAL(18,6),
    ADD COLUMN IF NOT EXISTS rate_unit VARCHAR(32);

UPDATE provider_models
SET input_rate = input_rate * 1000,
    output_rate = output_rate * 1000,
    upstream_input_rate = upstream_input_rate * 1000,
    upstream_output_rate = upstream_output_rate * 1000,
    rate_unit = 'usd_per_million_tokens'
WHERE rate_unit IS NULL;

ALTER TABLE provider_models
    ALTER COLUMN rate_unit SET DEFAULT 'usd_per_million_tokens',
    ALTER COLUMN rate_unit SET NOT NULL;

ALTER TABLE balances
    ALTER COLUMN balance TYPE DECIMAL(18,8),
    ALTER COLUMN frozen_balance TYPE DECIMAL(18,8),
    ALTER COLUMN total_recharged TYPE DECIMAL(18,8),
    ALTER COLUMN total_consumed TYPE DECIMAL(18,8);

ALTER TABLE balance_logs
    ALTER COLUMN change_amount TYPE DECIMAL(18,8),
    ALTER COLUMN balance_before TYPE DECIMAL(18,8),
    ALTER COLUMN balance_after TYPE DECIMAL(18,8);

ALTER TABLE api_calls
    ALTER COLUMN cost_points TYPE DECIMAL(18,8);

CREATE TABLE IF NOT EXISTS payment_events (
    id BIGSERIAL PRIMARY KEY,
    provider VARCHAR(40) NOT NULL,
    event_id VARCHAR(128) NOT NULL,
    order_id BIGINT REFERENCES overseas_orders(id),
    payload_hash VARCHAR(64) NOT NULL,
    status VARCHAR(20) NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    processed_at TIMESTAMP,
    UNIQUE(provider, event_id)
);

CREATE INDEX IF NOT EXISTS idx_payment_events_order_id ON payment_events(order_id);
