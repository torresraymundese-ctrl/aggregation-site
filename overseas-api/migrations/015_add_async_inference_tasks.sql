-- 异步图片、音频、视频任务只保存运行元数据，不保存提示词和上游响应正文。
-- 在真实上游适配器启用前，创建接口返回 501，不创建假任务。
CREATE TABLE IF NOT EXISTS async_inference_tasks (
    id BIGSERIAL PRIMARY KEY,
    uid VARCHAR(40) UNIQUE NOT NULL,
    request_id VARCHAR(40) UNIQUE NOT NULL,
    workspace_id BIGINT NOT NULL REFERENCES workspaces(id),
    project_id BIGINT NOT NULL,
    api_key_id BIGINT NOT NULL REFERENCES api_keys(id),
    user_id BIGINT NOT NULL REFERENCES overseas_users(id),
    task_type VARCHAR(16) NOT NULL
        CHECK (task_type IN ('image', 'audio', 'video')),
    model_id VARCHAR(100) NOT NULL,
    provider_model_id BIGINT REFERENCES provider_models(id),
    provider_credential_id BIGINT REFERENCES provider_credentials(id),
    input_sha256 CHAR(64) NOT NULL,
    upstream_task_id VARCHAR(255),
    state VARCHAR(16) NOT NULL DEFAULT 'queued'
        CHECK (state IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    progress SMALLINT NOT NULL DEFAULT 0 CHECK (progress BETWEEN 0 AND 100),
    result_url TEXT,
    error_code VARCHAR(80),
    retryable BOOLEAN NOT NULL DEFAULT FALSE,
    webhook_url TEXT,
    webhook_delivered_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    started_at TIMESTAMP,
    completed_at TIMESTAMP,
    expires_at TIMESTAMP NOT NULL,
    CONSTRAINT fk_async_task_project_workspace
        FOREIGN KEY (project_id, workspace_id) REFERENCES projects(id, workspace_id),
    CONSTRAINT ck_async_task_terminal_state CHECK (
        (state IN ('succeeded', 'failed', 'cancelled') AND completed_at IS NOT NULL)
        OR (state IN ('queued', 'running') AND completed_at IS NULL)
    ),
    CONSTRAINT ck_async_task_result CHECK (
        state <> 'succeeded' OR (result_url IS NOT NULL AND progress = 100)
    ),
    CONSTRAINT ck_async_task_failure CHECK (
        state <> 'failed' OR error_code IS NOT NULL
    )
);

CREATE INDEX IF NOT EXISTS idx_async_tasks_workspace_created
    ON async_inference_tasks(workspace_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_async_tasks_api_key_state
    ON async_inference_tasks(api_key_id, state, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_async_tasks_upstream
    ON async_inference_tasks(provider_model_id, upstream_task_id)
    WHERE upstream_task_id IS NOT NULL;
