<template>
  <ShellLayout :title="labels.title" :subtitle="labels.subtitle" admin>
    <div class="metric-grid">
      <div class="metric-card">
        <div class="label">{{ labels.totalModels }}</div>
        <div class="value">{{ models.length }}</div>
        <div class="sub">{{ labels.configured }}</div>
      </div>
      <div class="metric-card">
        <div class="label">{{ labels.activeModels }}</div>
        <div class="value">{{ activeCount }}</div>
        <div class="sub">{{ labels.visibleToUsers }}</div>
      </div>
      <div class="metric-card">
        <div class="label">{{ labels.avgMargin }}</div>
        <div class="value">{{ averageMargin }}%</div>
        <div class="sub">{{ labels.platformBuffer }}</div>
      </div>
      <div class="metric-card">
        <div class="label">{{ labels.providers }}</div>
        <div class="value">{{ providerCount }}</div>
        <div class="sub">{{ labels.connectedSources }}</div>
      </div>
    </div>

    <div class="panel">
      <div class="panel-header">
        <div>
          <h3>{{ labels.pricingTable }}</h3>
          <p class="muted admin-model-note">{{ labels.rule }}</p>
        </div>
        <button class="btn btn-ghost btn-sm" @click="fetchModels">{{ labels.refresh }}</button>
      </div>

      <div v-if="loading" class="empty-state">{{ t('loading') }}</div>
      <div v-else-if="error" class="empty-state danger">{{ error }}</div>
      <div v-else class="table-wrap">
        <table class="data-table admin-pricing-table">
          <thead>
            <tr>
              <th>{{ labels.provider }}</th>
              <th>{{ t('model') }}</th>
              <th>{{ labels.upstreamInput }}</th>
              <th>{{ labels.upstreamOutput }}</th>
              <th>{{ labels.margin }}</th>
              <th>{{ labels.finalInput }}</th>
              <th>{{ labels.finalOutput }}</th>
              <th>{{ t('status') }}</th>
              <th>{{ t('actions') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="model in models" :key="model.id">
              <td>{{ model.provider }}</td>
              <td>
                <strong>{{ displayModelName(model) }}</strong>
                <span class="mono muted model-id-line">{{ model.model_id }}</span>
              </td>
              <td>
                <input
                  v-model.number="drafts[model.id].upstream_input_rate"
                  class="pricing-input"
                  type="number"
                  min="0"
                  step="0.000001"
                />
              </td>
              <td>
                <input
                  v-model.number="drafts[model.id].upstream_output_rate"
                  class="pricing-input"
                  type="number"
                  min="0"
                  step="0.000001"
                />
              </td>
              <td>
                <div class="margin-input-wrap">
                  <input
                    v-model.number="drafts[model.id].margin_rate"
                    class="pricing-input"
                    type="number"
                    min="0"
                    max="9999"
                    step="0.01"
                  />
                  <span>%</span>
                </div>
              </td>
              <td class="accent mono">${{ finalRate(model.id, 'input') }}</td>
              <td class="accent mono">${{ finalRate(model.id, 'output') }}</td>
              <td>
                <select v-model.number="drafts[model.id].status" class="pricing-select">
                  <option :value="0">{{ t('active') }}</option>
                  <option :value="1">{{ t('disabled') }}</option>
                </select>
              </td>
              <td>
                <button class="btn btn-primary btn-sm" :disabled="savingId === model.id" @click="savePricing(model)">
                  {{ savingId === model.id ? labels.saving : labels.save }}
                </button>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </ShellLayout>
</template>

<script setup>
import { computed, onMounted, ref } from 'vue'
import { getAuthToken } from '../auth.js'
import ShellLayout from '../components/ShellLayout.vue'
import { useI18n } from '../i18n.js'
import { notifyError, notifySuccess } from '../notify.js'

const { t, isZh } = useI18n()
const models = ref([])
const drafts = ref({})
const loading = ref(true)
const error = ref('')
const savingId = ref(null)

const labels = computed(() => isZh.value ? {
  title: '模型定价配置',
  subtitle: '配置上游成本价、平台余量和用户最终扣费价格。',
  totalModels: '模型总数',
  activeModels: '启用模型',
  avgMargin: '平均余量',
  providers: '供应商',
  configured: '已配置',
  visibleToUsers: '用户可见',
  platformBuffer: '平台加价空间',
  connectedSources: '已接入来源',
  pricingTable: '模型价格表',
  rule: '最终售价 = 上游成本价 × (1 + 平台余量百分比)。用户调用时按最终售价扣统一余额。',
  refresh: '刷新',
  provider: '供应商',
  upstreamInput: '输入成本 / 每 100 万 Token',
  upstreamOutput: '输出成本 / 每 100 万 Token',
  margin: '平台余量',
  finalInput: '输入售价 / 每 100 万 Token',
  finalOutput: '输出售价 / 每 100 万 Token',
  save: '保存',
  saving: '保存中',
  saved: '已保存。',
  loadFailed: '模型定价加载失败。',
  saveFailed: '模型定价保存失败。',
  invalidNumber: '请填写合法的非负数字。',
} : {
  title: 'Model Pricing',
  subtitle: 'Configure upstream cost, platform margin and final user billing price.',
  totalModels: 'Total models',
  activeModels: 'Active models',
  avgMargin: 'Avg margin',
  providers: 'Providers',
  configured: 'configured',
  visibleToUsers: 'visible to users',
  platformBuffer: 'platform buffer',
  connectedSources: 'connected sources',
  pricingTable: 'Model pricing table',
  rule: 'Final price = upstream cost x (1 + platform margin). Calls deduct the final price from the unified balance.',
  refresh: 'Refresh',
  provider: 'Provider',
  upstreamInput: 'Input cost / 1M tokens',
  upstreamOutput: 'Output cost / 1M tokens',
  margin: 'Margin',
  finalInput: 'Input price / 1M tokens',
  finalOutput: 'Output price / 1M tokens',
  save: 'Save',
  saving: 'Saving',
  saved: 'Saved.',
  loadFailed: 'Failed to load model pricing.',
  saveFailed: 'Failed to save model pricing.',
  invalidNumber: 'Please enter valid non-negative numbers.',
})

const activeCount = computed(() => models.value.filter((model) => model.status === 0).length)
const providerCount = computed(() => new Set(models.value.map((model) => model.provider_id)).size)
const averageMargin = computed(() => {
  if (!models.value.length) return '0.00'
  const total = models.value.reduce((sum, model) => sum + Number(model.margin_rate || 0), 0)
  return (total / models.value.length).toFixed(2)
})

const displayModelName = (model) => {
  const name = String(model?.display_name || model?.model_id || '')
  return name.replace(/\s+Placeholder$/i, '')
}

const money = (value) => Number(value || 0).toFixed(6).replace(/0+$/, '').replace(/\.$/, '.0')

const isValidPrice = (value) => Number.isFinite(Number(value)) && Number(value) >= 0

const finalRate = (id, type) => {
  const draft = drafts.value[id]
  if (!draft) return '0.0'
  const base = type === 'input' ? draft.upstream_input_rate : draft.upstream_output_rate
  if (!isValidPrice(base) || !isValidPrice(draft.margin_rate)) return '--'
  return money(Number(base) * (1 + Number(draft.margin_rate) / 100))
}

const buildDrafts = (items) => {
  const nextDrafts = {}
  items.forEach((model) => {
    nextDrafts[model.id] = {
      upstream_input_rate: Number(model.upstream_input_rate),
      upstream_output_rate: Number(model.upstream_output_rate),
      margin_rate: Number(model.margin_rate),
      status: Number(model.status),
    }
  })
  drafts.value = nextDrafts
}

const fetchModels = async () => {
  loading.value = true
  error.value = ''
  const token = getAuthToken()
  try {
    const res = await fetch('/api/admin/models', { headers: { Authorization: `Bearer ${token}` } })
    const data = await res.json()
    if (data.code !== 0) {
      error.value = data.message || labels.value.loadFailed
      return
    }
    models.value = data.data?.items || []
    buildDrafts(models.value)
  } catch (e) {
    error.value = e.message || labels.value.loadFailed
  } finally {
    loading.value = false
  }
}

const savePricing = async (model) => {
  const draft = drafts.value[model.id]
  if (!draft) return
  if (![draft.upstream_input_rate, draft.upstream_output_rate, draft.margin_rate].every(isValidPrice)) {
    notifyError(labels.value.invalidNumber)
    return
  }

  const token = getAuthToken()
  savingId.value = model.id
  try {
    const res = await fetch(`/api/admin/models/${model.id}/pricing`, {
      method: 'PUT',
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${token}`,
      },
      body: JSON.stringify({
        upstream_input_rate: Number(draft.upstream_input_rate),
        upstream_output_rate: Number(draft.upstream_output_rate),
        margin_rate: Number(draft.margin_rate),
        status: Number(draft.status),
      }),
    })
    const data = await res.json()
    if (data.code !== 0) {
      notifyError(data.message || labels.value.saveFailed)
      return
    }
    notifySuccess(labels.value.saved)
    await fetchModels()
  } catch (e) {
    notifyError(e.message || labels.value.saveFailed)
  } finally {
    savingId.value = null
  }
}

onMounted(fetchModels)
</script>

<style scoped>
.admin-model-note {
  margin-top: 6px;
  max-width: 840px;
  font-size: 13px;
}

.admin-pricing-table {
  min-width: 1120px;
}

.model-id-line {
  display: block;
  margin-top: 4px;
  font-size: 12px;
}

.pricing-input,
.pricing-select {
  width: 118px;
  color: var(--fg);
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 8px 10px;
}

.pricing-select {
  width: 104px;
}

.pricing-input:focus,
.pricing-select:focus {
  border-color: var(--accent);
}

.margin-input-wrap {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--muted);
}

button:disabled {
  opacity: .58;
  cursor: wait;
}
</style>
