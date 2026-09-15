<template>
  <ShellLayout :title="labels.title" :subtitle="labels.subtitle" admin>
    <div class="metric-grid">
      <div class="metric-card">
        <div class="label">{{ labels.users }}</div>
        <div class="value">{{ summary.total_users }}</div>
        <div class="sub">{{ labels.top100 }}</div>
      </div>
      <div class="metric-card">
        <div class="label">{{ labels.highRisk }}</div>
        <div class="value">{{ summary.high_count }}</div>
        <div class="sub">{{ labels.needCheck }}</div>
      </div>
      <div class="metric-card">
        <div class="label">{{ labels.watch }}</div>
        <div class="value">{{ summary.watch_count }}</div>
        <div class="sub">{{ labels.keepWatching }}</div>
      </div>
      <div class="metric-card">
        <div class="label">{{ labels.calls24h }}</div>
        <div class="value">{{ summary.calls_24h }}</div>
        <div class="sub">${{ money(summary.spend_24h) }}</div>
      </div>
    </div>

    <div class="panel">
      <div class="panel-header">
        <h3>{{ labels.userRisk }}</h3>
        <button class="btn btn-ghost btn-sm" @click="fetchRisk">{{ labels.refresh }}</button>
      </div>

      <div v-if="loading" class="empty-state">{{ t('loading') }}</div>
      <div v-else-if="error" class="empty-state danger">{{ error }}</div>
      <div v-else class="table-wrap">
        <table class="data-table risk-table">
          <thead>
            <tr>
              <th>{{ labels.risk }}</th>
              <th>{{ labels.user }}</th>
              <th>{{ labels.balance }}</th>
              <th>{{ labels.keys }}</th>
              <th>{{ labels.calls24h }}</th>
              <th>{{ labels.failed }}</th>
              <th>{{ labels.spend24h }}</th>
              <th>{{ labels.status }}</th>
              <th>{{ labels.lastCall }}</th>
              <th>{{ labels.reason }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="user in users" :key="user.id">
              <td>
                <span class="badge" :class="riskClass(user.risk_level)">
                  {{ riskText(user.risk_level) }}
                </span>
              </td>
              <td>
                <strong>{{ user.email }}</strong>
                <div class="muted mono">{{ user.uid }}</div>
              </td>
              <td>${{ money(user.balance) }}</td>
              <td>{{ user.active_key_count }} / {{ user.key_count }}</td>
              <td>{{ user.calls_24h }}</td>
              <td>{{ user.failed_calls_24h }} · {{ percent(user.fail_rate) }}</td>
              <td>${{ money(user.spend_24h) }}</td>
              <td>{{ statusText(user.status) }}</td>
              <td class="mono">{{ formatTime(user.last_call_at) }}</td>
              <td>{{ reasonText(user.risk_reasons) }}</td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>

    <div class="docs-rule-grid">
      <div class="rule-card">
        <span>{{ labels.highRule }}</span>
        <p>{{ labels.highRuleDesc }}</p>
      </div>
      <div class="rule-card">
        <span>{{ labels.watchRule }}</span>
        <p>{{ labels.watchRuleDesc }}</p>
      </div>
      <div class="rule-card">
        <span>{{ labels.noAutoBlock }}</span>
        <p>{{ labels.noAutoBlockDesc }}</p>
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
const users = ref([])
const summary = ref({
  total_users: 0,
  high_count: 0,
  watch_count: 0,
  calls_24h: 0,
  spend_24h: 0,
})

const labels = computed(() => isZh.value ? {
  title: '风控中心',
  subtitle: '查看用户调用异常、失败率、余额消耗和 Key 使用情况。',
  users: '用户数',
  top100: '按风险排序前 100',
  highRisk: '高风险',
  needCheck: '需要人工确认',
  watch: '观察',
  keepWatching: '持续关注',
  calls24h: '24 小时调用',
  userRisk: '用户风险列表',
  refresh: '刷新',
  risk: '风险',
  user: '用户',
  balance: '余额',
  keys: '启用 Key / 总 Key',
  failed: '失败',
  spend24h: '24 小时消费',
  status: '状态',
  lastCall: '最近调用',
  reason: '原因',
  normal: '正常',
  watchLevel: '观察',
  high: '高风险',
  disabled: '已停用',
  active: '正常',
  inactive: '停用',
  none: '无',
  never: '暂无',
  loadFailed: '风控数据加载失败。',
  highRule: '高风险规则',
  highRuleDesc: '24 小时调用超过 1000 次、消费超过 $50，或失败率超过 50%。',
  watchRule: '观察规则',
  watchRuleDesc: '24 小时调用超过 300 次、消费超过 $10，或失败率超过 25%。',
  noAutoBlock: '处理方式',
  noAutoBlockDesc: '这里只做风险展示，不自动封停；封停和恢复仍在用户管理里操作。',
  reasonMap: {
    high_call_volume: '调用量过高',
    high_spend_24h: '消费过高',
    high_failure_rate: '失败率过高',
    call_volume_watch: '调用量需观察',
    spend_watch: '消费需观察',
    failure_rate_watch: '失败率需观察',
  },
} : {
  title: 'Risk Control',
  subtitle: 'Watch abnormal usage, failure rate, balance spend and API key activity.',
  users: 'Users',
  top100: 'Top 100 by risk',
  highRisk: 'High Risk',
  needCheck: 'manual review',
  watch: 'Watch',
  keepWatching: 'monitor',
  calls24h: '24h Calls',
  userRisk: 'User risk list',
  refresh: 'Refresh',
  risk: 'Risk',
  user: 'User',
  balance: 'Balance',
  keys: 'Active / Total Keys',
  failed: 'Failed',
  spend24h: '24h Spend',
  status: 'Status',
  lastCall: 'Last Call',
  reason: 'Reason',
  normal: 'Normal',
  watchLevel: 'Watch',
  high: 'High',
  disabled: 'Disabled',
  active: 'Active',
  inactive: 'Inactive',
  none: 'None',
  never: 'Never',
  loadFailed: 'Failed to load risk data.',
  highRule: 'High risk rule',
  highRuleDesc: '24h calls over 1000, spend over $50, or failure rate over 50%.',
  watchRule: 'Watch rule',
  watchRuleDesc: '24h calls over 300, spend over $10, or failure rate over 25%.',
  noAutoBlock: 'Handling',
  noAutoBlockDesc: 'This page only shows risk. Disable and restore users from the user management page.',
  reasonMap: {
    high_call_volume: 'high call volume',
    high_spend_24h: 'high spend',
    high_failure_rate: 'high failure rate',
    call_volume_watch: 'call volume watch',
    spend_watch: 'spend watch',
    failure_rate_watch: 'failure rate watch',
  },
})

const money = (value) => Number(value || 0).toFixed(4).replace(/0+$/, '').replace(/\.$/, '.0')
const percent = (value) => `${(Number(value || 0) * 100).toFixed(1)}%`
const formatTime = (value) => value ? new Date(value).toLocaleString() : labels.value.never
const statusText = (status) => Number(status) === 0 ? labels.value.active : labels.value.inactive
const riskText = (level) => ({
  high: labels.value.high,
  watch: labels.value.watchLevel,
  disabled: labels.value.disabled,
  normal: labels.value.normal,
}[level] || labels.value.normal)

const riskClass = (level) => ({
  high: 'badge-danger',
  watch: 'badge-warn',
  disabled: 'badge-accent',
  normal: 'badge-success',
}[level] || 'badge-success')

const reasonText = (reasons = []) => {
  if (!Array.isArray(reasons) || reasons.length === 0) return labels.value.none
  return reasons.map((reason) => labels.value.reasonMap[reason] || reason).join(', ')
}

const fetchRisk = async () => {
  loading.value = true
  error.value = ''
  const token = getAuthToken()
  try {
    const res = await fetch('/api/admin/risk/users', {
      headers: { Authorization: `Bearer ${token}` },
    })
    const data = await res.json()
    if (data.code !== 0) {
      error.value = data.message || labels.value.loadFailed
      return
    }
    summary.value = data.data || summary.value
    users.value = data.data?.items || []
  } catch (e) {
    error.value = e.message || labels.value.loadFailed
  } finally {
    loading.value = false
  }
}

onMounted(fetchRisk)
</script>

<style scoped>
.risk-table { min-width: 1120px; }
.muted { color: var(--muted); margin-top: 4px; font-size: 12px; }
</style>
