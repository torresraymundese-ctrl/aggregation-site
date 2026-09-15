# 架构说明

## 文件职责

- `overseas-api/src/main.rs`：后端入口、路由、模型转发、扣费和日志写入。
- `overseas-api/src/config.rs`：读取数据库、Redis、JWT、CORS 和 Provider Key 环境变量。
- `overseas-api/src/utils/error.rs`：统一处理后端错误脱敏。
- `overseas-api/src/models/`：数据库模型结构。
- `overseas-api/src/repository/`：用户、Key 等数据读写。
- `overseas-api/migrations/`：数据库建表和种子数据。
- `overseas-api/migrations/005_seed_domestic_providers.sql`：国内供应商和占位模型配置。
- `overseas-api/migrations/006_add_model_pricing_margin.sql`：模型成本价、平台余量和最终售价字段。
- `overseas-api/migrations/007_add_provider_ops_and_audit.sql`：管理端审计日志。
- `overseas-api/migrations/008-012`：资金加固、逻辑模型、多接入点、工作空间、BYOK、供应商禁用和价格尺度纠正。
- `overseas-api/src/byok.rs`：供应商 Key 的 AES-256-GCM 加密、解密、前缀和指纹。
- `overseas-site/src/views/Home.vue`：首页和模型星球。
- `overseas-site/src/router/index.js`：前端路由和登录/管理员访问拦截。
- `overseas-site/src/views/Models.vue`：模型广场。
- `overseas-site/src/views/Settings.vue`：工作空间内的 BYOK 凭据管理。
- `overseas-site/src/views/Team.vue`：工作空间、成员角色和项目管理。
- `overseas-site/src/components/ShellLayout.vue`：登录控制台和未登录公开页的两种布局。
- `overseas-site/src/views/AdminModels.vue`：管理端模型定价配置页。
- `overseas-site/src/views/AdminProviders.vue`：管理端供应商、Key 环境变量和健康状态页。
- `overseas-site/src/views/AdminReports.vue`：管理端利润报表页。
- `overseas-site/src/views/AdminRisk.vue`：管理端用户风险观察页。
- `overseas-site/src/views/AdminAuditLogs.vue`：管理端操作审计页。
- `overseas-site/src/components/PaymentQrModal.vue`：扫码付款弹窗，展示 IOTPay 静态收款码、订单号和应付金额。
- `overseas-site/public/payment/openbridge-iotpay-qr.jpg`：OpenBridge Technology Inc. IOTPay 静态收款码图片。
- `overseas-site/src/views/Docs.vue`：接口文档页，提供 OpenAPI 规格入口和调用示例。
- `overseas-site/scripts/smoke-local.cjs`：本地全流程冒烟脚本，覆盖用户、Key、模型、管理端和风控。
- `overseas-site/src/styles/global.css`：全站视觉样式。
- `docker-compose.yml`：本机开发用 PostgreSQL、Redis 和后端容器；数据库与缓存分别映射到主机 55433、56380。
- `deploy/init-postgres.sql`：新环境初始化数据库。
- `deploy/check-domestic-provider-placeholders.py`：检查国内供应商占位模型和 Key 环境变量。

## 调用关系

1. 前端通过 `/api/models` 读取模型列表。
2. 用户创建平台 API Key。
3. 用户调用 `/api/v1/chat/completions`，请求体里的 `model` 决定使用哪个模型。
4. 后端查询 `provider_models` 找到模型价格和供应商。
5. 后端查询 `providers` 找到上游地址和对应环境变量名。
6. 上游调用成功后，后端按输入/输出 token 扣统一余额。
7. 后端写入 `api_calls`，前端日志页读取展示。
8. 管理端通过 `/api/admin/models` 读取模型定价，通过 `/api/admin/models/:id/pricing` 更新成本价、余量和最终售价。
9. 管理端通过 `/api/admin/providers` 管理供应商；健康状态来自最近 24 小时真实调用日志。
10. 中继调用前会检查供应商最近 10 分钟失败率，达到阈值会返回 `PROVIDER_CIRCUIT_OPEN`，不进入上游、不扣费。
11. 管理端通过 `/api/admin/reports/profit` 查看 30 天收入、上游成本和毛利；通过 `/api/admin/audit-logs` 查看管理操作记录。
12. 管理端通过 `/api/admin/risk/users` 查看用户风险，风险来自真实调用量、失败率、消费和账号状态。
13. 开发者通过 `/api/openapi.json` 获取 OpenAPI 规格，用于导入接口调试工具或生成 SDK。
14. 用户在充值页创建订单后扫码付款；管理员在订单页确认到账，后端把订单改为已支付并给用户余额加值。

## 2026-07-14 核心调用关系

1. API Key 中间件校验用户、工作空间、项目、有效期、IP 白名单和预算字段。
2. 后端按请求模型查找逻辑模型，只在调用前选择一个健康且已启用的等价接入点。
3. BYOK 接入点从当前工作空间读取密文，内存解密后调用官方 HTTPS 域名；平台不保存明文。
4. 上游成功或失败都写 `api_calls` 和 `request_attempts`，记录请求编号、路由原因、耗时、Token 和成本。
5. 上游失败直接返回，不切换接入点，不替换模型。
6. 控制台用 `X-Nexus-Workspace` 选择当前工作空间；后端同时校验登录用户成员关系、路径 UID 和数据库 `workspace_id`。
7. 工作空间管理员可维护成员和项目，普通成员只读；Default 项目和工作空间所有者不可被误停用。
8. 请求进入上游前先锁定 API Key，按模型上下文和本次输出上限冻结最高费用；成功在同一事务内按实际费用结算并退回差额，失败释放冻结金额，流式请求在发送 `[DONE]` 前完成结算。

## 关键决定

- 用户侧只暴露统一余额，不做单模型钱包。
- 正式开放模型都走平台 API Key，不让用户理解多个供应商账号。
- 国内供应商优先走 OpenAI 兼容接口，减少每家重复开发。
- 上游 Key 未配置时返回 503，不伪造成功响应，不扣费。
- 占位模型只用于提前打通页面、白名单和路由，正式销售前必须替换真实模型 ID 和价格。
- 用户扣费仍读取 `input_rate/output_rate`；管理端用 `upstream_input_rate/upstream_output_rate + margin_rate` 计算并写回最终售价。
- Provider 明文 Key 不入库，只在服务器环境变量里保存；管理端只维护环境变量名和是否已配置。
- 风控中心只做风险观察，不自动封停用户；封停和恢复仍由用户管理页执行。
- OpenAPI 规格由后端直接返回，避免前端文档和真实接口脱节。
- 管理端页面前端先校验管理员身份，后端接口仍保留最终权限校验。
- 当前先使用静态 IOTPay 二维码 + 管理员人工确认入账；正式支付自动回调后续再接。
- 本机容器之间通过 `postgres`、`redis` 服务名连接，Vite 代理固定连接 `127.0.0.1:8080`，避免端口冲突和 IPv6 的 `localhost` 解析造成接口不可用。
- 数据库结构只通过 SQLx 版本化迁移维护，应用启动时不再重复执行兼容 DDL。
- 供应商默认使用 BYOK；只有取得书面授权并由管理员明确设置后，才能使用平台统一余额。
- 金额统一为美元高精度字段，模型单价统一为每 100 万 Token。
- 工作空间是余额、Key、订单、日志和 BYOK 凭据的隔离边界。
- API Key 明确归属项目；退出登录递增用户 token_version，所有旧 JWT 立即失效。
