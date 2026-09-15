<template>
  <ShellLayout :title="t('users')" :subtitle="t('usersSubtitle')" admin>
    <div class="panel">
      <div class="panel-header"><h3>{{ t('users') }}</h3><span class="admin-badge">{{ users.length }} {{ t('usersCount') }}</span></div>
      <div v-if="loading" class="empty-state">{{ t('loading') }}</div>
      <div v-else class="table-wrap">
        <table class="data-table">
          <thead><tr><th>ID</th><th>{{ t('email') }}</th><th>{{ t('nickname') }}</th><th>{{ t('status') }}</th><th>{{ t('time') }}</th><th>{{ t('action') }}</th></tr></thead>
          <tbody>
            <tr v-for="user in users" :key="user.id">
              <td class="mono">{{ user.uid || user.id }}</td>
              <td>{{ user.email }}</td>
              <td>{{ user.nickname }}</td>
              <td><span :class="['badge', user.status === 0 ? 'badge-success' : 'badge-danger']">{{ user.status === 0 ? t('active') : t('disabled') }}</span></td>
              <td class="mono">{{ formatTime(user.created_at) }}</td>
              <td>
                <button v-if="user.status === 0" class="btn btn-danger btn-sm" @click="disableUser(user.id)">{{ t('disable') }}</button>
                <button v-else class="btn btn-warn btn-sm" @click="enableUser(user.id)">{{ t('enable') }}</button>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </ShellLayout>
</template>

<script setup>
import { onMounted, ref } from 'vue'
import { getAuthToken } from '../auth.js'
import ShellLayout from '../components/ShellLayout.vue'
import { useI18n } from '../i18n.js'

const { t } = useI18n()
const users = ref([])
const loading = ref(true)
const formatTime = (value) => value ? new Date(value).toLocaleString() : '--'

const fetchUsers = async () => {
  const token = getAuthToken()
  try {
    const res = await fetch('/api/admin/users', { headers: { Authorization: `Bearer ${token}` } })
    const data = await res.json()
    if (data.code === 0) users.value = data.data?.items || []
  } catch (e) {
    console.error(e)
  } finally {
    loading.value = false
  }
}

const updateStatus = async (id, action) => {
  const token = getAuthToken()
  const res = await fetch(`/api/admin/users/${id}/${action}`, { method: 'PUT', headers: { Authorization: `Bearer ${token}` } })
  const data = await res.json()
  if (data.code === 0) await fetchUsers()
}
const disableUser = (id) => updateStatus(id, 'disable')
const enableUser = (id) => updateStatus(id, 'enable')

onMounted(fetchUsers)
</script>
