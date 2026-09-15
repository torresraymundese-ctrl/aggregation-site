<template>
  <ShellLayout :title="labels.title" :subtitle="labels.subtitle" :admin="admin" :guest="isGuest">
    <div class="market-hero">
      <div>
        <span class="badge badge-accent">{{ labels.unifiedBalance }}</span>
        <h2>{{ labels.heroTitle }}</h2>
        <p>{{ labels.heroDesc }}</p>
      </div>
      <router-link to="/keys" class="btn btn-primary">{{ t('createKey') }}</router-link>
    </div>

    <div class="panel model-filter-panel">
      <div class="model-filter-grid">
        <div class="form-group model-search">
          <label for="model-search">{{ labels.search }}</label>
          <input id="model-search" v-model.trim="search" type="search" :placeholder="labels.searchPlaceholder" />
        </div>
        <div class="form-group">
          <label for="vendor-filter">{{ labels.vendor }}</label>
          <select id="vendor-filter" v-model="vendorFilter">
            <option value="all">{{ labels.all }}</option>
            <option v-for="vendor in vendorOptions" :key="vendor" :value="vendor">{{ vendor }}</option>
          </select>
        </div>
        <div class="form-group">
          <label for="modality-filter">{{ labels.modality }}</label>
          <select id="modality-filter" v-model="modalityFilter">
            <option value="all">{{ labels.all }}</option>
            <option v-for="modality in modalityOptions" :key="modality" :value="modality">{{ modality }}</option>
          </select>
        </div>
        <div class="form-group">
          <label for="status-filter">{{ labels.status }}</label>
          <select id="status-filter" v-model="statusFilter">
            <option value="all">{{ labels.all }}</option>
            <option v-for="status in statusOptions" :key="status" :value="status">{{ statusText(status) }}</option>
          </select>
        </div>
        <div class="form-group">
          <label for="model-sort">{{ labels.sort }}</label>
          <select id="model-sort" v-model="sortBy">
            <option value="default">{{ labels.defaultSort }}</option>
            <option value="input-asc">{{ labels.inputLow }}</option>
            <option value="output-asc">{{ labels.outputLow }}</option>
            <option value="context-desc">{{ labels.contextHigh }}</option>
            <option value="context-asc">{{ labels.contextLow }}</option>
          </select>
        </div>
      </div>
      <div class="model-filter-meta">
        <span>{{ labels.results }}：{{ filteredModels.length }}</span>
        <span>{{ labels.compareSelected }}：{{ compareIds.length }}/3</span>
      </div>
    </div>

    <div v-if="providerSummary.length" class="provider-grid compact">
      <button
        v-for="provider in providerSummary"
        :key="provider.name"
        type="button"
        class="provider-tile provider-filter-tile"
        :class="{ active: vendorFilter === provider.name }"
        @click="vendorFilter = vendorFilter === provider.name ? 'all' : provider.name"
      >
        <span :class="['provider-mark', providerTone(provider.name)]">{{ providerShort(provider.name) }}</span>
        <div>
          <h3>{{ provider.name }}</h3>
          <p>{{ provider.count }} {{ labels.configuredModels }}</p>
        </div>
      </button>
    </div>

    <div v-if="loading" class="empty-state">{{ t('loading') }}</div>
    <div v-else-if="error" class="empty-state danger">{{ error }}</div>
    <div v-else-if="!filteredModels.length" class="empty-state">{{ labels.noMatchingModels }}</div>
    <template v-else>
      <div v-if="comparedModels.length" class="panel comparison-panel">
        <div class="panel-header">
          <div>
            <h3>{{ labels.comparison }}</h3>
            <p class="muted">{{ labels.comparisonDesc }}</p>
          </div>
          <button type="button" class="btn btn-ghost btn-sm" @click="compareIds = []">{{ labels.clear }}</button>
        </div>
        <div class="table-wrap">
          <table class="data-table comparison-table">
            <thead>
              <tr>
                <th>{{ labels.item }}</th>
                <th v-for="model in comparedModels" :key="`compare-head-${model.model_id}`">{{ displayModelName(model) }}</th>
              </tr>
            </thead>
            <tbody>
              <tr><td>{{ labels.vendor }}</td><td v-for="model in comparedModels" :key="`vendor-${model.model_id}`">{{ model.vendor }}</td></tr>
              <tr><td>{{ labels.inputPrice }}</td><td v-for="model in comparedModels" :key="`input-${model.model_id}`">{{ formatPrice(model.input_rate) }}</td></tr>
              <tr><td>{{ labels.outputPrice }}</td><td v-for="model in comparedModels" :key="`output-${model.model_id}`">{{ formatPrice(model.output_rate) }}</td></tr>
              <tr><td>{{ labels.context }}</td><td v-for="model in comparedModels" :key="`context-${model.model_id}`">{{ compactNumber(model.context_len) }}</td></tr>
              <tr><td>{{ labels.modality }}</td><td v-for="model in comparedModels" :key="`modality-${model.model_id}`">{{ listText(model.modalities) }}</td></tr>
              <tr><td>{{ labels.accessPoints }}</td><td v-for="model in comparedModels" :key="`providers-${model.model_id}`">{{ providerNames(model) }}</td></tr>
            </tbody>
          </table>
        </div>
      </div>

      <div class="model-market-grid">
        <article v-for="model in filteredModels" :key="model.model_id" class="model-card">
          <div class="model-card-top">
            <span :class="['provider-mark', providerTone(model.vendor)]">{{ providerShort(model.vendor) }}</span>
            <span :class="['badge', statusClass(model.normalizedStatus)]">{{ statusText(model.normalizedStatus) }}</span>
          </div>
          <div>
            <h3>{{ displayModelName(model) }}</h3>
            <p class="mono">{{ model.model_id }}</p>
          </div>
          <p class="model-description">{{ model.description || labels.notProvided }}</p>
          <div v-if="model.modalities.length" class="capability-list">
            <span v-for="modality in model.modalities" :key="`${model.model_id}-${modality}`" class="badge badge-accent">{{ modality }}</span>
          </div>
          <div class="model-facts">
            <span><strong>{{ compactNumber(model.context_len) }}</strong>{{ labels.context }}</span>
            <span><strong>{{ compactNumber(model.max_tokens) }}</strong>{{ labels.maxOutput }}</span>
          </div>
          <div class="price-row">
            <div><span>{{ labels.inputPrice }}</span><strong>{{ formatPrice(model.input_rate) }}</strong></div>
            <div><span>{{ labels.outputPrice }}</span><strong>{{ formatPrice(model.output_rate) }}</strong></div>
          </div>
          <div class="model-card-actions">
            <button
              type="button"
              class="btn btn-ghost btn-sm"
              :class="{ active: compareIds.includes(model.model_id) }"
              @click="toggleCompare(model)"
            >
              {{ compareIds.includes(model.model_id) ? labels.removeCompare : labels.addCompare }}
            </button>
            <button type="button" class="btn btn-primary btn-sm" @click="openDetail(model)">{{ labels.detailsAndTest }}</button>
          </div>
        </article>
      </div>
    </template>

    <div v-if="detailModel" class="modal-backdrop model-detail-backdrop" role="presentation" @click.self="closeDetail">
      <section class="modal-card model-detail-modal" role="dialog" aria-modal="true" :aria-label="displayModelName(detailModel)">
        <div class="model-detail-header">
          <div>
            <div class="capability-list">
              <span :class="['badge', statusClass(detailModel.normalizedStatus)]">{{ statusText(detailModel.normalizedStatus) }}</span>
              <span class="badge badge-accent">{{ detailModel.vendor }}</span>
            </div>
            <h3>{{ displayModelName(detailModel) }}</h3>
            <p class="mono">{{ detailModel.model_id }}</p>
          </div>
          <button type="button" class="btn btn-ghost btn-sm" @click="closeDetail">{{ labels.close }}</button>
        </div>
        <p>{{ detailModel.description || labels.notProvided }}</p>

        <div class="detail-fact-grid">
          <div><span>{{ labels.inputPrice }}</span><strong>{{ formatPrice(detailModel.input_rate) }}</strong></div>
          <div><span>{{ labels.outputPrice }}</span><strong>{{ formatPrice(detailModel.output_rate) }}</strong></div>
          <div><span>{{ labels.context }}</span><strong>{{ compactNumber(detailModel.context_len) }}</strong></div>
          <div><span>{{ labels.maxOutput }}</span><strong>{{ compactNumber(detailModel.max_tokens) }}</strong></div>
        </div>

        <div class="detail-info-grid">
          <div>
            <h4>{{ labels.modalities }}</h4>
            <p>{{ listText(detailModel.modalities) }}</p>
          </div>
          <div>
            <h4>{{ labels.supportedParameters }}</h4>
            <p>{{ listText(detailModel.supportedParameters) }}</p>
          </div>
          <div>
            <h4>{{ labels.accessPoints }}</h4>
            <div v-if="detailModel.providerEntries.length" class="endpoint-list">
              <span v-for="(provider, index) in detailModel.providerEntries" :key="`${provider.name}-${index}`">
                {{ provider.name }}<small v-if="provider.region"> · {{ provider.region }}</small>
              </span>
            </div>
            <p v-else>{{ labels.notProvided }}</p>
          </div>
          <div>
            <h4>{{ labels.routing }}</h4>
            <p>{{ routingText(detailModel.routing) }}</p>
          </div>
        </div>

        <div class="playground-panel">
          <div class="panel-header">
            <div>
              <h3>{{ labels.playground }}</h3>
              <p class="muted">{{ labels.playgroundDesc }}</p>
            </div>
            <button type="button" class="btn btn-primary btn-sm" :disabled="playgroundLoading" @click="runPlayground">
              {{ playgroundLoading ? labels.sending : labels.send }}
            </button>
          </div>
          <div class="form-group">
            <label for="playground-key">{{ labels.apiKey }}</label>
            <input
              id="playground-key"
              v-model="playgroundKey"
              type="password"
              autocomplete="off"
              spellcheck="false"
              placeholder="nr-sk-..."
            />
            <small>{{ labels.keyPrivacy }}</small>
          </div>
          <div class="form-group">
            <label for="playground-body">{{ labels.requestBody }}</label>
            <textarea id="playground-body" v-model="playgroundBody" class="request-editor mono" spellcheck="false"></textarea>
          </div>
          <div v-if="playgroundError" class="empty-state danger playground-output">{{ playgroundError }}</div>
          <pre v-else-if="playgroundResult" class="code-card playground-output"><code>{{ playgroundResult }}</code></pre>
        </div>

        <div class="code-example-panel">
          <div class="code-tab-list">
            <button
              v-for="tab in codeTabs"
              :key="tab"
              type="button"
              class="filter-chip"
              :class="{ active: activeCodeTab === tab }"
              @click="activeCodeTab = tab"
            >{{ tab }}</button>
          </div>
          <pre class="code-card"><code>{{ activeCodeExample }}</code></pre>
        </div>
      </section>
    </div>
  </ShellLayout>
</template>

<script setup>
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { apiErrorMessage, requestJson } from '../api.js'
import { getAuthToken } from '../auth.js'
import ShellLayout from '../components/ShellLayout.vue'
import { useI18n } from '../i18n.js'
import { notifyError } from '../notify.js'

const props = defineProps({
  admin: { type: Boolean, default: false },
})

const isGuest = !props.admin && !getAuthToken()

const { t, isZh } = useI18n()
const models = ref([])
const loading = ref(true)
const error = ref('')
const search = ref('')
const vendorFilter = ref('all')
const modalityFilter = ref('all')
const statusFilter = ref('all')
const sortBy = ref('default')
const compareIds = ref([])
const detailModel = ref(null)
const playgroundKey = ref('')
const playgroundBody = ref('')
const playgroundLoading = ref(false)
const playgroundError = ref('')
const playgroundResult = ref('')
const codeTabs = ['Curl', 'Python', 'Node']
const activeCodeTab = ref('Curl')
const endpoint = 'https://openbridgetech.ca/api/v1/chat/completions'

const labels = computed(() => isZh.value ? {
  title: '模型广场',
  subtitle: '搜索、比较并用真实网关测试开放模型。',
  unifiedBalance: '统一余额',
  heroTitle: '找到合适的模型，然后直接复制接入代码。',
  heroDesc: '价格单位统一为美元/每 100 万 Token。能力、状态和接入点只展示接口返回的真实信息。',
  search: '搜索模型',
  searchPlaceholder: '模型名称、ID、厂商或描述',
  vendor: '厂商',
  modality: '模态',
  status: '状态',
  sort: '排序',
  all: '全部',
  defaultSort: '默认顺序',
  inputLow: '输入价格从低到高',
  outputLow: '输出价格从低到高',
  contextHigh: '上下文从高到低',
  contextLow: '上下文从低到高',
  results: '结果',
  compareSelected: '已选对比',
  configuredModels: '个模型',
  noMatchingModels: '没有符合条件的模型。',
  comparison: '模型对比',
  comparisonDesc: '最多同时比较 3 个模型。',
  clear: '清空',
  item: '项目',
  inputPrice: '输入 / 每 100 万 Token',
  outputPrice: '输出 / 每 100 万 Token',
  context: '上下文',
  maxOutput: '最大输出',
  accessPoints: '接入点',
  notProvided: '接口未提供',
  addCompare: '加入对比',
  removeCompare: '移出对比',
  detailsAndTest: '详情与测试',
  compareLimit: '最多只能同时比较 3 个模型。',
  active: '可用',
  disabled: '停用',
  degraded: '降级',
  unknown: '未知',
  close: '关闭',
  modalities: '能力与模态',
  supportedParameters: '支持参数',
  routing: '路由信息',
  playground: '真实 API Playground',
  playgroundDesc: '请求会直接发送到 Nexus 网关，不会使用 Mock。',
  apiKey: '平台 API Key',
  keyPrivacy: 'Key 仅保存在当前页面内存中，关闭弹层后立即清除。',
  requestBody: '请求 JSON',
  send: '发送请求',
  sending: '请求中',
  keyRequired: '请输入平台 API Key。',
  invalidJson: '请求体不是合法 JSON。',
  modelRequired: '请求体必须包含 model。',
  streamUnsupported: '页面 Playground 当前只支持非流式请求；流式调用请使用代码示例。',
  requestFailed: '模型请求失败。',
  loadFailed: '模型加载失败。',
} : {
  title: 'Model Marketplace',
  subtitle: 'Search, compare and test open models through the live gateway.',
  unifiedBalance: 'Unified balance',
  heroTitle: 'Find the right model, then copy working integration code.',
  heroDesc: 'Prices are shown in USD per 1M tokens. Capabilities, status and endpoints come only from the live API.',
  search: 'Search models',
  searchPlaceholder: 'Model name, ID, vendor or description',
  vendor: 'Vendor',
  modality: 'Modality',
  status: 'Status',
  sort: 'Sort',
  all: 'All',
  defaultSort: 'Default order',
  inputLow: 'Lowest input price',
  outputLow: 'Lowest output price',
  contextHigh: 'Largest context',
  contextLow: 'Smallest context',
  results: 'Results',
  compareSelected: 'Selected for comparison',
  configuredModels: 'models',
  noMatchingModels: 'No models match these filters.',
  comparison: 'Model comparison',
  comparisonDesc: 'Compare up to 3 models at a time.',
  clear: 'Clear',
  item: 'Item',
  inputPrice: 'Input / 1M tokens',
  outputPrice: 'Output / 1M tokens',
  context: 'Context',
  maxOutput: 'Max output',
  accessPoints: 'Endpoints',
  notProvided: 'Not provided by API',
  addCompare: 'Compare',
  removeCompare: 'Remove',
  detailsAndTest: 'Details & test',
  compareLimit: 'You can compare up to 3 models at a time.',
  active: 'Active',
  disabled: 'Disabled',
  degraded: 'Degraded',
  unknown: 'Unknown',
  close: 'Close',
  modalities: 'Capabilities & modalities',
  supportedParameters: 'Supported parameters',
  routing: 'Routing',
  playground: 'Live API Playground',
  playgroundDesc: 'Requests go directly to Nexus Gateway. No mock data is used.',
  apiKey: 'Platform API key',
  keyPrivacy: 'The key stays only in page memory and is cleared when this dialog closes.',
  requestBody: 'Request JSON',
  send: 'Send request',
  sending: 'Sending',
  keyRequired: 'Enter a platform API key.',
  invalidJson: 'The request body is not valid JSON.',
  modelRequired: 'The request body must include model.',
  streamUnsupported: 'The page playground currently supports non-streaming requests only. Use the code examples for streaming.',
  requestFailed: 'Model request failed.',
  loadFailed: 'Failed to load models.',
})

const arrayOfText = (value) => {
  if (Array.isArray(value)) {
    return [...new Set(value.map((item) => typeof item === 'string' ? item : item?.name || item?.id || '').filter(Boolean))]
  }
  if (value && typeof value === 'object') {
    return Object.entries(value).filter(([, enabled]) => Boolean(enabled)).map(([name]) => name)
  }
  if (typeof value === 'string' && value.trim()) return value.split(',').map((item) => item.trim()).filter(Boolean)
  return []
}

const normalizeStatus = (status) => {
  if (status === undefined || status === null || status === 0 || status === '0') return 'active'
  const text = String(status).toLowerCase()
  if (['active', 'enabled', 'available', 'online'].includes(text)) return 'active'
  if (['degraded', 'limited'].includes(text)) return 'degraded'
  if (['disabled', 'inactive', 'offline', '1'].includes(text)) return 'disabled'
  return text || 'unknown'
}

const normalizeProviders = (model) => {
  let entries = []
  if (Array.isArray(model.providers)) {
    entries = model.providers
  } else if (model.providers && typeof model.providers === 'object') {
    entries = Object.entries(model.providers).map(([name, detail]) => typeof detail === 'object' ? { name, ...detail } : { name })
  } else if (Array.isArray(model.endpoints)) {
    entries = model.endpoints
  } else if (model.provider) {
    entries = [model.provider]
  }
  return entries.map((entry) => {
    if (typeof entry === 'string') return { name: entry, region: '' }
    return {
      ...entry,
      name: entry?.name || entry?.provider || entry?.provider_name || entry?.provider_id || '',
      region: entry?.region || '',
    }
  }).filter((entry) => entry.name)
}

const numericOrNull = (value) => {
  if (value === undefined || value === null || value === '') return null
  const number = Number(value)
  return Number.isFinite(number) ? number : null
}

const normalizeModel = (model, index) => {
  const providerEntries = normalizeProviders(model)
  return {
    ...model,
    model_id: String(model.model_id || model.id || ''),
    display_name: model.display_name || model.name || model.model_id || model.id || '',
    vendor: String(model.vendor || model.provider || providerEntries[0]?.name || labels.value.unknown),
    description: typeof model.description === 'string' ? model.description : '',
    input_rate: numericOrNull(model.input_rate ?? model.pricing?.input),
    output_rate: numericOrNull(model.output_rate ?? model.pricing?.output),
    context_len: numericOrNull(model.context_len ?? model.context_length),
    max_tokens: numericOrNull(model.max_tokens ?? model.max_output_tokens),
    modalities: arrayOfText(model.modalities),
    supportedParameters: arrayOfText(model.supported_parameters),
    providerEntries,
    routing: model.routing ?? null,
    normalizedStatus: normalizeStatus(model.status),
    originalOrder: index,
  }
}

const vendorOptions = computed(() => [...new Set(models.value.map((model) => model.vendor))].sort((a, b) => a.localeCompare(b)))
const modalityOptions = computed(() => [...new Set(models.value.flatMap((model) => model.modalities))].sort((a, b) => a.localeCompare(b)))
const statusOptions = computed(() => [...new Set(models.value.map((model) => model.normalizedStatus))])
const providerSummary = computed(() => vendorOptions.value.map((name) => ({
  name,
  count: models.value.filter((model) => model.vendor === name).length,
})))

const filteredModels = computed(() => {
  const query = search.value.toLowerCase()
  const items = models.value.filter((model) => {
    const haystack = [model.model_id, model.display_name, model.vendor, model.description].join(' ').toLowerCase()
    return (!query || haystack.includes(query))
      && (vendorFilter.value === 'all' || model.vendor === vendorFilter.value)
      && (modalityFilter.value === 'all' || model.modalities.includes(modalityFilter.value))
      && (statusFilter.value === 'all' || model.normalizedStatus === statusFilter.value)
  })
  const sorted = [...items]
  if (sortBy.value === 'input-asc') sorted.sort((a, b) => (a.input_rate ?? Infinity) - (b.input_rate ?? Infinity))
  if (sortBy.value === 'output-asc') sorted.sort((a, b) => (a.output_rate ?? Infinity) - (b.output_rate ?? Infinity))
  if (sortBy.value === 'context-desc') sorted.sort((a, b) => (b.context_len ?? -1) - (a.context_len ?? -1))
  if (sortBy.value === 'context-asc') sorted.sort((a, b) => (a.context_len ?? Infinity) - (b.context_len ?? Infinity))
  if (sortBy.value === 'default') sorted.sort((a, b) => a.originalOrder - b.originalOrder)
  return sorted
})

const comparedModels = computed(() => compareIds.value
  .map((id) => models.value.find((model) => model.model_id === id))
  .filter(Boolean))

const displayModelName = (model) => String(model?.display_name || model?.model_id || '').replace(/\s+Placeholder$/i, '')
const money = (value) => Number(value || 0).toFixed(6).replace(/0+$/, '').replace(/\.$/, '.0')
const formatPrice = (value) => value === null ? labels.value.notProvided : `$${money(value)}`
const compactNumber = (value) => {
  if (value === null || value === undefined) return labels.value.notProvided
  const number = Number(value)
  if (number >= 1000000) return `${number / 1000000}M`
  if (number >= 1000) return `${number / 1000}K`
  return String(number)
}
const listText = (items) => items.length ? items.join(', ') : labels.value.notProvided
const providerNames = (model) => model.providerEntries.length
  ? model.providerEntries.map((provider) => provider.name).join(', ')
  : labels.value.notProvided
const routingText = (routing) => {
  if (!routing) return labels.value.notProvided
  if (typeof routing === 'string') return routing
  if (Array.isArray(routing)) return routing.join(', ')
  return Object.entries(routing).map(([key, value]) => `${key}: ${typeof value === 'object' ? JSON.stringify(value) : value}`).join(' · ')
}
const statusText = (status) => labels.value[status] || status || labels.value.unknown
const statusClass = (status) => status === 'active' ? 'badge-success' : status === 'degraded' ? 'badge-warn' : 'badge-danger'
const providerShort = (provider) => String(provider || 'AI').replace(/[^a-z0-9]/gi, '').slice(0, 2).toUpperCase() || 'AI'
const providerTone = (provider) => {
  const value = String(provider || '').toLowerCase()
  if (value.includes('openai')) return 'tone-green'
  if (value.includes('anthropic') || value.includes('claude')) return 'tone-amber'
  if (value.includes('google') || value.includes('gemini')) return 'tone-sky'
  if (value.includes('deepseek')) return 'tone-red'
  if (value.includes('zhipu') || value.includes('glm')) return 'tone-purple'
  if (value.includes('qwen')) return 'tone-cyan'
  return 'tone-blue'
}

const toggleCompare = (model) => {
  if (compareIds.value.includes(model.model_id)) {
    compareIds.value = compareIds.value.filter((id) => id !== model.model_id)
    return
  }
  if (compareIds.value.length >= 3) {
    notifyError(labels.value.compareLimit)
    return
  }
  compareIds.value = [...compareIds.value, model.model_id]
}

const defaultBody = (model) => JSON.stringify({
  model: model.model_id,
  messages: [{ role: 'user', content: isZh.value ? '你好，请用一句话介绍自己。' : 'Hello. Introduce yourself in one sentence.' }],
  max_tokens: 256,
}, null, 2)

const openDetail = (model) => {
  detailModel.value = model
  playgroundKey.value = ''
  playgroundBody.value = defaultBody(model)
  playgroundError.value = ''
  playgroundResult.value = ''
  activeCodeTab.value = 'Curl'
}

const closeDetail = () => {
  detailModel.value = null
  playgroundKey.value = ''
  playgroundBody.value = ''
  playgroundError.value = ''
  playgroundResult.value = ''
}

const runPlayground = async () => {
  playgroundError.value = ''
  playgroundResult.value = ''
  if (!playgroundKey.value.trim()) {
    playgroundError.value = labels.value.keyRequired
    return
  }
  let body
  try {
    body = JSON.parse(playgroundBody.value)
  } catch {
    playgroundError.value = labels.value.invalidJson
    return
  }
  if (!body?.model) {
    playgroundError.value = labels.value.modelRequired
    return
  }
  if (body.stream === true) {
    playgroundError.value = labels.value.streamUnsupported
    return
  }
  playgroundLoading.value = true
  try {
    const data = await requestJson('/api/v1/chat/completions', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        'X-API-Key': playgroundKey.value.trim(),
      },
      body: JSON.stringify(body),
    })
    playgroundResult.value = JSON.stringify(data, null, 2)
  } catch (e) {
    playgroundError.value = apiErrorMessage(e, e?.message || labels.value.requestFailed)
  } finally {
    playgroundLoading.value = false
  }
}

const snippetBody = computed(() => detailModel.value ? defaultBody(detailModel.value) : '{}')
const codeExamples = computed(() => ({
  Curl: `curl ${endpoint} \\
  -H "Content-Type: application/json" \\
  -H "X-API-Key: nr-sk-your-key" \\
  -d '${snippetBody.value.replace(/\n/g, '')}'`,
  Python: `import requests\n\nresponse = requests.post(\n    "${endpoint}",\n    headers={\n        "Content-Type": "application/json",\n        "X-API-Key": "nr-sk-your-key",\n    },\n    json=${snippetBody.value.replace(/"([^\"]+)":/g, '"$1":')},\n    timeout=60,\n)\nresponse.raise_for_status()\nprint(response.json())`,
  Node: `const response = await fetch("${endpoint}", {\n  method: "POST",\n  headers: {\n    "Content-Type": "application/json",\n    "X-API-Key": "nr-sk-your-key",\n  },\n  body: JSON.stringify(${snippetBody.value}),\n});\n\nif (!response.ok) throw new Error(\`HTTP \${response.status}\`);\nconsole.log(await response.json());`,
}))
const activeCodeExample = computed(() => codeExamples.value[activeCodeTab.value])

const handleEscape = (event) => {
  if (event.key === 'Escape' && detailModel.value) closeDetail()
}

watch(isZh, () => {
  if (detailModel.value && !playgroundResult.value && !playgroundError.value) {
    playgroundBody.value = defaultBody(detailModel.value)
  }
})

onMounted(async () => {
  window.addEventListener('keydown', handleEscape)
  try {
    const data = await requestJson('/api/models')
    if (data.code !== 0) throw new Error(data.message || labels.value.loadFailed)
    const items = Array.isArray(data.data) ? data.data : data.data?.items || []
    models.value = items.map(normalizeModel).filter((model) => model.model_id)
  } catch (e) {
    error.value = apiErrorMessage(e, e?.message || labels.value.loadFailed)
  } finally {
    loading.value = false
  }
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', handleEscape)
  playgroundKey.value = ''
})
</script>

<style scoped>
.model-filter-panel { margin-bottom: 20px; }
.model-filter-grid { display: grid; grid-template-columns: minmax(240px, 2fr) repeat(4, minmax(140px, 1fr)); gap: 12px; }
.model-filter-grid select { width: 100%; }
.model-filter-meta { display: flex; justify-content: space-between; gap: 12px; margin-top: 14px; color: var(--muted); font-size: 13px; }
.provider-filter-tile { color: var(--fg); text-align: left; cursor: pointer; min-height: 120px; }
.provider-filter-tile.active { border-color: rgba(77,139,247,.62); background: rgba(77,139,247,.12); }
.model-description { min-height: 42px; }
.capability-list { display: flex; flex-wrap: wrap; gap: 8px; }
.model-card-actions { display: flex; justify-content: space-between; gap: 10px; margin-top: auto; }
.model-card-actions .btn { flex: 1; }
.model-card-actions .btn.active { border-color: var(--accent); color: var(--accent); }
.comparison-panel { margin-bottom: 20px; }
.comparison-panel .panel-header p { margin-top: 5px; }
.comparison-table { min-width: 720px; }
.comparison-table td:first-child { color: var(--muted); font-weight: 700; }
.model-detail-backdrop { align-items: start; overflow-y: auto; }
.model-detail-modal { width: min(1040px, 100%); margin: 24px auto; }
.model-detail-header { display: flex; align-items: flex-start; justify-content: space-between; gap: 18px; }
.model-detail-header h3 { margin-top: 10px; }
.detail-fact-grid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 10px; margin: 20px 0; }
.detail-fact-grid > div, .detail-info-grid > div { border: 1px solid var(--border); border-radius: var(--radius); background: #111; padding: 14px; }
.detail-fact-grid span { color: var(--muted); font-size: 12px; }
.detail-fact-grid strong { display: block; margin-top: 5px; }
.detail-info-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; }
.detail-info-grid h4 { margin-bottom: 8px; }
.detail-info-grid p { margin: 0; overflow-wrap: anywhere; }
.endpoint-list { display: flex; flex-wrap: wrap; gap: 8px; }
.endpoint-list span { border: 1px solid var(--border); border-radius: 999px; padding: 5px 9px; color: var(--fg); font-size: 12px; }
.endpoint-list small { color: var(--muted); }
.playground-panel, .code-example-panel { margin-top: 20px; border-top: 1px solid var(--border); padding-top: 20px; }
.playground-panel .form-group { margin-top: 14px; }
.playground-panel small { display: block; margin-top: 6px; color: var(--muted); }
.request-editor { width: 100%; min-height: 190px; resize: vertical; }
.playground-output { margin-top: 14px; max-height: 320px; overflow: auto; }
.code-tab-list { display: flex; gap: 8px; margin-bottom: 12px; }
.code-example-panel .code-card { max-height: 360px; overflow: auto; white-space: pre; }
button:disabled { opacity: .58; cursor: wait; }
@media (max-width: 1080px) {
  .model-filter-grid { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  .model-search { grid-column: 1 / -1; }
}
@media (max-width: 720px) {
  .model-filter-grid, .detail-fact-grid, .detail-info-grid { grid-template-columns: 1fr; }
  .model-filter-meta, .model-detail-header, .model-card-actions { align-items: stretch; flex-direction: column; }
  .model-detail-modal { margin: 0 auto; }
}
</style>
