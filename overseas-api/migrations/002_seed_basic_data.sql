-- Basic seed data for local overseas API testing.
-- Keep this file idempotent so it can be safely re-run in development.

INSERT INTO packages (
    package_id,
    name,
    price,
    token_quota,
    bonus_token,
    duration_days,
    priority,
    sort,
    status
) VALUES
    ('PKG_STARTER', 'Starter Token Pack', 19.90, 20000, 2000, 30, 0, 10, 0),
    ('PKG_PRO', 'Pro Token Pack', 99.00, 120000, 20000, 90, 1, 20, 0),
    ('PKG_BUSINESS', 'Business Token Pack', 499.00, 750000, 150000, 180, 2, 30, 0)
ON CONFLICT (package_id) DO UPDATE SET
    name = EXCLUDED.name,
    price = EXCLUDED.price,
    token_quota = EXCLUDED.token_quota,
    bonus_token = EXCLUDED.bonus_token,
    duration_days = EXCLUDED.duration_days,
    priority = EXCLUDED.priority,
    sort = EXCLUDED.sort,
    status = EXCLUDED.status,
    updated_at = NOW();

INSERT INTO providers (
    provider_id,
    name,
    base_url,
    api_key_env,
    status,
    sort
) VALUES
    ('openai', 'OpenAI', 'https://api.openai.com/v1', 'PROVIDER_OPENAI_API_KEY', 0, 10),
    ('anthropic', 'Anthropic', 'https://api.anthropic.com/v1', 'PROVIDER_ANTHROPIC_API_KEY', 0, 20),
    ('google', 'Google Gemini', 'https://generativelanguage.googleapis.com/v1beta', 'PROVIDER_GOOGLE_API_KEY', 0, 30)
ON CONFLICT (provider_id) DO UPDATE SET
    name = EXCLUDED.name,
    base_url = EXCLUDED.base_url,
    api_key_env = EXCLUDED.api_key_env,
    status = EXCLUDED.status,
    sort = EXCLUDED.sort,
    updated_at = NOW();

INSERT INTO provider_models (
    provider_id,
    model_id,
    name,
    display_name,
    input_rate,
    output_rate,
    context_len,
    max_tokens,
    status,
    sort
)
SELECT p.id, v.model_id, v.name, v.display_name, v.input_rate, v.output_rate, v.context_len, v.max_tokens, v.status, v.sort
FROM providers p
JOIN (
    VALUES
        ('openai', 'gpt-4.1-mini', 'gpt-4.1-mini', 'GPT-4.1 Mini', 0.400000, 1.600000, 1000000, 32768, 0, 10),
        ('openai', 'gpt-4o-mini', 'gpt-4o-mini', 'GPT-4o Mini', 0.150000, 0.600000, 128000, 16384, 0, 20),
        ('anthropic', 'claude-fable-5', 'claude-fable-5', 'Claude Fable 5', 10.000000, 50.000000, 1000000, 128000, 0, 30),
        ('anthropic', 'claude-sonnet-4-6', 'claude-sonnet-4-6', 'Claude Sonnet 4.6', 3.000000, 15.000000, 1000000, 64000, 0, 40),
        ('google', 'gemini-2.5-flash', 'gemini-2.5-flash', 'Gemini 2.5 Flash', 0.300000, 2.500000, 1000000, 65536, 0, 50),
        ('google', 'gemini-3.5-flash', 'gemini-3.5-flash', 'Gemini 3.5 Flash', 2.700000, 16.200000, 1000000, 65536, 0, 60)
) AS v(provider_id, model_id, name, display_name, input_rate, output_rate, context_len, max_tokens, status, sort)
ON p.provider_id = v.provider_id
ON CONFLICT (provider_id, model_id) DO UPDATE SET
    name = EXCLUDED.name,
    display_name = EXCLUDED.display_name,
    input_rate = EXCLUDED.input_rate,
    output_rate = EXCLUDED.output_rate,
    context_len = EXCLUDED.context_len,
    max_tokens = EXCLUDED.max_tokens,
    status = EXCLUDED.status,
    sort = EXCLUDED.sort,
    updated_at = NOW();
