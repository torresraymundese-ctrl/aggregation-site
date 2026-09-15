-- 团队、项目、预算和 BYOK 基础结构。
-- provider_credentials 只保存 AES-GCM 密文、nonce 和指纹，禁止保存明文 Key。

CREATE TABLE IF NOT EXISTS workspaces (
    id BIGSERIAL PRIMARY KEY,
    uid VARCHAR(64) UNIQUE NOT NULL,
    name VARCHAR(100) NOT NULL,
    owner_user_id BIGINT NOT NULL REFERENCES overseas_users(id),
    status SMALLINT NOT NULL DEFAULT 0 CHECK (status IN (0, 1)),
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS workspace_members (
    workspace_id BIGINT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    user_id BIGINT NOT NULL REFERENCES overseas_users(id) ON DELETE CASCADE,
    role VARCHAR(16) NOT NULL CHECK (role IN ('admin', 'member')),
    status SMALLINT NOT NULL DEFAULT 0 CHECK (status IN (0, 1)),
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    PRIMARY KEY (workspace_id, user_id)
);

CREATE TABLE IF NOT EXISTS projects (
    id BIGSERIAL PRIMARY KEY,
    uid VARCHAR(64) UNIQUE NOT NULL,
    workspace_id BIGINT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name VARCHAR(100) NOT NULL,
    status SMALLINT NOT NULL DEFAULT 0 CHECK (status IN (0, 1)),
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    UNIQUE(workspace_id, name)
);

CREATE INDEX IF NOT EXISTS idx_workspaces_owner_status
    ON workspaces(owner_user_id, status);
CREATE INDEX IF NOT EXISTS idx_workspace_members_user_status
    ON workspace_members(user_id, status);
CREATE INDEX IF NOT EXISTS idx_projects_workspace_status
    ON projects(workspace_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS uq_projects_id_workspace
    ON projects(id, workspace_id);

INSERT INTO workspaces (uid, name, owner_user_id, status)
SELECT 'ws_' || u.uid, COALESCE(NULLIF(u.nickname, ''), u.email), u.id, 0
FROM overseas_users u
ON CONFLICT (uid) DO NOTHING;

INSERT INTO workspace_members (workspace_id, user_id, role, status)
SELECT w.id, w.owner_user_id, 'admin', 0
FROM workspaces w
ON CONFLICT (workspace_id, user_id) DO NOTHING;

INSERT INTO projects (uid, workspace_id, name, status)
SELECT 'prj_' || u.uid, w.id, 'Default', 0
FROM overseas_users u
JOIN workspaces w ON w.uid = 'ws_' || u.uid
ON CONFLICT (uid) DO NOTHING;

ALTER TABLE api_keys
    ADD COLUMN IF NOT EXISTS workspace_id BIGINT,
    ADD COLUMN IF NOT EXISTS project_id BIGINT,
    ADD COLUMN IF NOT EXISTS expires_at TIMESTAMP,
    ADD COLUMN IF NOT EXISTS daily_spend_limit DECIMAL(18,8),
    ADD COLUMN IF NOT EXISTS monthly_spend_limit DECIMAL(20,8),
    ADD COLUMN IF NOT EXISTS total_spend_limit DECIMAL(20,8),
    ADD COLUMN IF NOT EXISTS ip_allowlist CIDR[] NOT NULL DEFAULT '{}'::cidr[];

ALTER TABLE api_keys
    ALTER COLUMN daily_spend_limit TYPE DECIMAL(20,8);

UPDATE api_keys k
SET workspace_id = w.id,
    project_id = p.id
FROM overseas_users u
JOIN workspaces w ON w.uid = 'ws_' || u.uid
JOIN projects p ON p.workspace_id = w.id AND p.name = 'Default'
WHERE k.user_id = u.id
  AND (k.workspace_id IS NULL OR k.project_id IS NULL);

ALTER TABLE api_keys
    ALTER COLUMN workspace_id SET NOT NULL,
    ALTER COLUMN project_id SET NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_api_keys_workspace'
    ) THEN
        ALTER TABLE api_keys
            ADD CONSTRAINT fk_api_keys_workspace
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_api_keys_project'
    ) THEN
        ALTER TABLE api_keys
            ADD CONSTRAINT fk_api_keys_project
            FOREIGN KEY (project_id) REFERENCES projects(id);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_api_keys_project_workspace'
    ) THEN
        ALTER TABLE api_keys
            ADD CONSTRAINT fk_api_keys_project_workspace
            FOREIGN KEY (project_id, workspace_id) REFERENCES projects(id, workspace_id);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_api_keys_workspace_member'
    ) THEN
        ALTER TABLE api_keys
            ADD CONSTRAINT fk_api_keys_workspace_member
            FOREIGN KEY (workspace_id, user_id) REFERENCES workspace_members(workspace_id, user_id);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_api_keys_spend_limits'
    ) THEN
        ALTER TABLE api_keys
            ADD CONSTRAINT chk_api_keys_spend_limits
            CHECK (
                (daily_spend_limit IS NULL OR daily_spend_limit > 0)
                AND (monthly_spend_limit IS NULL OR monthly_spend_limit > 0)
                AND (total_spend_limit IS NULL OR total_spend_limit > 0)
            );
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_api_keys_workspace_project_status
    ON api_keys(workspace_id, project_id, status);
CREATE INDEX IF NOT EXISTS idx_api_keys_active_expiry
    ON api_keys(expires_at)
    WHERE status = 0 AND expires_at IS NOT NULL;

CREATE TABLE IF NOT EXISTS api_key_budget_usage (
    api_key_id BIGINT PRIMARY KEY REFERENCES api_keys(id) ON DELETE CASCADE,
    day_bucket DATE NOT NULL DEFAULT CURRENT_DATE,
    month_bucket DATE NOT NULL DEFAULT (DATE_TRUNC('month', CURRENT_DATE)::DATE),
    day_spent DECIMAL(20,8) NOT NULL DEFAULT 0 CHECK (day_spent >= 0),
    month_spent DECIMAL(20,8) NOT NULL DEFAULT 0 CHECK (month_spent >= 0),
    total_spent DECIMAL(20,8) NOT NULL DEFAULT 0 CHECK (total_spent >= 0),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

INSERT INTO api_key_budget_usage (
    api_key_id,
    day_bucket,
    month_bucket,
    day_spent,
    month_spent,
    total_spent
)
SELECT
    k.id,
    CURRENT_DATE,
    DATE_TRUNC('month', CURRENT_DATE)::DATE,
    COALESCE(SUM(CASE
        WHEN ac.status_code BETWEEN 200 AND 299
         AND ac.created_at >= CURRENT_DATE
        THEN ac.user_cost_usd ELSE 0 END), 0),
    COALESCE(SUM(CASE
        WHEN ac.status_code BETWEEN 200 AND 299
         AND ac.created_at >= DATE_TRUNC('month', CURRENT_DATE)
        THEN ac.user_cost_usd ELSE 0 END), 0),
    COALESCE(SUM(CASE
        WHEN ac.status_code BETWEEN 200 AND 299
        THEN ac.user_cost_usd ELSE 0 END), 0)
FROM api_keys k
LEFT JOIN api_calls ac ON ac.api_key_id = k.id
GROUP BY k.id
ON CONFLICT (api_key_id) DO NOTHING;

CREATE TABLE IF NOT EXISTS spend_reservations (
    id BIGSERIAL PRIMARY KEY,
    api_call_id BIGINT UNIQUE NOT NULL REFERENCES api_calls(id) ON DELETE CASCADE,
    api_key_id BIGINT NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    reserved_usd DECIMAL(20,8) NOT NULL CHECK (reserved_usd >= 0),
    actual_usd DECIMAL(20,8) CHECK (actual_usd >= 0 AND actual_usd <= reserved_usd),
    state VARCHAR(16) NOT NULL DEFAULT 'reserved'
        CHECK (state IN ('reserved', 'settled', 'released')),
    expires_at TIMESTAMP NOT NULL,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    settled_at TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_spend_reservations_active
    ON spend_reservations(api_key_id, expires_at)
    WHERE state = 'reserved';

ALTER TABLE balances
    ADD COLUMN IF NOT EXISTS workspace_id BIGINT;

UPDATE balances b
SET workspace_id = w.id
FROM overseas_users u
JOIN workspaces w ON w.uid = 'ws_' || u.uid
WHERE b.user_id = u.id
  AND b.workspace_id IS NULL;

ALTER TABLE balances
    ALTER COLUMN workspace_id SET NOT NULL;

-- 余额从此按工作空间唯一；user_id 仅保留为所有者兼容字段。
ALTER TABLE balances
    DROP CONSTRAINT IF EXISTS balances_user_id_key;

CREATE INDEX IF NOT EXISTS idx_balances_user_id
    ON balances(user_id);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_balances_workspace'
    ) THEN
        ALTER TABLE balances
            ADD CONSTRAINT fk_balances_workspace
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id);
    END IF;


    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_balances_workspace_member'
    ) THEN
        ALTER TABLE balances
            ADD CONSTRAINT fk_balances_workspace_member
            FOREIGN KEY (workspace_id, user_id) REFERENCES workspace_members(workspace_id, user_id);
    END IF;
END $$;

CREATE UNIQUE INDEX IF NOT EXISTS uq_balances_workspace
    ON balances(workspace_id);

ALTER TABLE overseas_orders
    ADD COLUMN IF NOT EXISTS workspace_id BIGINT;

UPDATE overseas_orders o
SET workspace_id = w.id
FROM overseas_users u
JOIN workspaces w ON w.uid = 'ws_' || u.uid
WHERE o.user_id = u.id
  AND o.workspace_id IS NULL;

ALTER TABLE overseas_orders
    ALTER COLUMN workspace_id SET NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_overseas_orders_workspace'
    ) THEN
        ALTER TABLE overseas_orders
            ADD CONSTRAINT fk_overseas_orders_workspace
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id);
    END IF;


    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_overseas_orders_workspace_member'
    ) THEN
        ALTER TABLE overseas_orders
            ADD CONSTRAINT fk_overseas_orders_workspace_member
            FOREIGN KEY (workspace_id, user_id) REFERENCES workspace_members(workspace_id, user_id);
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_overseas_orders_workspace_created_at
    ON overseas_orders(workspace_id, created_at DESC);

ALTER TABLE balance_logs
    ADD COLUMN IF NOT EXISTS workspace_id BIGINT,
    ADD COLUMN IF NOT EXISTS project_id BIGINT,
    ADD COLUMN IF NOT EXISTS api_call_id BIGINT;

UPDATE balance_logs bl
SET workspace_id = w.id
FROM overseas_users u
JOIN workspaces w ON w.uid = 'ws_' || u.uid
WHERE bl.user_id = u.id
  AND bl.workspace_id IS NULL;

UPDATE balance_logs bl
SET project_id = p.id
FROM projects p
WHERE bl.workspace_id = p.workspace_id
  AND p.name = 'Default'
  AND bl.project_id IS NULL;

ALTER TABLE balance_logs
    ALTER COLUMN workspace_id SET NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_balance_logs_workspace'
    ) THEN
        ALTER TABLE balance_logs
            ADD CONSTRAINT fk_balance_logs_workspace
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_balance_logs_project'
    ) THEN
        ALTER TABLE balance_logs
            ADD CONSTRAINT fk_balance_logs_project
            FOREIGN KEY (project_id) REFERENCES projects(id);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_balance_logs_project_workspace'
    ) THEN
        ALTER TABLE balance_logs
            ADD CONSTRAINT fk_balance_logs_project_workspace
            FOREIGN KEY (project_id, workspace_id) REFERENCES projects(id, workspace_id);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_balance_logs_api_call'
    ) THEN
        ALTER TABLE balance_logs
            ADD CONSTRAINT fk_balance_logs_api_call
            FOREIGN KEY (api_call_id) REFERENCES api_calls(id);
    END IF;


    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_balance_logs_workspace_member'
    ) THEN
        ALTER TABLE balance_logs
            ADD CONSTRAINT fk_balance_logs_workspace_member
            FOREIGN KEY (workspace_id, user_id) REFERENCES workspace_members(workspace_id, user_id);
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_balance_logs_workspace_created_at
    ON balance_logs(workspace_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_balance_logs_project_created_at
    ON balance_logs(project_id, created_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS uq_balance_logs_api_call
    ON balance_logs(api_call_id)
    WHERE api_call_id IS NOT NULL;

ALTER TABLE api_calls
    ADD COLUMN IF NOT EXISTS workspace_id BIGINT,
    ADD COLUMN IF NOT EXISTS project_id BIGINT,
    ADD COLUMN IF NOT EXISTS provider_credential_id BIGINT;

UPDATE api_calls ac
SET workspace_id = k.workspace_id,
    project_id = k.project_id
FROM api_keys k
WHERE ac.api_key_id = k.id
  AND (ac.workspace_id IS NULL OR ac.project_id IS NULL);

UPDATE api_calls ac
SET workspace_id = w.id,
    project_id = p.id
FROM overseas_users u
JOIN workspaces w ON w.uid = 'ws_' || u.uid
JOIN projects p ON p.workspace_id = w.id AND p.name = 'Default'
WHERE ac.user_id = u.id
  AND (ac.workspace_id IS NULL OR ac.project_id IS NULL);

ALTER TABLE api_calls
    ALTER COLUMN workspace_id SET NOT NULL,
    ALTER COLUMN project_id SET NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_api_calls_workspace'
    ) THEN
        ALTER TABLE api_calls
            ADD CONSTRAINT fk_api_calls_workspace
            FOREIGN KEY (workspace_id) REFERENCES workspaces(id);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_api_calls_project'
    ) THEN
        ALTER TABLE api_calls
            ADD CONSTRAINT fk_api_calls_project
            FOREIGN KEY (project_id) REFERENCES projects(id);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_api_calls_project_workspace'
    ) THEN
        ALTER TABLE api_calls
            ADD CONSTRAINT fk_api_calls_project_workspace
            FOREIGN KEY (project_id, workspace_id) REFERENCES projects(id, workspace_id);
    END IF;


    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_api_calls_workspace_member'
    ) THEN
        ALTER TABLE api_calls
            ADD CONSTRAINT fk_api_calls_workspace_member
            FOREIGN KEY (workspace_id, user_id) REFERENCES workspace_members(workspace_id, user_id);
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_api_calls_workspace_created_at
    ON api_calls(workspace_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_api_calls_project_created_at
    ON api_calls(project_id, created_at DESC);

ALTER TABLE providers
    ADD COLUMN IF NOT EXISTS credential_mode VARCHAR(32) NOT NULL DEFAULT 'byok';

UPDATE providers
SET credential_mode = 'byok'
WHERE credential_mode IS NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_providers_credential_mode'
    ) THEN
        ALTER TABLE providers
            ADD CONSTRAINT chk_providers_credential_mode
            CHECK (credential_mode IN ('platform_authorized', 'byok'));
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS provider_credentials (
    id BIGSERIAL PRIMARY KEY,
    uid VARCHAR(64) UNIQUE NOT NULL,
    workspace_id BIGINT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    provider_id BIGINT NOT NULL REFERENCES providers(id) ON DELETE CASCADE,
    name VARCHAR(100) NOT NULL,
    key_prefix VARCHAR(20) NOT NULL,
    key_fingerprint VARCHAR(64) NOT NULL CHECK (LENGTH(key_fingerprint) = 64),
    ciphertext BYTEA NOT NULL CHECK (OCTET_LENGTH(ciphertext) > 0),
    nonce BYTEA NOT NULL CHECK (OCTET_LENGTH(nonce) = 12),
    encryption_key_id VARCHAR(64) NOT NULL,
    status SMALLINT NOT NULL DEFAULT 0 CHECK (status IN (0, 1)),
    created_by BIGINT NOT NULL REFERENCES overseas_users(id),
    last_used_at TIMESTAMP,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    UNIQUE(workspace_id, provider_id)
);

CREATE INDEX IF NOT EXISTS idx_provider_credentials_workspace_status
    ON provider_credentials(workspace_id, status);
CREATE INDEX IF NOT EXISTS idx_provider_credentials_provider_status
    ON provider_credentials(provider_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS uq_provider_credentials_id_workspace
    ON provider_credentials(id, workspace_id);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_api_calls_provider_credential'
    ) THEN
        ALTER TABLE api_calls
            ADD CONSTRAINT fk_api_calls_provider_credential
            FOREIGN KEY (provider_credential_id) REFERENCES provider_credentials(id);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_api_calls_provider_credential_workspace'
    ) THEN
        ALTER TABLE api_calls
            ADD CONSTRAINT fk_api_calls_provider_credential_workspace
            FOREIGN KEY (provider_credential_id, workspace_id)
            REFERENCES provider_credentials(id, workspace_id);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_request_attempts_provider_credential'
    ) THEN
        ALTER TABLE request_attempts
            ADD CONSTRAINT fk_request_attempts_provider_credential
            FOREIGN KEY (provider_credential_id) REFERENCES provider_credentials(id);
    END IF;


    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'fk_provider_credentials_workspace_member'
    ) THEN
        ALTER TABLE provider_credentials
            ADD CONSTRAINT fk_provider_credentials_workspace_member
            FOREIGN KEY (workspace_id, created_by) REFERENCES workspace_members(workspace_id, user_id);
    END IF;
END $$;
