-- 登录令牌版本。退出登录时递增，旧 JWT 立即失效。
ALTER TABLE overseas_users
    ADD COLUMN IF NOT EXISTS token_version BIGINT NOT NULL DEFAULT 0;

