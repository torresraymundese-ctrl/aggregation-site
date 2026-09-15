-- Login lock fields used by the auth login handler.

ALTER TABLE overseas_users
ADD COLUMN IF NOT EXISTS failed_login_attempts SMALLINT DEFAULT 0,
ADD COLUMN IF NOT EXISTS locked_until TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_overseas_users_locked_until ON overseas_users(locked_until);
