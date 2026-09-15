<template>
  <ShellLayout :title="t('balance')" :subtitle="t('featureBillingDesc')" :balance-text="money(balance.balance)">
    <div v-if="loading" class="empty-state">{{ t('loading') }}</div>
    <div v-else-if="error" class="empty-state danger">{{ error }}</div>
    <template v-else>
      <div class="metric-grid">
        <div class="metric-card"><div class="label">{{ t('remainingBalance') }}</div><div class="value">{{ money(balance.balance) }}</div><div class="sub">{{ t('availableNow') }}</div></div>
        <div class="metric-card"><div class="label">{{ t('totalRecharged') }}</div><div class="value">{{ money(balance.total_recharged) }}</div><div class="sub">{{ t('lifetime') }}</div></div>
        <div class="metric-card"><div class="label">{{ t('totalConsumed') }}</div><div class="value">{{ money(balance.total_consumed) }}</div><div class="sub">{{ t('lifetime') }}</div></div>
        <div class="metric-card"><div class="label">{{ labels.frozenBalance }}</div><div class="value">{{ money(balance.frozen_balance) }}</div><div class="sub">{{ labels.fromAccount }}</div></div>
      </div>
      <div class="panel">
        <div class="panel-header"><h3>{{ labels.balanceHistory }}</h3></div>
        <div v-if="!balanceLogs.length" class="empty-state">{{ labels.noBalanceLogs }}</div>
        <div v-else class="table-wrap">
          <table class="data-table">
            <thead><tr><th>{{ labels.type }}</th><th>{{ labels.amount }}</th><th>{{ labels.before }}</th><th>{{ labels.after }}</th><th>{{ t('time') }}</th></tr></thead>
            <tbody>
              <tr v-for="(item, index) in balanceLogs" :key="`${item.created_at}-${index}`">
                <td>{{ item.type || '--' }}</td>
                <td :class="Number(item.amount) >= 0 ? 'success' : 'danger'">{{ signedMoney(item.amount) }}</td>
                <td>{{ money(item.balance_before) }}</td>
                <td>{{ money(item.balance_after) }}</td>
                <td class="mono">{{ formatTime(item.created_at) }}</td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </template>
  </ShellLayout>
</template>

<script setup>
import { computed, onMounted, ref } from 'vue'
import { apiErrorMessage, requestJson } from '../api.js'
import { authHeaders } from '../auth.js'
import ShellLayout from '../components/ShellLayout.vue'
import { useI18n } from '../i18n.js'

const { t, isZh } = useI18n()
const balance = ref({ balance: 0, frozen_balance: 0, total_recharged: 0, total_consumed: 0 })
const balanceLogs = ref([])
const loading = ref(true)
const error = ref('')
const labels = computed(() => isZh.value ? {
  frozenBalance: '冻结余额',
  fromAccount: '来自真实账户数据',
  balanceHistory: '余额流水',
  noBalanceLogs: '暂无余额流水。',
  type: '类型',
  amount: '变动金额',
  before: '变动前',
  after: '变动后',
  loadFailed: '余额数据加载失败。',
} : {
  frozenBalance: 'Frozen balance',
  fromAccount: 'From live account data',
  balanceHistory: 'Balance history',
  noBalanceLogs: 'No balance history yet.',
  type: 'Type',
  amount: 'Amount',
  before: 'Before',
  after: 'After',
  loadFailed: 'Failed to load balance data.',
})
const money = (value) => `$${Number(value || 0).toFixed(4)}`
const signedMoney = (value) => `${Number(value) >= 0 ? '+' : '-'}$${Math.abs(Number(value || 0)).toFixed(4)}`
const formatTime = (value) => value ? new Date(value).toLocaleString() : '--'

onMounted(async () => {
  try {
    const headers = authHeaders()
    const [balanceData, logsData] = await Promise.all([
      requestJson('/api/balance', { headers }),
      requestJson('/api/balance/logs', { headers }),
    ])
    if (balanceData.code !== 0) throw new Error(balanceData.message || labels.value.loadFailed)
    if (logsData.code !== 0) throw new Error(logsData.message || labels.value.loadFailed)
    balance.value = { ...balance.value, ...(balanceData.data || {}) }
    balanceLogs.value = logsData.data?.items || []
  } catch (e) {
    error.value = apiErrorMessage(e, e?.message || labels.value.loadFailed)
  } finally {
    loading.value = false
  }
})
</script>
