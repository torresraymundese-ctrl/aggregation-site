-- 聚合核心：把公开逻辑模型与供应商接入点分离，并建立可追踪的单次上游调用账本。
-- 价格单位已由 008 统一为 USD/每百万 Token；本迁移严禁再次换算价格。

CREATE TABLE IF NOT EXISTS logical_models (
    id BIGSERIAL PRIMARY KEY,
    model_id VARCHAR(100) UNIQUE NOT NULL,
    display_name VARCHAR(120) NOT NULL,
    vendor VARCHAR(80) NOT NULL,
    description TEXT,
    input_price_per_million DECIMAL(20,8) NOT NULL CHECK (input_price_per_million >= 0),
    output_price_per_million DECIMAL(20,8) NOT NULL CHECK (output_price_per_million >= 0),
    cached_input_price_per_million DECIMAL(20,8) CHECK (cached_input_price_per_million >= 0),
    context_len INT NOT NULL CHECK (context_len > 0),
    max_tokens INT NOT NULL CHECK (max_tokens > 0),
    input_modalities JSONB NOT NULL DEFAULT '["text"]'::jsonb
        CHECK (jsonb_typeof(input_modalities) = 'array'),
    output_modalities JSONB NOT NULL DEFAULT '["text"]'::jsonb
        CHECK (jsonb_typeof(output_modalities) = 'array'),
    supported_parameters JSONB NOT NULL DEFAULT '[]'::jsonb
        CHECK (jsonb_typeof(supported_parameters) = 'array'),
    status SMALLINT NOT NULL DEFAULT 0 CHECK (status IN (0, 1)),
    sort INT NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_logical_models_status_sort
    ON logical_models(status, sort, id);

-- 每个旧模型先形成一个逻辑模型。重复 model_id 取排序最靠前的接入点作为公开信息来源。
INSERT INTO logical_models (
    model_id,
    display_name,
    vendor,
    input_price_per_million,
    output_price_per_million,
    context_len,
    max_tokens,
    input_modalities,
    output_modalities,
    supported_parameters,
    status,
    sort,
    created_at,
    updated_at
)
SELECT DISTINCT ON (pm.model_id)
    pm.model_id,
    COALESCE(NULLIF(pm.display_name, ''), NULLIF(pm.name, ''), pm.model_id),
    p.name,
    pm.input_rate,
    pm.output_rate,
    GREATEST(COALESCE(pm.context_len, 4096), 1),
    GREATEST(COALESCE(pm.max_tokens, 4096), 1),
    '["text"]'::jsonb,
    '["text"]'::jsonb,
    '["temperature", "top_p", "max_tokens"]'::jsonb,
    CASE WHEN pm.status = 0 THEN 0 ELSE 1 END,
    COALESCE(pm.sort, 0),
    pm.created_at,
    pm.updated_at
FROM provider_models pm
JOIN providers p ON p.id = pm.provider_id
ORDER BY pm.model_id, pm.sort, pm.id
ON CONFLICT (model_id) DO NOTHING;

ALTER TABLE provider_models
    ADD COLUMN IF NOT EXISTS logical_model_id BIGINT,
    ADD COLUMN IF NOT EXISTS protocol VARCHAR(32),
    ADD COLUMN IF NOT EXISTS region VARCHAR(32),
    ADD COLUMN IF NOT EXISTS route_priority INT NOT NULL DEFAULT 100,
    ADD COLUMN IF NOT EXISTS upstream_input_price_per_million DECIMAL(20,8),
    ADD COLUMN IF NOT EXISTS upstream_output_price_per_million DECIMAL(20,8);

UPDATE provider_models pm
SET logical_model_id = lm.id
FROM logical_models lm
WHERE pm.logical_model_id IS NULL
  AND lm.model_id = pm.model_id;

UPDATE provider_models pm
SET protocol = CASE
        WHEN p.provider_id = 'anthropic' THEN 'anthropic_messages'
        WHEN p.provider_id = 'google' THEN 'gemini_generate_content'
        ELSE 'openai_chat'
    END
FROM providers p
WHERE pm.provider_id = p.id
  AND pm.protocol IS NULL;

UPDATE provider_models
SET upstream_input_price_per_million = COALESCE(upstream_input_price_per_million, upstream_input_rate, input_rate),
    upstream_output_price_per_million = COALESCE(upstream_output_price_per_million, upstream_output_rate, output_rate)
WHERE upstream_input_price_per_million IS NULL
   OR upstream_output_price_per_million IS NULL;

ALTER TABLE provider_models
    ALTER COLUMN logical_model_id SET NOT NULL,
    ALTER COLUMN protocol SET DEFAULT 'openai_chat',
    ALTER COLUMN protocol SET NOT NULL,
    ALTER COLUMN upstream_input_price_per_million SET NOT NULL,
    ALTER COLUMN upstream_output_price_per_million SET NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_provider_models_logical_model'
    ) THEN
        ALTER TABLE provider_models
            ADD CONSTRAINT fk_provider_models_logical_model
            FOREIGN KEY (logical_model_id) REFERENCES logical_models(id);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_provider_models_protocol'
    ) THEN
        ALTER TABLE provider_models
            ADD CONSTRAINT chk_provider_models_protocol
            CHECK (protocol IN ('openai_chat', 'anthropic_messages', 'gemini_generate_content'));
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_provider_models_upstream_prices'
    ) THEN
        ALTER TABLE provider_models
            ADD CONSTRAINT chk_provider_models_upstream_prices
            CHECK (
                upstream_input_price_per_million >= 0
                AND upstream_output_price_per_million >= 0
            );
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_provider_models_logical_route
    ON provider_models(logical_model_id, status, route_priority, id);
CREATE INDEX IF NOT EXISTS idx_provider_models_provider_status
    ON provider_models(provider_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS uq_provider_models_id_logical_model
    ON provider_models(id, logical_model_id);

CREATE TABLE IF NOT EXISTS routing_policies (
    id BIGSERIAL PRIMARY KEY,
    logical_model_id BIGINT UNIQUE NOT NULL REFERENCES logical_models(id) ON DELETE CASCADE,
    strategy VARCHAR(32) NOT NULL,
    fixed_provider_model_id BIGINT,
    input_weight DECIMAL(8,4) NOT NULL DEFAULT 1 CHECK (input_weight >= 0),
    output_weight DECIMAL(8,4) NOT NULL DEFAULT 1 CHECK (output_weight >= 0),
    minimum_samples INT NOT NULL DEFAULT 5 CHECK (minimum_samples > 0),
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    CONSTRAINT chk_routing_policies_strategy
        CHECK (strategy IN ('fixed', 'lowest_price', 'lowest_latency', 'highest_stability')),
    CONSTRAINT chk_routing_policies_weights
        CHECK (input_weight + output_weight > 0),
    CONSTRAINT chk_routing_policies_fixed_target
        CHECK (
            (strategy = 'fixed' AND fixed_provider_model_id IS NOT NULL)
            OR (strategy <> 'fixed' AND fixed_provider_model_id IS NULL)
        ),
    CONSTRAINT fk_routing_policies_fixed_endpoint
        FOREIGN KEY (fixed_provider_model_id, logical_model_id)
        REFERENCES provider_models(id, logical_model_id)
);

INSERT INTO routing_policies (
    logical_model_id,
    strategy,
    fixed_provider_model_id,
    input_weight,
    output_weight,
    minimum_samples
)
SELECT lm.id, 'fixed', selected.provider_model_id, 1, 1, 5
FROM logical_models lm
JOIN LATERAL (
    SELECT pm.id AS provider_model_id
    FROM provider_models pm
    WHERE pm.logical_model_id = lm.id
    ORDER BY pm.sort, pm.id
    LIMIT 1
) selected ON TRUE
ON CONFLICT (logical_model_id) DO NOTHING;

ALTER TABLE api_calls
    ADD COLUMN IF NOT EXISTS logical_model_id BIGINT,
    ADD COLUMN IF NOT EXISTS provider_model_id BIGINT,
    ADD COLUMN IF NOT EXISTS routing_strategy VARCHAR(32),
    ADD COLUMN IF NOT EXISTS credential_source VARCHAR(16),
    ADD COLUMN IF NOT EXISTS provider_cost_usd DECIMAL(20,8) NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS user_cost_usd DECIMAL(20,8) NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS first_token_latency_ms INT,
    ADD COLUMN IF NOT EXISTS error_code VARCHAR(64),
    ADD COLUMN IF NOT EXISTS retryable BOOLEAN,
    ADD COLUMN IF NOT EXISTS state VARCHAR(16) NOT NULL DEFAULT 'pending',
    ADD COLUMN IF NOT EXISTS completed_at TIMESTAMP;

UPDATE api_calls ac
SET logical_model_id = lm.id
FROM logical_models lm
WHERE ac.logical_model_id IS NULL
  AND lm.model_id = ac.model_id;

UPDATE api_calls ac
SET provider_model_id = pm.id
FROM provider_models pm
WHERE ac.provider_model_id IS NULL
  AND ac.provider_id = pm.provider_id
  AND ac.logical_model_id = pm.logical_model_id
  AND ac.model_id = pm.model_id;

UPDATE api_calls ac
SET routing_strategy = COALESCE(ac.routing_strategy, CASE WHEN ac.provider_model_id IS NOT NULL THEN 'fixed' END),
    credential_source = COALESCE(ac.credential_source, CASE WHEN ac.provider_model_id IS NOT NULL THEN 'platform' END),
    provider_cost_usd = CASE
        WHEN ac.provider_model_id IS NULL THEN COALESCE(ac.provider_cost_usd, 0)
        ELSE ROUND((
            ac.input_tokens::DECIMAL * pm.upstream_input_price_per_million
            + ac.output_tokens::DECIMAL * pm.upstream_output_price_per_million
        ) / 1000000, 8)
    END,
    user_cost_usd = COALESCE(ac.cost_points, 0),
    state = CASE
        WHEN ac.status_code BETWEEN 200 AND 299 THEN 'succeeded'
        WHEN ac.status_code IS NULL THEN 'pending'
        ELSE 'failed'
    END,
    completed_at = CASE WHEN ac.status_code IS NULL THEN ac.completed_at ELSE COALESCE(ac.completed_at, ac.created_at) END
FROM provider_models pm
WHERE ac.provider_model_id = pm.id;

-- 没有匹配到历史接入点的记录仍需补齐用户费用和状态。
UPDATE api_calls
SET user_cost_usd = COALESCE(cost_points, 0),
    state = CASE
        WHEN status_code BETWEEN 200 AND 299 THEN 'succeeded'
        WHEN status_code IS NULL THEN 'pending'
        ELSE 'failed'
    END,
    completed_at = CASE WHEN status_code IS NULL THEN completed_at ELSE COALESCE(completed_at, created_at) END
WHERE provider_model_id IS NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_api_calls_logical_model'
    ) THEN
        ALTER TABLE api_calls
            ADD CONSTRAINT fk_api_calls_logical_model
            FOREIGN KEY (logical_model_id) REFERENCES logical_models(id);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_api_calls_provider_model'
    ) THEN
        ALTER TABLE api_calls
            ADD CONSTRAINT fk_api_calls_provider_model
            FOREIGN KEY (provider_model_id) REFERENCES provider_models(id);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_api_calls_routing_strategy'
    ) THEN
        ALTER TABLE api_calls
            ADD CONSTRAINT chk_api_calls_routing_strategy
            CHECK (
                routing_strategy IS NULL
                OR routing_strategy IN ('fixed', 'lowest_price', 'lowest_latency', 'highest_stability')
            );
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_api_calls_credential_source'
    ) THEN
        ALTER TABLE api_calls
            ADD CONSTRAINT chk_api_calls_credential_source
            CHECK (credential_source IS NULL OR credential_source IN ('platform', 'byok'));
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_api_calls_state'
    ) THEN
        ALTER TABLE api_calls
            ADD CONSTRAINT chk_api_calls_state
            CHECK (state IN ('pending', 'succeeded', 'failed'));
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_api_calls_costs'
    ) THEN
        ALTER TABLE api_calls
            ADD CONSTRAINT chk_api_calls_costs
            CHECK (provider_cost_usd >= 0 AND user_cost_usd >= 0);
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_api_calls_logical_model_created_at
    ON api_calls(logical_model_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_api_calls_provider_model_created_at
    ON api_calls(provider_model_id, created_at DESC);

CREATE TABLE IF NOT EXISTS request_attempts (
    id BIGSERIAL PRIMARY KEY,
    api_call_id BIGINT UNIQUE NOT NULL REFERENCES api_calls(id) ON DELETE CASCADE,
    attempt_no SMALLINT NOT NULL DEFAULT 1 CHECK (attempt_no = 1),
    provider_model_id BIGINT NOT NULL REFERENCES provider_models(id),
    provider_credential_id BIGINT,
    started_at TIMESTAMP NOT NULL DEFAULT NOW(),
    first_token_at TIMESTAMP,
    completed_at TIMESTAMP,
    status_code INT,
    error_code VARCHAR(64),
    retryable BOOLEAN,
    latency_ms INT CHECK (latency_ms IS NULL OR latency_ms >= 0),
    first_token_latency_ms INT CHECK (first_token_latency_ms IS NULL OR first_token_latency_ms >= 0),
    input_tokens INT NOT NULL DEFAULT 0 CHECK (input_tokens >= 0),
    output_tokens INT NOT NULL DEFAULT 0 CHECK (output_tokens >= 0),
    provider_cost_usd DECIMAL(20,8) NOT NULL DEFAULT 0 CHECK (provider_cost_usd >= 0),
    upstream_request_id VARCHAR(128)
);

INSERT INTO request_attempts (
    api_call_id,
    attempt_no,
    provider_model_id,
    started_at,
    completed_at,
    status_code,
    error_code,
    retryable,
    latency_ms,
    first_token_latency_ms,
    input_tokens,
    output_tokens,
    provider_cost_usd
)
SELECT
    ac.id,
    1,
    ac.provider_model_id,
    ac.created_at,
    ac.completed_at,
    ac.status_code,
    ac.error_code,
    ac.retryable,
    ac.latency_ms,
    ac.first_token_latency_ms,
    ac.input_tokens,
    ac.output_tokens,
    ac.provider_cost_usd
FROM api_calls ac
WHERE ac.provider_model_id IS NOT NULL
ON CONFLICT (api_call_id) DO NOTHING;

CREATE INDEX IF NOT EXISTS idx_request_attempts_provider_started_at
    ON request_attempts(provider_model_id, started_at DESC);

