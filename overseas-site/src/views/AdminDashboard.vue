<template>
  <ShellLayout :title="t('adminPanel')" :subtitle="t('adminSubtitle')" admin>
    <div v-if="loading" class="empty-state">{{ t('loading') }}</div>
    <div v-else-if="error" class="empty-state danger">{{ error }}</div>
    <template v-else>
      <div class="metric-grid">
        <div class="metric-card"><div class="label">{{ labels.revenue30d }}</div><div class="value">{{ money(report.revenue) }}</div><div class="sub">{{ labels.successfulCallsOnly }}</div></div>
        <div class="metric-card"><div class="label">{{ labels.activeUsers }}</div><div class="value">{{ activeUsers }}</div><div class="sub">{{ labels.loadedUsers }}</div></div>
        <div class="metric-card"><div class="label">{{ labels.calls30d }}</div><div class="value">{{ report.calls }}</div><div class="sub">{{ labels.successfulCallsOnly }}</div></div>
        <div class="metric-card"><div class="label">{{ labels.activeKeys }}</div><div class="value">{{ activeKeys }}</div><div class="sub">{{ labels.liveRiskData }}</div></div>
      </div>
      <div class="dash-grid">
        <div class="panel">
          <div class="panel-header"><h3>{{ labels.modelProfit }}</h3><span class="admin-badge">30D</span></div>
          <div v-if="!report.items.length" class="empty-state">{{ labels.noProfitData }}</div>
          <div v-else class="table-wrap">
            <table class="data-table">
              <thead><tr><th>{{ t('model') }}</th><th>{{ labels.provider }}</th><th>{{ labels.calls }}</th><th>{{ labels.revenue }}</th><th>{{ labels.grossProfit }}</th></tr></thead>
              <tbody>
                <tr v-for="item in report.items" :key="`${item.provider}-${item.model_id}`">
                  <td class="mono">{{ item.model_id }}</td>
                  <td>{{ item.provider }}</td>
                  <td>{{ item.calls }}</td>
                  <td>{{ money(item.revenue) }}</td>
                  <td>{{ money(item.gross_profit) }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
        <div class="panel">
          <div class="panel-header"><h3>{{ t('providerHealth') }}</h3></div>
          <div v-if="!providers.length" class="empty-state">{{ labels.noProviders }}</div>
          <div v-else class="table-wrap">
            <table class="data-table">
              <thead><tr><th>{{ labels.provider }}</th><th>{{ labels.health }}</th><th>{{ labels.calls24h }}</th><th>{{ labels.avgLatency }}</th></tr></thead>
              <tbody>
                <tr v-for="provider in providers" :key="provider.id">
                  <td>{{ provider.name }}</td>
                  <td><span :class="['badge', healthClass(provider.health)]">{{ provider.health || '--' }}</span></td>
                  <td>{{ provider.calls_24h }}</td>
                  <td>{{ provider.calls_24h ? `${provider.avg_latency_ms}ms` : '--' }}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      </div>
    </template>
  </ShellLayout>
</template>

<script setup>
import { computed, onMounted, ref } from 'vue'
import { apiErrorMessage, requestJson } from '../api.js'
import { getAuthToken } from '../auth.js'
import ShellLayout from '../components/ShellLayout.vue'
import { useI18n } from '../i18n.js'

const { t, isZh } = useI18n()
const loading = ref(true)
const error = ref('')
const report = ref({ calls: 0, revenue: 0, items: [] })
const users = ref([])
const riskUsers = ref([])
const providers = ref([])
const labels = computed(() => isZh.value ? {
  revenue30d: '近 30 天收入',
  activeUsers: '启用用户',
  calls30d: '近 30 天成功调用',
  activeKeys: '启用密钥',
  successfulCallsOnly: '来自成功调用记录',
  loadedUsers: '来自用户管理数据',
  liveRiskData: '来自真实风控数据',
  modelProfit: '模型利润',
  noProfitData: '暂无成功调用和利润数据。',
  noProviders: '暂无供应商数据。',
  provider: '供应商',
  calls: '调用数',
  revenue: '收入',
  grossProfit: '毛利',
  health: '健康状态',
  calls24h: '24 小时调用',
  avgLatency: '平均延迟',
  loadFailed: '管理首页数据加载失败。',
} : {
  revenue30d: 'Revenue (30d)',
  activeUsers: 'Active users',
  calls30d: 'Successful calls (30d)',
  activeKeys: 'Active keys',
  successfulCallsOnly: 'From successful call records',
  loadedUsers: 'From user management data',
  liveRiskData: 'From live risk data',
  modelProfit: 'Profit by model',
  noProfitData: 'No successful calls or profit data yet.',
  noProviders: 'No provider data available.',
  provider: 'Provider',
  calls: 'Calls',
  revenue: 'Revenue',
  grossProfit: 'Gross profit',
  health: 'Health',
  calls24h: 'Calls (24h)',
  avgLatency: 'Avg latency',
  loadFailed: 'Failed to load admin dashboard data.',
})
const activeUsers = computed(() => users.value.filter((user) => Number(user.status) === 0).length)
const activeKeys = computed(() => riskUsers.value.reduce((sum, user) => sum + Number(user.active_key_count || 0), 0))
const money = (value) => `$${Number(value || 0).toFixed(4)}`
const healthClass = (value) => {
  if (value === 'healthy') return 'badge-success'
  if (value === 'degraded') return 'badge-warn'
  return 'badge-danger'
}

onMounted(async () => {
  const headers = { Authorization: `Bearer ${getAuthToken()}` }
  try {
    const [reportData, usersData, riskData, providersData] = await Promise.all([
      requestJson('/api/admin/reports/profit', { headers }),
      requestJson('/api/admin/users', { headers }),
      requestJson('/api/admin/risk/users', { headers }),
      requestJson('/api/admin/providers', { headers }),
    ])
    const failed = [reportData, usersData, riskData, providersData].find((item) => item.code !== 0)
    if (failed) throw new Error(failed.message || labels.value.loadFailed)
    report.value = { ...report.value, ...(reportData.data || {}), items: reportData.data?.items || [] }
    users.value = usersData.data?.items || []
    riskUsers.value = riskData.data?.items || []
    providers.value = providersData.data?.items || []
  } catch (e) {
    error.value = apiErrorMessage(e, e?.message || labels.value.loadFailed)
  } finally {
    loading.value = false
  }
})
</script>
