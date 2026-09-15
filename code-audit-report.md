# PlanA 海外站代码审计报告

> 审计日期: 2026-05-28
> 审计范围: `abroad/overseas-api/src`
> 审计级别: Extra-High Effort
> **更新: 2026-05-28 第二轮 - 发现新 Bug**

---

## ✅ 修复状态

| 问题 | 状态 | 修复内容 |
|------|------|----------|
| **C1** | ✅ 已修复 | auth_layer 中间件 + JWT 验证 |
| **C2** | ✅ 已修复 | x-webhook-signature 验证 |
| **C3** | ✅ 已修复 | 原子操作余额扣减 |
| **C4** | ✅ 已修复 | 环境变量读取 JWT_SECRET |
| **E0599** | ✅ 已修复 | 添加 Row trait |
| **H2** | ✅ 已修复 | 连接池改为默认20，可配置 |
| **M2** | ✅ 已修复 | reqwest 30s超时 |
| **H5** | ✅ 已修复 | 账户锁定 - 5次失败锁定5分钟 |
| **CR1** | ✅ 已修复 | 余额双重扣减 → 先检查不预扣，成功后再扣 |
| **CR2** | ✅ 已修复 | API 失败不退款 → 不预扣，无需退款 |
| **CR3** | ✅ 已修复 | bcrypt 失败返回空 token → 显式错误处理 |

---

## 🔴 新发现问题 (2026-05-28 第二轮) - 已全部修复 ✅

### CR1. 余额双重扣减 Bug (严重)

**文件**: `main.rs:api_proxy`
**位置**: L956-966 预扣 + L1024-1031 又扣

```rust
// 第一次：预估扣减
let estimated_cost = 0.1; // 100 tokens
sqlx::query("UPDATE balances SET balance = balance - $1 ...").await;

// 第二次：实际使用又扣
let cost = (usage_tokens as f64) / 1000.0;
sqlx::query("UPDATE balances SET balance = balance - $1 ...").await;
```

**问题**: 用户会被扣两次费用！

**修复**: 移除第一次预估扣减，只在实际使用后扣减。

---

### CR2. API 失败不退款 (严重)

**文件**: `main.rs:api_proxy:L1035`

```rust
Err(e) => {
    return Json(json!({"error": {"message": "External API request failed"}}));
}
```

**问题**: 外部 API 调用失败但余额已经被预扣，不会退还给用户。

**修复**: API 调用失败时回滚余额或直接不预扣。

---

### CR3. 注册成功但 bcrypt 失败返回空 token

**文件**: `main.rs:register:L471, L490`

```rust
let password_hash = bcrypt::hash(password, 10).unwrap_or_default();
// ...
let token = crate::utils::jwt::generate_token(&claims).unwrap_or_default();
```

**问题**: 如果 bcrypt 失败，返回空的 token 给用户。

---

## 执行摘要

| 类别 | 数量 |
|------|------|
| 🔴 严重 (Critical) | 4 → **0** ✅ |
| 🟠 高 (High) | 5 |
| 🟡 中 (Medium) | 6 |
| 🔵 低 (Low) | 3 |

---

## 🔴 严重问题 (Critical)

### C1. 假认证 - 所有端点使用硬编码用户 ID

**文件**: `main.rs`
**位置**: 多处

```rust
// 行 111, 152, 176, 202, 278, 338, 444, 499, 603, 641
let dev_user_id = 1i64;
```

**问题**: 所有需要认证的 API 都使用硬编码的 `user_id = 1`，根本没有真正的身份验证。任何人都可以访问和操作任意用户的数据。

**场景**: 攻击者可以通过修改请求来访问其他用户的 API keys、订单、余额。

**建议**: 实现 JWT 认证中间件，从 `Authorization` header 提取并验证 token。

---

### C2. 支付 Webhook 无签名验证

**文件**: `main.rs:369-439`

**问题**: `payment_webhook` 端点没有任何签名验证机制。任何人都可以伪造支付通知来给自己充值。

```rust
async fn payment_webhook(State(pool): State<PgPool>, Json(input): Json<serde_json::Value>) -> Json<serde_json::Value> {
    // 没有验证签名，直接信任输入的 order_no
    let order_no = match input.get("order_no").and_then(|v| v.as_str()) {
```

**场景**: 攻击者可以构造恶意请求调用此端点，给任意用户添加余额。

**建议**: 添加 Stripe/Webhook 签名验证。

---

### C3. API Gateway 余额检查竞态条件

**文件**: `main.rs:501-520`

**问题**: 检查余额和扣减余额之间存在 TOCTOU (Time-of-Check-Time-of-Use) 漏洞。

```rust
// 检查余额
let current_balance: f64 = /* ... */;

// ⚠️ 这里有并发窗口，其他请求可能同时扣减余额

if current_balance < 100.0 {
    return Json(json!({"error": { "message": "Insufficient balance" }}));
}

// 稍后才扣减
sqlx::query("UPDATE balances SET balance = balance - $1 ...")
```

**场景**: 并发请求在余额检查通过后，可能导致超支。

**建议**: 使用数据库事务和原子操作 `balance = balance - $1 WHERE balance >= $1`。

---

### C4. JWT Secret 硬编码

**文件**: `utils/jwt.rs:5-8`

```rust
fn jwt_secret() -> &'static [u8] {
    b"your-super-secret-key-change-in-production"
}
```

**问题**: JWT secret 是硬编码的默认值，从未从环境变量读取。

**建议**: 使用 `config::jwt_secret()` 获取secret。

---

## 🟠 高风险问题 (High)

### H1. 缺少 API Key 验证在 Gateway

**文件**: `main.rs:487-592`

**问题**: `/v1/chat/completions` 端点没有验证用户提供的 API key。

```rust
// 直接使用 dev_user_id，没有验证 Authorization header 中的 API key
let dev_user_id = 1i64;
```

---

### H2. 数据库连接池配置过小

**文件**: `main.rs:27`

```rust
.max_connections(5)
```

**问题**: 生产环境中 5 个连接可能不足。

**建议**: 根据预期负载调整。

---

### H3. SQL 注入风险 (潜在)

**文件**: `main.rs:多处`

**问题**: 虽然使用参数化查询，但某些地方可能存在字符串拼接。

```rust
// 检查是否有直接拼接的用户输入
// 目前看来都是安全的，但需持续关注
```

---

### H4. 错误信息泄露敏感信息

**文件**: `main.rs:多处`

```rust
Err(e) => return Json(json!({"code": 500, "message": e.to_string()}))
```

**问题**: 错误信息可能暴露数据库结构、内部路径等敏感信息给客户端。

**建议**: 使用自定义错误消息，避免直接返回堆栈信息。

---

### H5. 账户锁定机制缺失

**文件**: `main.rs:auth.rs` (未实现)

**问题**: 没有账户锁定机制，暴力破解风险。

---

## 🟡 中等风险问题 (Medium)

### M1. 未使用的变量警告

**文件**: `main.rs:402`

```rust
let (user_id, package_id, amount, tokens, bonus): (i64, i64, f64, i32, i32) = /* ... */;
```

**问题**: `package_id` 和 `amount` 未被使用，产生编译器警告。

---

### M2. 缺少请求超时设置

**文件**: `main.rs:528-535`

```rust
let client = reqwest::Client::new();
// 缺少 .timeout() 配置
let response = client.post(...).send().await;
```

**问题**: 如果上游 API 无响应，会导致请求挂起。

---

### M3. 支付回调后缺少余额变动日志

**文件**: `main.rs:410-428`

**问题**: 支付成功后没有记录 `balance_logs`。

```rust
// 只有余额更新，没有记录流水
sqlx::query("INSERT INTO balances ...").execute(&pool).await.ok();
```

**建议**: 添加 `balance_logs` 记录。

---

### M4. 未使用的模型定义

**文件**: `models/*.rs`

**问题**: 多处 struct 定义但未被使用 (产生 dead_code 警告)。

---

### M5. 软删除不等于真正删除

**文件**: `repository/api_key_repo.rs:79-87`

```rust
pub async fn soft_delete(&self, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE api_keys SET status = 1, updated_at = NOW() WHERE id = $1")
```

**评价**: 这是正常做法，只是需要注意清理逻辑正确。

---

### M6. 缺少 CORS 配置

**文件**: `main.rs`

**问题**: API 可能被前端调用，但未配置 CORS。

---

## 🔵 低风险问题 (Low)

### L1. 重复的错误处理模式

**问题**: 每个 handler 都有相似的错误处理代码。

**建议**: 提取为通用函数。

---

### L2. 未实现的占位符

**文件**: `main.rs:98-102`

```rust
async fn register() -> Json<serde_json::Value> { Json(json!({"code": 0, "message": "注册"})) }
async fn login() -> Json<serde_json::Value> { Json(json!({"code": 0, "message": "登录"})) }
```

**问题**: Auth 模块是 stub 状态。

---

### L3. 日志记录不足

**文件**: `logging.rs`

**问题**: 缺少结构化日志，关键操作没有日志记录。

---

## 优先修复建议

| 优先级 | 问题 | 修复工作量 |
|--------|------|----------|
| P0 | C1 假认证 | 高 |
| P0 | C2 Webhook 无签名 | 中 |
| P0 | C3 竞态���件 | 中 |
| P0 | C4 JWT Secret | 低 |
| P1 | H1 API Key 验证 | 高 |
| P1 | H4 错误信息脱敏 | 低 |
| P2 | M2 请求超时 | 低 |
| P2 | M3 余额日志 | 低 |
| P3 | L2 实现 Auth | 高 |

---

## 附录：编译警告汇总

```
warning: unused import: `Extension`
warning: unused variable: `package_id`
warning: unused variable: `amount`
warning: unused variable: `messages`
warning: unused variable: `e`
warning: struct `User` is never constructed
warning: struct `CreateUserInput` is never constructed
warning: struct `LoginInput` is never constructed
warning: struct `UserResponse` is never constructed
warning: struct `LoginResponse` is never constructed
warning: struct `CreateKeyInput` is never constructed
warning: struct `KeyResponse` is never constructed
warning: struct `Provider` is never constructed
warning: struct `ProviderModel` is never constructed
warning: struct `Package` is never constructed
warning: struct `Order` is never constructed
```