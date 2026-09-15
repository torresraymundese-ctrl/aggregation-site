# Nexus Gateway 国外站

Nexus Gateway 是面向开发者的 AI API 聚合网关。用户通过一个兼容入口发现、比较和调用模型；未取得供应商书面转售授权前，默认使用用户自己的供应商 Key（BYOK）。

## 技术架构

- 后端：Rust + Axum，负责登录、API Key、余额、模型路由、扣费和调用日志。
- 数据库：PostgreSQL 保存用户、余额、供应商、模型、Key 和调用记录。
- 缓存：Redis 用于 API Key 小时限流。
- 前端：Vue 3 + Vite，提供首页、模型广场、控制台、Key、扫码充值、日志、文档和管理后台。
- 部署：Docker Compose + Nginx，生产配置走 `.env.production`。

## 本地运行方法

1. 启动数据库和 Redis：

```powershell
docker compose up -d postgres redis
```

2. 启动后端：

```powershell
cd overseas-api
cargo run
```

3. 启动前端：

```powershell
cd overseas-site
npm.cmd run dev -- --host 0.0.0.0 --port 5310
```

本机访问地址：`http://127.0.0.1:5310`；局域网设备访问：`http://<本机局域网 IP>:5310`。PostgreSQL 映射到 `55433`，Redis 映射到 `56380`，避免占用其他项目的默认端口。

## 部署方法

```powershell
docker compose -f docker-compose-deploy.yml up -d --build
```

新库初始化使用 `deploy/init-postgres.sql`，里面会依次执行表结构、基础数据和国内供应商占位配置。

## 环境变量

已预留这些上游 Key：

- `PROVIDER_OPENAI_API_KEY`
- `PROVIDER_ANTHROPIC_API_KEY`
- `PROVIDER_GOOGLE_API_KEY`
- `PROVIDER_VOLCENGINE_API_KEY`
- `PROVIDER_DEEPSEEK_API_KEY`
- `PROVIDER_ZHIPU_API_KEY`
- `PROVIDER_QWEN_API_KEY`
- `PROVIDER_MOONSHOT_API_KEY`
- `PROVIDER_MINIMAX_API_KEY`
- `PROVIDER_STEPFUN_API_KEY`
- `BYOK_MASTER_KEY_B64`：32 字节随机主密钥的 Base64，用于加密用户供应商 Key。
- `JWT_SECRET`：至少 32 个字符；缺失或过短时后端拒绝启动。
- `PAYMENT_WEBHOOK_SECRET`：支付回调 HMAC 验签密钥。
- `TRUST_PROXY_HEADERS`：只有后端明确位于受控 Nginx 后方时设为 `true`，用于读取 Nginx 覆盖写入的 `X-Real-IP`。

## 测试方法

```powershell
cd overseas-api
cargo test
```

```powershell
cd overseas-site
npm.cmd run build
```

```powershell
cd overseas-site
$env:APP_BASE='http://127.0.0.1:5310'
npm.cmd run smoke:local
```

本地冒烟会覆盖公开页、注册登录、余额、API Key、模型白名单、管理端模型定价、供应商配置、利润报表、审计日志和风控中心。脚本会删除临时 Key，并停用临时测试账号。

```powershell
python deploy/check-domestic-provider-placeholders.py
```

## 搜索记录

- 2026-06-24：skills.sh 搜索未找到可直接复用的项目技能，继续沿用当前 Rust/Vue 自建架构。
- 2026-06-24：GitHub 参考方向为多供应商网关和统一 API 管理，参考项目包括 [One API](https://github.com/songquanpeng/one-api)、[New API](https://github.com/QuantumNous/new-api)、[LiteLLM](https://github.com/BerriAI/litellm)。本项目没有复制外部代码，只确认了“统一余额 + 多 Provider + OpenAI 兼容入口”的方向合理。
- 2026-07-14：主对标 OpenRouter；模型发现参考 Hugging Face、SiliconFlow；异步任务参考 Replicate；团队治理参考 302.AI、阿里云百炼；后台设计参考 New API。仅参考产品和官方文档，没有复制外部代码。

## 2026-07-14 聚合网关升级

- 控制台只接受 JWT，模型调用只接受平台 API Key；已移除开发用户冒充入口。
- 支付回调强制验签，支付、入账和退款使用事务与幂等事件。
- 价格统一为美元/每 100 万 Token，费用公式为 `Token 数 × 单价 ÷ 1,000,000`。
- 逻辑模型和供应商接入点分离；按固定、最低价格、最低延迟或最高稳定性在调用前选择一次，上游失败不二次改道。
- 新增请求编号、实际上游、路由原因、供应商成本和用户费用账本。
- 模型广场支持公开浏览、搜索、筛选、排序、最多三个模型比较、在线测试和 Curl/Python/Node 示例。
- 新增工作空间隔离、API Key 有效期、模型/IP 白名单、日/月/总预算和 BYOK 加密凭据。
- 新增工作空间切换、成员管理员/普通成员角色、项目管理和项目级 API Key 归属；退出登录会让旧 JWT 立即失效。
- 国内占位供应商默认禁用；禁用供应商不会进入路由或 BYOK 列表。
- OpenAI 接入点支持真实 SSE；在最终 usage 完成原子结算后才发送 `[DONE]`。Claude、Gemini 和未经验证的兼容接入点对 `stream=true` 明确返回 501。
- Claude/Gemini 已兼容函数工具调用、JSON Schema 结构化输出和图片输入；不兼容参数返回 400，不静默忽略。
- `/v1/responses` 支持非流式兼容适配；Embeddings 和图片/音频/视频路由在没有真实接入点时明确返回 501，不创建假任务。

## 已完成功能

- 用户登录、余额、套餐、API Key、模型白名单、限流、日消费上限。
- OpenAI、Anthropic Claude、Google Gemini 上游转发。
- 国内 OpenAI 兼容 Provider 架构占位：火山引擎、DeepSeek、智谱 GLM、通义千问、Kimi、MiniMax、阶跃星辰。
- 管理端模型定价配置：维护上游成本价、平台余量和用户最终扣费价。
- 管理端供应商运营：查看 Key 环境变量状态、上游地址、24 小时健康和供应商启停。
- 运营风控：按最近 10 分钟真实失败率自动熔断异常供应商。
- 管理端利润报表：按最终扣费价和上游成本价计算 30 天毛利。
- 管理端审计日志：记录模型价格和供应商配置变更。
- 管理端风控中心：按真实调用量、失败率、24 小时消费和 Key 数量查看用户风险。
- 静态二维码充值：用户创建订单后扫码付款，管理员确认到账后为账户加余额。
- OpenAPI 规格入口：`/api/openapi.json` 可导入 Apifox、Postman、Swagger 或 SDK 生成工具。
- 首页模型星球和模型广场展示。
- 生产部署、HTTPS、Nginx 安全规则和冒烟脚本。

## 待办事项

- 支付并发集成测试。
- Claude/Gemini SSE 转换与更多 OpenAI 兼容供应商的流式验证。
- 配置真实 Embeddings 以及图片/音频/视频异步供应商接入点。
- 取得供应商书面授权后，才允许把对应供应商切换为平台统一余额模式。
