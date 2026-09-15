<template>
  <ShellLayout :title="t('dashboardTitle')" :subtitle="t('dashboardSubtitle')" :balance-text="money(balance)">
    <div v-if="pageError" class="panel empty-state">{{ pageError }}</div>
    <div v-else class="metric-grid">
      <div class="metric-card">
        <div class="label">{{ t('remainingBalance') }}</div>
        <div class="value">{{ money(balance) }}</div>
        <div class="sub success">{{ money(totalRecharged) }} {{ t('totalRecharged') }}</div>
      </div>
      <div class="metric-card">
        <div class="label">{{ t('totalConsumed') }}</div>
        <div class="value">{{ money(totalConsumed) }}</div>
        <div class="sub">{{ t('consumedByModel') }}</div>
      </div>
      <div class="metric-card">
        <div class="label">{{ t('openModels') }}</div>
        <div class="value">{{ modelCount }}</div>
        <div class="sub">{{ t('openModelsSub') }}</div>
      </div>
      <div class="metric-card">
        <div class="label">{{ t('recentCalls') }}</div>
        <div class="value">{{ recentCalls.length }}</div>
        <div class="sub">{{ t('recentCallsSub') }}</div>
      </div>
    </div>

    <div v-if="!pageError" class="dash-grid">
      <div class="panel">
        <div class="panel-header">
          <h3>{{ t('startSteps') }}</h3>
          <span class="badge badge-accent">{{ t('simplePath') }}</span>
        </div>
        <div class="console-steps">
          <router-link v-for="step in steps" :key="step.title" :to="step.to" class="console-step">
            <span>{{ step.no }}</span>
            <div>
              <h4>{{ step.title }}</h4>
              <p>{{ step.desc }}</p>
            </div>
          </router-link>
        </div>
      </div>

      <div class="panel">
        <div class="panel-header">
          <h3>{{ t('unifiedBillingRule') }}</h3>
          <span class="badge badge-success">{{ t('oneWallet') }}</span>
        </div>
        <div class="billing-rule">
          <div v-for="rule in billingRules" :key="rule.no">
            <span>{{ rule.no }}</span>
            <p>{{ rule.text }}</p>
          </div>
        </div>
      </div>

      <div class="panel dash-full">
        <div class="panel-header">
          <h3>{{ t('recentCalls') }}</h3>
          <router-link to="/logs" class="btn btn-ghost btn-sm">{{ t('logs') }}</router-link>
        </div>
        <div v-if="logsLoading" class="empty-state">{{ t('loading') }}</div>
        <div v-else-if="!recentCalls.length" class="empty-state">{{ t('noCallsYet') }}</div>
        <div v-else class="table-wrap">
          <table class="data-table">
            <thead>
              <tr>
                <th>{{ t('model') }}</th>
                <th>{{ t('tokens') }}</th>
                <th>{{ t('cost') }}</th>
                <th>{{ t('latency') }}</th>
                <th>{{ t('status') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="item in recentCalls" :key="item.id">
                <td class="mono">{{ item.model_id }}</td>
                <td>{{ item.input_tokens + item.output_tokens }}</td>
                <td class="accent">{{ money(item.cost_points) }}</td>
                <td>{{ item.latency_ms }}ms</td>
                <td><span :class="['badge', item.status_code === 200 ? 'badge-success' : 'badge-danger']">{{ item.status_code }}</span></td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>

      <div class="panel dash-full">
        <div class="panel-header">
          <h3>{{ t('developerCallShape') }}</h3>
          <span class="badge badge-accent">{{ t('openaiCompatible') }}</span>
        </div>
        <pre class="code-card"><code>curl https://openbridgetech.ca/api/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "X-API-Key: nr-sk-your-key" \
  -d '{
    "model": "gpt-4.1-mini",
    "messages": [{"role": "user", "content": "Hello"}]
  }'</code></pre>
      </div>
    </div>
  </ShellLayout>
</template>

<script setup>
import { computed, onMounted, ref } from 'vue'
import { apiErrorMessage, requestJson } from '../api.js'
import { authHeaders, getAuthToken } from '../auth.js'
import ShellLayout from '../components/ShellLayout.vue'
import { useI18n } from '../i18n.js'

const { isZh, t } = useI18n()
const balance = ref(0)
const totalRecharged = ref(0)
const totalConsumed = ref(0)
const modelCount = ref(0)
const recentCalls = ref([])
const logsLoading = ref(true)
const pageError = ref('')

const steps = computed(() => [
  { no: '01', title: t('stepAddBalanceTitle'), desc: t('stepAddBalanceDesc'), to: '/purchase' },
  { no: '02', title: t('stepCreateKeyTitle'), desc: t('stepCreateKeyDesc'), to: '/keys' },
  { no: '03', title: t('stepChooseModelTitle'), desc: t('stepChooseModelDesc'), to: '/models' },
  { no: '04', title: t('stepCopySampleTitle'), desc: t('stepCopySampleDesc'), to: '/docs' },
])

const billingRules = computed(() => [
  { no: '1', text: t('billingRule1') },
  { no: '2', text: t('billingRule2') },
  { no: '3', text: t('billingRule3') },
])

const money = (value) => `$${Number(value || 0).toFixed(4)}`

const fetchBalance = async () => {
  const data = await requestJson('/api/balance', { headers: authHeaders() })
  if (data.code !== 0) throw new Error(data.message || 'Balance request failed')
  balance.value = data.data.balance
  totalRecharged.value = data.data.total_recharged
  totalConsumed.value = data.data.total_consumed
}

const fetchLogs = async () => {
  try {
    const data = await requestJson('/api/logs/calls', { headers: authHeaders() })
    if (data.code !== 0) throw new Error(data.message || 'Call log request failed')
    recentCalls.value = data.data.items
  } finally {
    logsLoading.value = false
  }
}

const fetchModels = async () => {
  const data = await requestJson('/api/models')
  if (data.code !== 0) throw new Error(data.message || 'Model request failed')
  modelCount.value = data.data.items.length
}

onMounted(async () => {
  try {
    const token = getAuthToken()
    await fetchModels()
    if (!token) {
      logsLoading.value = false
      return
    }
    await Promise.all([fetchBalance(), fetchLogs()])
  } catch (error) {
    logsLoading.value = false
    pageError.value = apiErrorMessage(
      error,
      isZh.value ? '控制台数据加载失败，请稍后重试。' : 'Failed to load dashboard data. Please try again.',
    )
  }
})
</script>
