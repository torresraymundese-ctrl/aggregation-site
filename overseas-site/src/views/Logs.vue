<template>
  <ShellLayout :title="t('logs')" :subtitle="t('logsSubtitle')" :admin="admin">
    <div class="metric-grid">
      <div class="metric-card"><div class="label">{{ labels.shownCalls }}</div><div class="value">{{ logs.length }}</div><div class="sub">{{ labels.currentList }}</div></div>
      <div class="metric-card"><div class="label">{{ t('tokens') }}</div><div class="value">{{ totalTokens }}</div><div class="sub">{{ labels.currentList }}</div></div>
      <div class="metric-card"><div class="label">{{ t('cost') }}</div><div class="value">${{ totalCost }}</div><div class="sub">{{ labels.currentList }}</div></div>
      <div class="metric-card"><div class="label">{{ t('successRate') }}</div><div class="value">{{ successRate }}</div><div class="sub">{{ labels.currentList }}</div></div>
    </div>
    <div class="panel">
      <div class="panel-header"><h3>{{ t('recentCalls') }}</h3></div>
      <div v-if="loading" class="empty-state">{{ t('loading') }}</div>
      <div v-else-if="error" class="empty-state danger">{{ error }}</div>
      <div v-else-if="!logs.length" class="empty-state">{{ labels.noLogs }}</div>
      <div v-else class="table-wrap">
        <table class="data-table">
          <thead><tr><th>{{ t('time') }}</th><th>{{ t('model') }}</th><th>{{ t('tokens') }}</th><th>{{ t('cost') }}</th><th>{{ t('latency') }}</th><th>{{ t('status') }}</th></tr></thead>
          <tbody>
            <tr v-for="log in logs" :key="log.id">
              <td class="mono">{{ formatTime(log.created_at) }}</td>
              <td class="mono">{{ log.model_id || log.model }}</td>
              <td>{{ (log.input_tokens || 0) + (log.output_tokens || 0) }}</td>
              <td class="accent">${{ Number(log.cost_points || 0).toFixed(4) }}</td>
              <td>{{ log.latency_ms || 0 }}ms</td>
              <td><span :class="['badge', statusClass(log.status_code)]">{{ statusText(log.status_code) }}</span></td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </ShellLayout>
</template>

<script setup>
import { computed, onMounted, ref } from 'vue'
import { apiErrorMessage, requestJson } from '../api.js'
import { authHeaders } from '../auth.js'
import ShellLayout from '../components/ShellLayout.vue'
import { useI18n } from '../i18n.js'

const { t, isZh } = useI18n()
defineProps({
  admin: { type: Boolean, default: false },
})
const logs = ref([])
const loading = ref(true)
const error = ref('')
const labels = computed(() => isZh.value ? {
  shownCalls: '已显示调用',
  currentList: '根据当前真实记录计算',
  noLogs: '暂无调用记录。',
  loadFailed: '调用记录加载失败。',
} : {
  shownCalls: 'Calls shown',
  currentList: 'Calculated from the records shown',
  noLogs: 'No call records yet.',
  loadFailed: 'Failed to load call records.',
})
const totalTokens = computed(() => logs.value.reduce((sum, item) => sum + (item.input_tokens || 0) + (item.output_tokens || 0), 0))
const totalCost = computed(() => logs.value.reduce((sum, item) => sum + Number(item.cost_points || 0), 0).toFixed(4))
const successRate = computed(() => {
  if (!logs.value.length) return '--'
  const succeeded = logs.value.filter((item) => Number(item.status_code) >= 200 && Number(item.status_code) < 400).length
  return `${(succeeded / logs.value.length * 100).toFixed(1)}%`
})
const formatTime = (value) => value ? new Date(value).toLocaleString() : '--'
const statusText = (value) => Number.isFinite(Number(value)) ? String(value) : '--'
const statusClass = (value) => Number(value) >= 200 && Number(value) < 400 ? 'badge-success' : 'badge-danger'

onMounted(async () => {
  try {
    const data = await requestJson('/api/logs/calls', { headers: authHeaders() })
    if (data.code !== 0) throw new Error(data.message || labels.value.loadFailed)
    logs.value = data.data?.items || []
  } catch (e) {
    error.value = apiErrorMessage(e, e?.message || labels.value.loadFailed)
  } finally {
    loading.value = false
  }
})
</script>
