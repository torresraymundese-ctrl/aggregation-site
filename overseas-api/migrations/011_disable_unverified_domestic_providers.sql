-- 国内供应商尚未完成真实模型、价格和授权验证，升级时统一关闭。
-- 管理员完成逐家验证后再显式启用，避免把占位数据展示为可用。

UPDATE providers
SET status = 1,
    updated_at = NOW()
WHERE provider_id IN (
    'volcengine', 'deepseek', 'zhipu', 'qwen',
    'moonshot', 'minimax', 'stepfun'
);

UPDATE provider_models pm
SET status = 1,
    updated_at = NOW()
FROM providers p
WHERE pm.provider_id = p.id
  AND p.provider_id IN (
      'volcengine', 'deepseek', 'zhipu', 'qwen',
      'moonshot', 'minimax', 'stepfun'
  );

UPDATE logical_models lm
SET status = 1,
    updated_at = NOW()
WHERE NOT EXISTS (
    SELECT 1
    FROM provider_models pm
    JOIN providers p ON p.id = pm.provider_id
    WHERE pm.logical_model_id = lm.id
      AND pm.status = 0
      AND p.status = 0
);
