-- Replicate 异步预测接入：仅保存路由、状态和结果 URL，不保存提示词或上游正文。

ALTER TABLE provider_models
    ALTER COLUMN model_id TYPE VARCHAR(255),
    ADD COLUMN IF NOT EXISTS async_output_url_pointer VARCHAR(255);

DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_provider_models_protocol'
    ) THEN
        ALTER TABLE provider_models DROP CONSTRAINT chk_provider_models_protocol;
    END IF;
    ALTER TABLE provider_models
        ADD CONSTRAINT chk_provider_models_protocol
        CHECK (protocol IN (
            'openai_chat',
            'anthropic_messages',
            'gemini_generate_content',
            'replicate_predictions'
        ));

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_replicate_output_pointer'
    ) THEN
        ALTER TABLE provider_models
            ADD CONSTRAINT chk_replicate_output_pointer
            CHECK (
                protocol <> 'replicate_predictions'
                OR (
                    async_output_url_pointer IS NOT NULL
                    AND LEFT(async_output_url_pointer, 1) = '/'
                )
            );
    END IF;
END $$;

INSERT INTO providers (
    provider_id, name, base_url, api_key_env, status, sort, credential_mode
)
VALUES (
    'replicate', 'Replicate', 'https://api.replicate.com/v1',
    'REPLICATE_API_TOKEN', 0, 100, 'byok'
)
ON CONFLICT (provider_id) DO NOTHING;

ALTER TABLE async_inference_tasks
    ADD COLUMN IF NOT EXISTS api_call_id BIGINT REFERENCES api_calls(id),
    ADD COLUMN IF NOT EXISTS upstream_get_url TEXT,
    ADD COLUMN IF NOT EXISTS upstream_cancel_url TEXT,
    ADD COLUMN IF NOT EXISTS upstream_webhook_url TEXT,
    ADD COLUMN IF NOT EXISTS webhook_secret_ciphertext BYTEA,
    ADD COLUMN IF NOT EXISTS webhook_secret_nonce BYTEA,
    ADD COLUMN IF NOT EXISTS webhook_secret_key_id VARCHAR(64);

CREATE UNIQUE INDEX IF NOT EXISTS uq_async_tasks_api_call
    ON async_inference_tasks(api_call_id)
    WHERE api_call_id IS NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'ck_async_task_upstream_urls'
    ) THEN
        ALTER TABLE async_inference_tasks
            ADD CONSTRAINT ck_async_task_upstream_urls CHECK (
                upstream_task_id IS NULL
                OR (
                    upstream_get_url IS NOT NULL
                    AND upstream_cancel_url IS NOT NULL
                    AND upstream_webhook_url IS NOT NULL
                )
            );
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'ck_async_task_webhook_secret'
    ) THEN
        ALTER TABLE async_inference_tasks
            ADD CONSTRAINT ck_async_task_webhook_secret CHECK (
                (
                    webhook_secret_ciphertext IS NULL
                    AND webhook_secret_nonce IS NULL
                    AND webhook_secret_key_id IS NULL
                )
                OR (
                    OCTET_LENGTH(webhook_secret_ciphertext) > 0
                    AND OCTET_LENGTH(webhook_secret_nonce) = 12
                    AND webhook_secret_key_id IS NOT NULL
                )
            );
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS async_webhook_events (
    event_id VARCHAR(255) PRIMARY KEY,
    task_id BIGINT NOT NULL REFERENCES async_inference_tasks(id) ON DELETE CASCADE,
    received_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_async_webhook_events_task
    ON async_webhook_events(task_id, received_at DESC);
