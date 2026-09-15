-- 001/002 中的模型价格本来就是 USD/每百万 Token。
-- 008 在切换计费公式时曾临时放大 1000 倍；这里精确纠正一次。

UPDATE provider_models
SET input_rate = input_rate / 1000,
    output_rate = output_rate / 1000,
    upstream_input_rate = upstream_input_rate / 1000,
    upstream_output_rate = upstream_output_rate / 1000,
    upstream_input_price_per_million = upstream_input_price_per_million / 1000,
    upstream_output_price_per_million = upstream_output_price_per_million / 1000;

UPDATE logical_models
SET input_price_per_million = input_price_per_million / 1000,
    output_price_per_million = output_price_per_million / 1000,
    cached_input_price_per_million = cached_input_price_per_million / 1000
WHERE cached_input_price_per_million IS NOT NULL;

UPDATE logical_models
SET input_price_per_million = input_price_per_million / 1000,
    output_price_per_million = output_price_per_million / 1000
WHERE cached_input_price_per_million IS NULL;
