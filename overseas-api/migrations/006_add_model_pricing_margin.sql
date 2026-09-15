-- 模型定价配置。
-- upstream_* 是供应商成本价，margin_rate 是平台余量百分比。
-- 现有 input_rate/output_rate 继续作为用户实际扣费价，避免破坏现有调用链路。

ALTER TABLE provider_models
    ADD COLUMN IF NOT EXISTS upstream_input_rate DECIMAL(18,6),
    ADD COLUMN IF NOT EXISTS upstream_output_rate DECIMAL(18,6),
    ADD COLUMN IF NOT EXISTS margin_rate DECIMAL(8,4) DEFAULT 0;

UPDATE provider_models
SET
    upstream_input_rate = COALESCE(upstream_input_rate, input_rate),
    upstream_output_rate = COALESCE(upstream_output_rate, output_rate),
    margin_rate = COALESCE(margin_rate, 0);
