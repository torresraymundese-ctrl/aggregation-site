-- 并发预算预留：按日、月、总额分别保存已消费与处理中金额。
-- 所有预留和结算都先锁定 api_keys 行，再更新这些周期账本和工作空间余额。

CREATE TABLE IF NOT EXISTS api_key_budget_periods (
    api_key_id BIGINT NOT NULL REFERENCES api_keys(id) ON DELETE CASCADE,
    period_type VARCHAR(8) NOT NULL CHECK (period_type IN ('day', 'month', 'total')),
    period_start DATE NOT NULL,
    spent_usd DECIMAL(20,8) NOT NULL DEFAULT 0 CHECK (spent_usd >= 0),
    reserved_usd DECIMAL(20,8) NOT NULL DEFAULT 0 CHECK (reserved_usd >= 0),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW(),
    PRIMARY KEY (api_key_id, period_type, period_start)
);

CREATE INDEX IF NOT EXISTS idx_api_key_budget_periods_lookup
    ON api_key_budget_periods(api_key_id, period_type, period_start);

-- 只回填已经成功且真实产生用户费用的请求，失败请求不得占用预算。
INSERT INTO api_key_budget_periods (api_key_id, period_type, period_start, spent_usd)
SELECT api_key_id, 'day', CURRENT_DATE, COALESCE(SUM(user_cost_usd), 0)
FROM api_calls
WHERE api_key_id IS NOT NULL
  AND status_code BETWEEN 200 AND 299
  AND created_at >= CURRENT_DATE
GROUP BY api_key_id
ON CONFLICT (api_key_id, period_type, period_start)
DO UPDATE SET spent_usd = EXCLUDED.spent_usd, updated_at = NOW();

INSERT INTO api_key_budget_periods (api_key_id, period_type, period_start, spent_usd)
SELECT api_key_id, 'month', DATE_TRUNC('month', CURRENT_DATE)::DATE, COALESCE(SUM(user_cost_usd), 0)
FROM api_calls
WHERE api_key_id IS NOT NULL
  AND status_code BETWEEN 200 AND 299
  AND created_at >= DATE_TRUNC('month', CURRENT_DATE)
GROUP BY api_key_id
ON CONFLICT (api_key_id, period_type, period_start)
DO UPDATE SET spent_usd = EXCLUDED.spent_usd, updated_at = NOW();

INSERT INTO api_key_budget_periods (api_key_id, period_type, period_start, spent_usd)
SELECT api_key_id, 'total', DATE '1970-01-01', COALESCE(SUM(user_cost_usd), 0)
FROM api_calls
WHERE api_key_id IS NOT NULL
  AND status_code BETWEEN 200 AND 299
GROUP BY api_key_id
ON CONFLICT (api_key_id, period_type, period_start)
DO UPDATE SET spent_usd = EXCLUDED.spent_usd, updated_at = NOW();

ALTER TABLE spend_reservations
    ADD COLUMN IF NOT EXISTS day_bucket DATE,
    ADD COLUMN IF NOT EXISTS month_bucket DATE;

UPDATE spend_reservations
SET day_bucket = created_at::DATE,
    month_bucket = DATE_TRUNC('month', created_at)::DATE
WHERE day_bucket IS NULL OR month_bucket IS NULL;

ALTER TABLE spend_reservations
    ALTER COLUMN day_bucket SET NOT NULL,
    ALTER COLUMN month_bucket SET NOT NULL;

CREATE INDEX IF NOT EXISTS idx_spend_reservations_expiry
    ON spend_reservations(api_key_id, expires_at)
    WHERE state = 'reserved';

UPDATE balances
SET balance = COALESCE(balance, 0),
    frozen_balance = COALESCE(frozen_balance, 0),
    total_consumed = COALESCE(total_consumed, 0);

ALTER TABLE balances
    ALTER COLUMN balance SET NOT NULL,
    ALTER COLUMN frozen_balance SET NOT NULL,
    ALTER COLUMN total_consumed SET NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'chk_balances_non_negative'
    ) THEN
        ALTER TABLE balances
            ADD CONSTRAINT chk_balances_non_negative
            CHECK (balance >= 0 AND frozen_balance >= 0 AND total_consumed >= 0);
    END IF;
END $$;
