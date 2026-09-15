-- 为真实 OpenAI-compatible Embeddings 增加独立协议和首个 BYOK 接入点。
-- text-embedding-3-small 官方标准价为每百万输入 Token 0.02 美元，向量本身不按输出 Token 计费。

ALTER TABLE provider_models
    DROP CONSTRAINT IF EXISTS chk_provider_models_protocol;

ALTER TABLE provider_models
    ADD CONSTRAINT chk_provider_models_protocol
    CHECK (protocol IN (
        'openai_chat',
        'anthropic_messages',
        'gemini_generate_content',
        'replicate_predictions',
        'openai_embeddings'
    ));

INSERT INTO logical_models (
    model_id,
    display_name,
    vendor,
    description,
    input_price_per_million,
    output_price_per_million,
    context_len,
    max_tokens,
    input_modalities,
    output_modalities,
    supported_parameters,
    status,
    sort
) VALUES (
    'text-embedding-3-small',
    'Text Embedding 3 Small',
    'OpenAI',
    'OpenAI small text embedding model',
    0.02000000,
    0.00000000,
    8192,
    1,
    '["text"]'::JSONB,
    '["embedding"]'::JSONB,
    '["encoding_format", "dimensions", "user"]'::JSONB,
    0,
    5
)
ON CONFLICT (model_id) DO NOTHING;

INSERT INTO provider_models (
    provider_id,
    model_id,
    name,
    display_name,
    input_rate,
    output_rate,
    rate_unit,
    context_len,
    max_tokens,
    status,
    sort,
    upstream_input_rate,
    upstream_output_rate,
    margin_rate,
    logical_model_id,
    protocol,
    region,
    route_priority,
    upstream_input_price_per_million,
    upstream_output_price_per_million
)
SELECT
    p.id,
    'text-embedding-3-small',
    'text-embedding-3-small',
    'Text Embedding 3 Small',
    0.020000,
    0.000000,
    'usd_per_million_tokens',
    8192,
    1,
    0,
    5,
    0.020000,
    0.000000,
    0,
    lm.id,
    'openai_embeddings',
    'global',
    10,
    0.02000000,
    0.00000000
FROM providers p
JOIN logical_models lm ON lm.model_id = 'text-embedding-3-small'
WHERE p.provider_id = 'openai'
ON CONFLICT (provider_id, model_id) DO UPDATE SET
    name = EXCLUDED.name,
    display_name = EXCLUDED.display_name,
    input_rate = EXCLUDED.input_rate,
    output_rate = EXCLUDED.output_rate,
    rate_unit = EXCLUDED.rate_unit,
    context_len = EXCLUDED.context_len,
    max_tokens = EXCLUDED.max_tokens,
    upstream_input_rate = EXCLUDED.upstream_input_rate,
    upstream_output_rate = EXCLUDED.upstream_output_rate,
    margin_rate = EXCLUDED.margin_rate,
    logical_model_id = EXCLUDED.logical_model_id,
    protocol = EXCLUDED.protocol,
    region = EXCLUDED.region,
    route_priority = EXCLUDED.route_priority,
    upstream_input_price_per_million = EXCLUDED.upstream_input_price_per_million,
    upstream_output_price_per_million = EXCLUDED.upstream_output_price_per_million,
    updated_at = NOW();

INSERT INTO routing_policies (
    logical_model_id,
    strategy,
    fixed_provider_model_id,
    input_weight,
    output_weight,
    minimum_samples
)
SELECT lm.id, 'fixed', pm.id, 1, 0, 5
FROM logical_models lm
JOIN provider_models pm
  ON pm.logical_model_id = lm.id
 AND pm.protocol = 'openai_embeddings'
WHERE lm.model_id = 'text-embedding-3-small'
ORDER BY pm.route_priority, pm.id
LIMIT 1
ON CONFLICT (logical_model_id) DO NOTHING;

-- 旧迁移把模型能力统一写成文本基础参数；按真实协议修正模型广场元数据。
UPDATE logical_models lm
SET input_modalities = '["text", "image"]'::JSONB,
    output_modalities = '["text"]'::JSONB,
    supported_parameters = '["temperature", "top_p", "max_tokens", "stop", "n", "stream", "tools", "tool_choice", "response_format"]'::JSONB,
    updated_at = NOW()
WHERE EXISTS (
    SELECT 1
    FROM provider_models pm
    JOIN providers p ON p.id = pm.provider_id
    WHERE pm.logical_model_id = lm.id
      AND pm.protocol = 'openai_chat'
      AND p.provider_id = 'openai'
);

UPDATE logical_models lm
SET input_modalities = '["text", "image"]'::JSONB,
    output_modalities = '["text"]'::JSONB,
    supported_parameters = '["temperature", "top_p", "max_tokens", "stop", "tools", "tool_choice", "response_format"]'::JSONB,
    updated_at = NOW()
WHERE EXISTS (
    SELECT 1 FROM provider_models pm
    WHERE pm.logical_model_id = lm.id
      AND pm.protocol = 'anthropic_messages'
);

UPDATE logical_models lm
SET input_modalities = '["text", "image"]'::JSONB,
    output_modalities = '["text"]'::JSONB,
    supported_parameters = '["temperature", "top_p", "max_tokens", "stop", "n", "tools", "tool_choice", "response_format"]'::JSONB,
    updated_at = NOW()
WHERE EXISTS (
    SELECT 1 FROM provider_models pm
    WHERE pm.logical_model_id = lm.id
      AND pm.protocol = 'gemini_generate_content'
);

UPDATE logical_models lm
SET input_modalities = '["text"]'::JSONB,
    output_modalities = '["embedding"]'::JSONB,
    supported_parameters = '["encoding_format", "dimensions", "user"]'::JSONB,
    updated_at = NOW()
WHERE EXISTS (
    SELECT 1 FROM provider_models pm
    WHERE pm.logical_model_id = lm.id
      AND pm.protocol = 'openai_embeddings'
);
