<template>
  <ShellLayout :title="labels.title" :subtitle="labels.subtitle" admin>
    <div class="metric-grid">
      <div class="metric-card">
        <div class="label">{{ labels.providers }}</div>
        <div class="value">{{ providers.length }}</div>
        <div class="sub">{{ labels.totalSources }}</div>
      </div>
      <div class="metric-card">
        <div class="label">{{ labels.healthy }}</div>
        <div class="value">{{ healthyCount }}</div>
        <div class="sub">{{ labels.fromRealCalls }}</div>
      </div>
      <div class="metric-card">
        <div class="label">{{ labels.keyReady }}</div>
        <div class="value">{{ keyReadyCount }}</div>
        <div class="sub">{{ labels.envConfigured }}</div>
      </div>
      <div class="metric-card">
        <div class="label">{{ labels.calls24h }}</div>
        <div class="value">{{ calls24h }}</div>
        <div class="sub">{{ labels.providerTraffic }}</div>
      </div>
    </div>

    <div class="panel">
      <div class="panel-header">
        <div>
          <h3>{{ labels.providerTable }}</h3>
          <p class="muted admin-provider-note">{{ labels.note }}</p>
        </div>
        <button class="btn btn-ghost btn-sm" @click="fetchProviders">{{ labels.refresh }}</button>
      </div>

      <div v-if="loading" class="empty-state">{{ t('loading') }}</div>
      <div v-else-if="error" class="empty-state danger">{{ error }}</div>
      <div v-else class="table-wrap">
        <table class="data-table admin-provider-table">
          <thead>
            <tr>
              <th>{{ labels.provider }}</th>
              <th>{{ labels.baseUrl }}</th>
              <th>{{ labels.keyEnv }}</th>
              <th>{{ labels.keyStatus }}</th>
              <th>{{ labels.health }}</th>
              <th>{{ labels.models }}</th>
              <th>{{ labels.calls }}</th>
              <th>{{ t('status') }}</th>
              <th>{{ t('actions') }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="provider in providers" :key="provider.id">
              <td>
                <input v-model="drafts[provider.id].name" class="provider-input name-input" />
                <span class="mono muted provider-id-line">{{ provider.provider_id }}</span>
              </td>
              <td><input v-model="drafts[provider.id].base_url" class="provider-input url-input" /></td>
              <td><input v-model="drafts[provider.id].api_key_env" class="provider-input env-input" /></td>
              <td>
                <span :class="['badge', provider.key_configured ? 'badge-success' : 'badge-warn']">
                  {{ provider.key_configured ? labels.configured : labels.missing }}
                </span>
              </td>
              <td><span :class="['badge', healthBadge(provider.health)]">{{ healthText(provider.health) }}</span></td>
              <td>{{ provider.active_model_count }}/{{ provider.model_count }}</td>
              <td>
                <strong>{{ provider.calls_24h }}</strong>
                <span class="mono muted provider-id-line">{{ provider.failed_calls_24h }} {{ labels.failed }}</span>
              </td>
              <td>
                <select v-model.number="drafts[provider.id].status" class="provider-select">
                  <option :value="0">{{ t('active') }}</option>
                  <option :value="1">{{ t('disabled') }}</option>
                </select>
              </td>
              <td>
                <button class="btn btn-primary btn-sm" :disabled="savingId === provider.id" @click="saveProvider(provider)">
                  {{ savingId === provider.id ? labels.saving : labels.save }}
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
const providers = ref([])
const drafts = ref({})
const loading = ref(true)
const error = ref('')
const savingId = ref(null)

const labels = computed(() => isZh.value ? {
  title: '供应商管理',
  subtitle: '管理上游地址、Key 环境变量、启停状态和真实调用健康。',
  providers: '供应商',
  totalSources: '上游来源',
  healthy: '健康',
  fromRealCalls: '按真实调用计算',
  keyReady: 'Key 已配置',
  envConfigured: '环境变量存在',
  calls24h: '24 小时调用',
  providerTraffic: '供应商流量',
  providerTable: '供应商列表',
  note: '这里不保存明文 Key，只维护环境变量名。真实 Key 仍放在服务器环境变量里。',
  refresh: '刷新',
  provider: '供应商',
  baseUrl: '上游地址',
  keyEnv: 'Key 环境变量',
  keyStatus: 'Key 状态',
  health: '健康',
  models: '模型',
  calls: '调用',
  failed: '失败',
  configured: '已配置',
  missing: '缺失',
  save: '保存',
  saving: '保存中',
  saved: '已保存。',
  saveFailed: '供应商保存失败。',
  loadFailed: '供应商加载失败。',
  healthyText: '健康',
  degraded: '波动',
  down: '熔断',
  keyMissing: '缺 Key',
  unverified: '待验证',
  disabled: '停用',
} : {
  title: 'Provider Management',
  subtitle: 'Manage upstream URLs, key env names, status and real-call health.',
  providers: 'Providers',
  totalSources: 'upstream sources',
  healthy: 'Healthy',
  fromRealCalls: 'from real calls',
  keyReady: 'Keys ready',
  envConfigured: 'env configured',
  calls24h: '24h calls',
  providerTraffic: 'provider traffic',
  providerTable: 'Provider table',
  note: 'Plain keys are not stored here. This page keeps the env var name; real keys remain in server environment variables.',
  refresh: 'Refresh',
  provider: 'Provider',
  baseUrl: 'Base URL',
  keyEnv: 'Key env',
  keyStatus: 'Key status',
  health: 'Health',
  models: 'Models',
  calls: 'Calls',
  failed: 'failed',
  configured: 'Configured',
  missing: 'Missing',
  save: 'Save',
  saving: 'Saving',
  saved: 'Saved.',
  saveFailed: 'Failed to save provider.',
  loadFailed: 'Failed to load providers.',
  healthyText: 'Healthy',
  degraded: 'Degraded',
  down: 'Circuit',
  keyMissing: 'No key',
  unverified: 'Unverified',
  disabled: 'Disabled',
})

const healthyCount = computed(() => providers.value.filter((provider) => provider.health === 'healthy').length)
const keyReadyCount = computed(() => providers.value.filter((provider) => provider.key_configured).length)
const calls24h = computed(() => providers.value.reduce((sum, provider) => sum + Number(provider.calls_24h || 0), 0))

const buildDrafts = (items) => {
  const next = {}
  items.forEach((provider) => {
    next[provider.id] = {
      name: provider.name || '',
      base_url: provider.base_url || '',
      api_key_env: provider.api_key_env || '',
      status: Number(provider.status),
      sort: Number(provider.sort || 0),
    }
  })
  drafts.value = next
}

const healthText = (value) => ({
  healthy: labels.value.healthyText,
  degraded: labels.value.degraded,
  down: labels.value.down,
  key_missing: labels.value.keyMissing,
  unverified: labels.value.unverified,
  disabled: labels.value.disabled,
}[value] || value)

const healthBadge = (value) => {
  if (value === 'healthy') return 'badge-success'
  if (value === 'degraded' || value === 'unverified' || value === 'key_missing') return 'badge-warn'
  return 'badge-danger'
}

const fetchProviders = async () => {
  loading.value = true
  error.value = ''
  const token = getAuthToken()
  try {
    const res = await fetch('/api/admin/providers', { headers: { Authorization: `Bearer ${token}` } })
    const data = await res.json()
    if (data.code !== 0) {
      error.value = data.message || labels.value.loadFailed
      return
    }
    providers.value = data.data?.items || []
    buildDrafts(providers.value)
  } catch (e) {
    error.value = e.message || labels.value.loadFailed
  } finally {
    loading.value = false
  }
}

const saveProvider = async (provider) => {
  const draft = drafts.value[provider.id]
  if (!draft) return
  const token = getAuthToken()
  savingId.value = provider.id
  try {
    const res = await fetch(`/api/admin/providers/${provider.id}`, {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${token}` },
      body: JSON.stringify(draft),
    })
    const data = await res.json()
    if (data.code !== 0) {
      notifyError(data.message || labels.value.saveFailed)
      return
    }
    notifySuccess(labels.value.saved)
    await fetchProviders()
  } catch (e) {
    notifyError(e.message || labels.value.saveFailed)
  } finally {
    savingId.value = null
  }
}

onMounted(fetchProviders)
</script>

<style scoped>
.admin-provider-note { margin-top: 6px; font-size: 13px; }
.admin-provider-table { min-width: 1240px; }
.provider-id-line { display: block; margin-top: 4px; font-size: 12px; }
.provider-input, .provider-select {
  width: 100%;
  color: var(--fg);
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 8px 10px;
}
.name-input { min-width: 150px; }
.url-input { min-width: 260px; }
.env-input { min-width: 230px; }
.provider-select { min-width: 96px; }
button:disabled { opacity: .58; cursor: wait; }
</style>
