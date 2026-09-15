<template>
  <ShellLayout :title="labels.title" :subtitle="labels.subtitle" admin>
    <div class="panel">
      <div class="panel-header">
        <h3>{{ labels.latest }}</h3>
        <button class="btn btn-ghost btn-sm" @click="fetchLogs">{{ labels.refresh }}</button>
      </div>
      <div v-if="loading" class="empty-state">{{ t('loading') }}</div>
      <div v-else-if="error" class="empty-state danger">{{ error }}</div>
      <div v-else class="table-wrap">
        <table class="data-table audit-table">
          <thead>
            <tr>
              <th>{{ t('time') }}</th>
              <th>{{ labels.admin }}</th>
              <th>{{ labels.action }}</th>
              <th>{{ labels.target }}</th>
              <th>{{ labels.after }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="log in logs" :key="log.id">
              <td class="mono">{{ formatTime(log.created_at) }}</td>
              <td>{{ log.admin_email }}</td>
              <td><span class="badge badge-accent">{{ log.action }}</span></td>
              <td class="mono">{{ log.target_type }} #{{ log.target_id }}</td>
              <td class="mono audit-json">{{ compactJson(log.after_data) }}</td>
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
const logs = ref([])
const loading = ref(true)
const error = ref('')

const labels = computed(() => isZh.value ? {
  title: '审计日志',
  subtitle: '记录管理员修改供应商、模型价格和上下架的操作。',
  latest: '最近操作',
  refresh: '刷新',
  admin: '管理员',
  action: '动作',
  target: '对象',
  after: '变更后',
  loadFailed: '审计日志加载失败。',
} : {
  title: 'Audit Logs',
  subtitle: 'Track admin changes to providers, model pricing and availability.',
  latest: 'Latest changes',
  refresh: 'Refresh',
  admin: 'Admin',
  action: 'Action',
  target: 'Target',
  after: 'After',
  loadFailed: 'Failed to load audit logs.',
})

const formatTime = (value) => value ? new Date(value).toLocaleString() : '--'
const compactJson = (value) => {
  if (!value) return '--'
  const text = JSON.stringify(value)
  return text.length > 180 ? `${text.slice(0, 180)}...` : text
}

const fetchLogs = async () => {
  loading.value = true
  error.value = ''
  const token = getAuthToken()
  try {
    const res = await fetch('/api/admin/audit-logs', { headers: { Authorization: `Bearer ${token}` } })
    const data = await res.json()
    if (data.code !== 0) {
      error.value = data.message || labels.value.loadFailed
      return
    }
    logs.value = data.data?.items || []
  } catch (e) {
    error.value = e.message || labels.value.loadFailed
  } finally {
    loading.value = false
  }
}

onMounted(fetchLogs)
</script>

<style scoped>
.audit-table { min-width: 980px; }
.audit-json { max-width: 480px; overflow-wrap: anywhere; color: var(--muted); font-size: 12px; }
</style>
