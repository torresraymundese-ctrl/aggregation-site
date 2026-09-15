-- 国内模型供应商占位配置。
-- 这些记录用于提前打通模型目录、Key 白名单和 Provider 路由。
-- 正式销售前需要把占位模型 ID 和价格替换成供应商确认数据。

INSERT INTO providers (
    provider_id,
    name,
    base_url,
    api_key_env,
    status,
    sort
) VALUES
    ('volcengine', 'Volcengine', 'https://ark.cn-beijing.volces.com/api/v3', 'PROVIDER_VOLCENGINE_API_KEY', 1, 40),
    ('deepseek', 'DeepSeek', 'https://api.deepseek.com/v1', 'PROVIDER_DEEPSEEK_API_KEY', 1, 50),
    ('zhipu', 'Zhipu GLM', 'https://open.bigmodel.cn/api/paas/v4', 'PROVIDER_ZHIPU_API_KEY', 1, 60),
    ('qwen', 'Alibaba Qwen', 'https://dashscope.aliyuncs.com/compatible-mode/v1', 'PROVIDER_QWEN_API_KEY', 1, 70),
    ('moonshot', 'Moonshot Kimi', 'https://api.moonshot.cn/v1', 'PROVIDER_MOONSHOT_API_KEY', 1, 80),
    ('minimax', 'MiniMax', 'https://api.minimax.io/v1', 'PROVIDER_MINIMAX_API_KEY', 1, 90),
    ('stepfun', 'StepFun', 'https://api.stepfun.com/v1', 'PROVIDER_STEPFUN_API_KEY', 1, 100)
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
        ('volcengine', 'doubao-placeholder-chat', 'doubao-placeholder-chat', 'Doubao Chat', 0.500000, 1.500000, 128000, 8192, 1, 110),
        ('volcengine', 'doubao-placeholder-lite', 'doubao-placeholder-lite', 'Doubao Lite', 0.150000, 0.500000, 128000, 8192, 1, 120),
        ('deepseek', 'deepseek-chat', 'deepseek-chat', 'DeepSeek Chat', 0.300000, 1.200000, 128000, 8192, 1, 130),
        ('deepseek', 'deepseek-reasoner', 'deepseek-reasoner', 'DeepSeek Reasoner', 0.800000, 2.400000, 128000, 8192, 1, 140),
        ('zhipu', 'glm-placeholder-plus', 'glm-placeholder-plus', 'GLM Plus', 0.500000, 1.500000, 128000, 8192, 1, 150),
        ('zhipu', 'glm-placeholder-air', 'glm-placeholder-air', 'GLM Air', 0.150000, 0.500000, 128000, 8192, 1, 160),
        ('qwen', 'qwen-placeholder-max', 'qwen-placeholder-max', 'Qwen Max', 0.500000, 1.500000, 128000, 8192, 1, 170),
        ('qwen', 'qwen-placeholder-plus', 'qwen-placeholder-plus', 'Qwen Plus', 0.200000, 0.800000, 128000, 8192, 1, 180),
        ('moonshot', 'kimi-placeholder-chat', 'kimi-placeholder-chat', 'Kimi Chat', 0.500000, 1.500000, 128000, 8192, 1, 190),
        ('minimax', 'minimax-placeholder-chat', 'minimax-placeholder-chat', 'MiniMax Chat', 0.500000, 1.500000, 128000, 8192, 1, 200),
        ('stepfun', 'step-placeholder-chat', 'step-placeholder-chat', 'StepFun Chat', 0.500000, 1.500000, 128000, 8192, 1, 210)
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
