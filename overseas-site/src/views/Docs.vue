<template>
  <ShellLayout :title="t('docs')" :subtitle="t('docsDesc')">
    <div class="market-hero">
      <div>
        <span class="badge badge-accent">v1 gateway</span>
        <h2>{{ t('docsHeroTitle') }}</h2>
        <p>{{ t('docsHeroDesc') }}</p>
      </div>
      <router-link to="/models" class="btn btn-ghost">{{ t('checkModels') }}</router-link>
    </div>

    <div class="panel openapi-panel">
      <div>
        <div class="panel-header compact-header">
          <h3>{{ labels.openapi }}</h3>
          <span class="badge badge-success">OpenAPI 3.1</span>
        </div>
        <p>{{ labels.openapiDesc }}</p>
      </div>
      <a class="btn btn-primary" href="/api/openapi.json" target="_blank" rel="noreferrer">
        {{ labels.openSpec }}
      </a>
    </div>

    <div class="panel">
      <div class="panel-header"><h3>{{ t('quickCall') }}</h3><span class="badge badge-success">curl</span></div>
      <pre class="code-card"><code>curl https://openbridgetech.ca/api/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "X-API-Key: nr-sk-your-key" \
  -d '{
    "model": "gpt-4.1-mini",
    "messages": [{"role": "user", "content": "Hello"}]
  }'</code></pre>
    </div>

    <div class="panel">
      <div class="panel-header">
        <h3>{{ labels.modelList }}</h3>
        <span class="badge badge-accent">GET /v1/models</span>
      </div>
      <p>{{ labels.modelListDesc }}</p>
      <pre class="code-card"><code>curl https://openbridgetech.ca/api/v1/models \
  -H "X-API-Key: nr-sk-your-key"</code></pre>
    </div>

    <div class="dash-grid">
      <div class="panel">
        <div class="panel-header"><h3>{{ t('requestFormat') }}</h3><span class="badge badge-accent">JSON</span></div>
        <pre class="code-card"><code>{
  "model": "gpt-4.1-mini",
  "messages": [
    { "role": "user", "content": "Write one test sentence." }
  ],
  "max_tokens": 512
}</code></pre>
      </div>

      <div class="panel">
        <div class="panel-header"><h3>{{ t('responseFormat') }}</h3><span class="badge badge-success">{{ t('openaiCompatible') }}</span></div>
        <pre class="code-card"><code>{
  "id": "chatcmpl-...",
  "object": "chat.completion",
  "model": "gpt-4.1-mini",
  "choices": [{
    "index": 0,
    "message": { "role": "assistant", "content": "Hello from Nexus Gateway." },
    "finish_reason": "stop"
  }],
  "usage": {
    "prompt_tokens": 12,
    "completion_tokens": 8,
    "total_tokens": 20
  }
}</code></pre>
      </div>
    </div>

    <div class="dash-grid">
      <div class="panel">
        <div class="panel-header">
          <h3>OpenAI Python SDK</h3>
          <span class="badge badge-success">base_url</span>
        </div>
        <p>{{ labels.sdkAuth }}</p>
        <pre class="code-card"><code>from openai import OpenAI

API_KEY = "nr-sk-your-key"

client = OpenAI(
    api_key="not-used",
    base_url="https://openbridgetech.ca/api/v1",
    default_headers={"X-API-Key": API_KEY},
    timeout=60.0,
    max_retries=0,
)

response = client.chat.completions.create(
    model="gpt-4.1-mini",
    messages=[
        {"role": "user", "content": "Hello"},
    ],
    max_tokens=512,
)
print(response.choices[0].message.content)</code></pre>
      </div>
      <div class="panel">
        <div class="panel-header">
          <h3>Python HTTP</h3>
          <span class="badge badge-accent">requests</span>
        </div>
        <pre class="code-card"><code>import requests

response = requests.post(
    "https://openbridgetech.ca/api/v1/chat/completions",
    headers={"X-API-Key": "nr-sk-your-key"},
    json={
        "model": "gpt-4.1-mini",
        "messages": [{"role": "user", "content": "Hello"}],
        "max_tokens": 512,
    },
    timeout=60,
)
response.raise_for_status()
print(response.json())</code></pre>
      </div>
    </div>

    <div class="panel">
      <div class="panel-header">
        <h3>Node.js</h3>
        <span class="badge badge-accent">fetch</span>
      </div>
      <pre class="code-card"><code>const response = await fetch(
  "https://openbridgetech.ca/api/v1/chat/completions",
  {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "X-API-Key": "nr-sk-your-key",
    },
    body: JSON.stringify({
      model: "gpt-4.1-mini",
      messages: [{ role: "user", content: "Hello" }],
      max_tokens: 512,
    }),
  }
)

if (!response.ok) throw new Error(await response.text())
console.log(await response.json())</code></pre>
    </div>

    <div class="panel">
      <div class="panel-header">
        <h3>{{ labels.currentScope }}</h3>
        <span class="badge badge-success">{{ labels.truthfulScope }}</span>
      </div>
      <div class="docs-rule-grid">
        <div v-for="item in labels.scopeItems" :key="item.title" class="rule-card">
          <span>{{ item.title }}</span>
          <p>{{ item.desc }}</p>
        </div>
      </div>
    </div>

    <div class="dash-grid">
      <div class="panel">
        <div class="panel-header">
          <h3>{{ labels.streaming }}</h3>
          <span class="badge badge-success">OpenAI SSE</span>
        </div>
        <p>{{ labels.streamingDesc }}</p>
        <pre class="code-card"><code>{
  "model": "gpt-4.1-mini",
  "messages": [{ "role": "user", "content": "Hello" }],
  "stream": true
}</code></pre>
      </div>
      <div class="panel">
        <div class="panel-header">
          <h3>{{ labels.retryTitle }}</h3>
          <span class="badge badge-accent">{{ labels.clientControlled }}</span>
        </div>
        <div class="docs-rule-grid">
          <div v-for="item in labels.retryItems" :key="item.title" class="rule-card">
            <span>{{ item.title }}</span>
            <p>{{ item.desc }}</p>
          </div>
        </div>
      </div>
    </div>

    <div class="panel">
      <div class="panel-header">
        <h3>{{ labels.errorCodes }}</h3>
        <span class="badge badge-accent">HTTP + JSON</span>
      </div>
      <p>{{ labels.errorShape }}</p>
      <div class="docs-rule-grid">
        <div v-for="item in labels.errorItems" :key="item.status" class="rule-card">
          <span>{{ item.status }}</span>
          <p>{{ item.codes }}</p>
          <p>{{ item.action }}</p>
        </div>
      </div>
    </div>

    <div class="docs-rule-grid">
      <div class="rule-card">
        <span>{{ t('auth') }}</span>
        <p>{{ t('authDesc') }}</p>
      </div>
      <div class="rule-card">
        <span>{{ t('billing') }}</span>
        <p>{{ t('billingDesc') }}</p>
      </div>
      <div class="rule-card">
        <span>{{ t('limits') }}</span>
        <p>{{ t('limitsDesc') }}</p>
      </div>
      <div class="rule-card">
        <span>{{ t('errors') }}</span>
        <p>{{ t('errorsDesc') }}</p>
      </div>
    </div>
  </ShellLayout>
</template>

<script setup>
import { computed } from 'vue'
import ShellLayout from '../components/ShellLayout.vue'
import { useI18n } from '../i18n.js'

const { t, isZh } = useI18n()
const labels = computed(() => isZh.value ? {
  openapi: 'OpenAPI 规格',
  openapiDesc: '用于导入 Apifox、Postman 或 Swagger。模型调用以本页标明的当前支持范围为准。',
  openSpec: '打开 openapi.json',
  modelList: '获取可用模型',
  modelListDesc: '模型列表也必须携带平台 X-API-Key；请以接口实时返回的模型 ID 为准。',
  sdkAuth: 'SDK 的 api_key 参数不能代替平台鉴权。必须设置 base_url，并通过 default_headers 显式发送 X-API-Key。示例关闭 SDK 自动重试，由业务代码按下方规则决定是否重试。',
  currentScope: '当前支持范围',
  truthfulScope: '真实能力',
  scopeItems: [
    { title: '已支持的接口', desc: 'POST /v1/chat/completions、POST /v1/responses 和 GET /v1/models。模型调用只接受平台 X-API-Key。' },
    { title: '稳定的共同能力', desc: '文本消息、system、函数工具调用、JSON Schema 结构化输出、图片输入，以及统一的 OpenAI 响应格式。不兼容参数会直接返回 400。' },
    { title: '供应商范围', desc: '已配置且启用的 OpenAI、Anthropic、Google 和 OpenAI 兼容文本模型；具体可用项以 /v1/models 为准。' },
    { title: '明确不可用的接口', desc: 'Embeddings 和图片、音频、视频生成已有固定路由，但当前没有启用的真实接入点，会返回 501，不会创建假任务。' },
    { title: 'BYOK 状态', desc: 'BYOK 凭据加密保存并已接入模型路由；BYOK 调用使用用户自己的供应商账户，平台不重复收取模型 Token 费用。' },
  ],
  streaming: '流式说明',
  streamingDesc: 'OpenAI 接入点支持真正的 SSE 分块输出。网关会预留最大费用，读取最终 usage 后原子结算，再发送 [DONE]。Claude、Gemini 和未经验证的 OpenAI 兼容接入点使用 stream=true 会明确返回 501。',
  retryTitle: '重试规则',
  clientControlled: '客户端控制',
  retryItems: [
    { title: '不重试', desc: '400、401、402、403，以及 KEY_DAILY_LIMIT_EXCEEDED。先修正模型、Key、余额、白名单或预算。' },
    { title: '按时间重试', desc: 'RATE_LIMIT_EXCEEDED 才按 Retry-After 等待后重试，不要立即循环请求。' },
    { title: '有限重试', desc: '502 或临时 503 可按 1 秒、2 秒退避，最多重试 2 次；PROVIDER_NOT_CONFIGURED 和 BYOK_CREDENTIAL_UNAVAILABLE 不重试。' },
    { title: '模型不变', desc: '网关失败后不会自动改道或更换模型；客户端重试时也应保持原模型，并防止业务重复执行。' },
  ],
  errorCodes: '错误码',
  errorShape: '先判断 HTTP 状态。网关业务错误通常位于 error.code；API Key 中间件可能返回顶层 code 和 message，客户端应兼容这两种真实格式。',
  errorItems: [
    { status: '400', codes: 'UNSUPPORTED_MODEL', action: '使用 /v1/models 返回的有效模型 ID。' },
    { status: '401', codes: 'Valid API key required / Invalid or expired API key', action: '检查 X-API-Key、Key 状态、有效期和 IP 白名单。' },
    { status: '402', codes: 'INSUFFICIENT_BALANCE', action: '充值后重新发起请求。' },
    { status: '403', codes: 'MODEL_NOT_ALLOWED', action: '把模型加入该 Key 的模型白名单。' },
    { status: '429', codes: 'RATE_LIMIT_EXCEEDED / KEY_DAILY_LIMIT_EXCEEDED', action: '前者读取 Retry-After；后者调整预算或等待下一计费日。' },
    { status: '501', codes: 'STREAMING_NOT_AVAILABLE_FOR_PROVIDER / EMBEDDINGS_NOT_AVAILABLE / ASYNC_MEDIA_NOT_AVAILABLE', action: '该供应商或能力尚无真实接入点；不要重试或按成功处理。' },
    { status: '502', codes: 'UPSTREAM_API_ERROR / UPSTREAM_REQUEST_FAILED', action: '上游失败；可有限退避重试。' },
    { status: '503', codes: 'PROVIDER_NOT_CONFIGURED / PROVIDER_CIRCUIT_OPEN / PROVIDER_HEALTH_UNAVAILABLE / RATE_LIMITER_UNAVAILABLE / BYOK_CREDENTIAL_UNAVAILABLE', action: '查看具体 code；配置类错误不要重试，临时服务错误才有限重试。' },
  ],
} : {
  openapi: 'OpenAPI Spec',
  openapiDesc: 'Import into Apifox, Postman, or Swagger. For model calls, use the current support boundary documented on this page.',
  openSpec: 'Open openapi.json',
  modelList: 'List available models',
  modelListDesc: 'The model list also requires the platform X-API-Key. Treat the model IDs returned by this endpoint as authoritative.',
  sdkAuth: 'The SDK api_key option does not replace platform authentication. Set base_url and explicitly send X-API-Key through default_headers. Automatic SDK retries are disabled so your application can follow the rules below.',
  currentScope: 'Current support',
  truthfulScope: 'Actual capability',
  scopeItems: [
    { title: 'Supported endpoints', desc: 'POST /v1/chat/completions, POST /v1/responses, and GET /v1/models. Model calls accept only the platform X-API-Key.' },
    { title: 'Stable common subset', desc: 'Text messages, system prompts, function tools, JSON Schema structured output, image input, and unified OpenAI response shapes. Incompatible parameters fail with HTTP 400.' },
    { title: 'Provider scope', desc: 'Configured and enabled OpenAI, Anthropic, Google, and OpenAI-compatible text models. Use /v1/models for the live list.' },
    { title: 'Explicitly unavailable endpoints', desc: 'Embeddings and image, audio, or video generation have stable routes but no enabled real provider endpoint. They return HTTP 501 and never create fake tasks.' },
    { title: 'BYOK status', desc: 'BYOK credentials are encrypted and active in model routing. BYOK calls use the user\'s provider account, so the platform does not charge the model token cost again.' },
  ],
  streaming: 'Streaming',
  streamingDesc: 'OpenAI endpoints support true SSE chunks. Nexus reserves the maximum charge, atomically settles from the final usage event, and only then emits [DONE]. Claude, Gemini, and unverified OpenAI-compatible providers return HTTP 501 for stream=true.',
  retryTitle: 'Retry rules',
  clientControlled: 'Client controlled',
  retryItems: [
    { title: 'Do not retry', desc: '400, 401, 402, 403, or KEY_DAILY_LIMIT_EXCEEDED. Fix the model, key, balance, allowlist, or budget first.' },
    { title: 'Retry after waiting', desc: 'Only RATE_LIMIT_EXCEEDED should follow Retry-After. Never retry it in a tight loop.' },
    { title: 'Limited retries', desc: 'For 502 or a transient 503, back off for 1s then 2s and retry at most twice. Do not retry PROVIDER_NOT_CONFIGURED or BYOK_CREDENTIAL_UNAVAILABLE.' },
    { title: 'Keep the model fixed', desc: 'The gateway does not reroute or replace the model after a failure. Keep the same model on retry and protect your business flow from duplicate work.' },
  ],
  errorCodes: 'Error codes',
  errorShape: 'Check the HTTP status first. Gateway business errors usually use error.code; API-key middleware may return top-level code and message. Clients must handle both actual shapes.',
  errorItems: [
    { status: '400', codes: 'UNSUPPORTED_MODEL', action: 'Use a valid model ID returned by /v1/models.' },
    { status: '401', codes: 'Valid API key required / Invalid or expired API key', action: 'Check X-API-Key, key status, expiry, and IP allowlist.' },
    { status: '402', codes: 'INSUFFICIENT_BALANCE', action: 'Add balance before sending the request again.' },
    { status: '403', codes: 'MODEL_NOT_ALLOWED', action: 'Add the model to this key\'s model allowlist.' },
    { status: '429', codes: 'RATE_LIMIT_EXCEEDED / KEY_DAILY_LIMIT_EXCEEDED', action: 'Read Retry-After for the first; change the budget or wait for the next billing day for the second.' },
    { status: '501', codes: 'STREAMING_NOT_AVAILABLE_FOR_PROVIDER / EMBEDDINGS_NOT_AVAILABLE / ASYNC_MEDIA_NOT_AVAILABLE', action: 'The provider or capability has no real endpoint yet. Do not retry or treat it as success.' },
    { status: '502', codes: 'UPSTREAM_API_ERROR / UPSTREAM_REQUEST_FAILED', action: 'The upstream failed; a limited backoff retry is allowed.' },
    { status: '503', codes: 'PROVIDER_NOT_CONFIGURED / PROVIDER_CIRCUIT_OPEN / PROVIDER_HEALTH_UNAVAILABLE / RATE_LIMITER_UNAVAILABLE / BYOK_CREDENTIAL_UNAVAILABLE', action: 'Inspect the code. Do not retry configuration errors; only retry transient service errors with a limit.' },
  ],
})
</script>

<style scoped>
.openapi-panel {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 20px;
}
.openapi-panel p {
  color: var(--muted);
  margin-top: 8px;
  max-width: 760px;
}
.compact-header {
  justify-content: flex-start;
  margin-bottom: 0;
}

@media (max-width: 720px) {
  .openapi-panel {
    align-items: flex-start;
    flex-direction: column;
  }
}
</style>
