<template>
  <ShellLayout :title="labels.title" :subtitle="labels.subtitle" admin>
    <div class="metric-grid">
      <div class="metric-card"><div class="label">{{ labels.calls }}</div><div class="value">{{ report.calls }}</div><div class="sub">30D</div></div>
      <div class="metric-card"><div class="label">{{ labels.revenue }}</div><div class="value">${{ money(report.revenue) }}</div><div class="sub">{{ labels.finalBilling }}</div></div>
      <div class="metric-card"><div class="label">{{ labels.cost }}</div><div class="value">${{ money(report.upstream_cost) }}</div><div class="sub">{{ labels.upstreamCost }}</div></div>
      <div class="metric-card"><div class="label">{{ labels.profit }}</div><div class="value">${{ money(report.gross_profit) }}</div><div class="sub">{{ money(report.margin_rate) }}%</div></div>
    </div>

    <div class="panel">
      <div class="panel-header">
        <h3>{{ labels.modelProfit }}</h3>
        <button class="btn btn-ghost btn-sm" @click="fetchReport">{{ labels.refresh }}</button>
      </div>
      <div v-if="loading" class="empty-state">{{ t('loading') }}</div>
      <div v-else-if="error" class="empty-state danger">{{ error }}</div>
      <div v-else class="table-wrap">
        <table class="data-table">
          <thead>
            <tr>
              <th>{{ t('model') }}</th>
              <th>{{ labels.provider }}</th>
              <th>{{ labels.calls }}</th>
              <th>{{ labels.revenue }}</th>
              <th>{{ labels.cost }}</th>
              <th>{{ labels.profit }}</th>
              <th>{{ labels.margin }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="item in report.items" :key="`${item.provider}-${item.model_id}`">
              <td class="mono">{{ item.model_id }}</td>
              <td>{{ item.provider }}</td>
              <td>{{ item.calls }}</td>
              <td>${{ money(item.revenue) }}</td>
              <td>${{ money(item.upstream_cost) }}</td>
              <td class="accent">${{ money(item.gross_profit) }}</td>
              <td>{{ money(item.margin_rate) }}%</td>
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

const { t, isZh } = useI18n()
const loading = ref(true)
const error = ref('')
const report = ref({ calls: 0, revenue: 0, upstream_cost: 0, gross_profit: 0, margin_rate: 0, items: [] })

const labels = computed(() => isZh.value ? {
  title: '利润报表',
  subtitle: '按最终扣费价和上游成本价计算 30 天毛利。',
  calls: '调用',
  revenue: '收入',
  cost: '成本',
  profit: '毛利',
  margin: '毛利率',
  finalBilling: '用户最终扣费',
  upstreamCost: '上游成本',
  modelProfit: '模型利润明细',
  provider: '供应商',
  refresh: '刷新',
  loadFailed: '利润报表加载失败。',
} : {
  title: 'Profit Report',
  subtitle: '30-day gross profit from final billing price and upstream cost.',
  calls: 'Calls',
  revenue: 'Revenue',
  cost: 'Cost',
  profit: 'Profit',
  margin: 'Margin',
  finalBilling: 'final billing',
  upstreamCost: 'upstream cost',
  modelProfit: 'Model profit details',
  provider: 'Provider',
  refresh: 'Refresh',
  loadFailed: 'Failed to load profit report.',
})

const money = (value) => Number(value || 0).toFixed(4).replace(/0+$/, '').replace(/\.$/, '.0')

const fetchReport = async () => {
  loading.value = true
  error.value = ''
  const token = getAuthToken()
  try {
    const res = await fetch('/api/admin/reports/profit', { headers: { Authorization: `Bearer ${token}` } })
    const data = await res.json()
    if (data.code !== 0) {
      error.value = data.message || labels.value.loadFailed
      return
    }
    report.value = data.data || report.value
  } catch (e) {
    error.value = e.message || labels.value.loadFailed
  } finally {
    loading.value = false
  }
}

onMounted(fetchReport)
</script>
