<template>
  <ShellLayout :title="t('apiKeys')" :subtitle="t('apiKeysSubtitle')">
    <div class="panel">
      <div class="panel-header"><h3>{{ t('createKey') }}</h3></div>
      <div class="form-grid">
        <div class="form-group">
          <label>{{ t('keyName') }}</label>
          <input v-model="newKeyName" :placeholder="t('productionApiPlaceholder')" />
        </div>
        <div class="form-group">
          <label>{{ t('rateLimit') }}</label>
          <input v-model.number="rateLimit" type="number" min="1" max="10000" />
        </div>
        <div class="form-group">
          <label>{{ projectLabels.project }}</label>
          <select v-model="selectedProjectUid" required>
            <option v-for="project in projects" :key="project.uid" :value="project.uid">{{ project.name }}</option>
          </select>
        </div>
        <div class="form-group">
          <label>{{ t('dailySpendLimit') }}</label>
          <input v-model.number="dailySpendLimit" type="number" min="0" step="0.0001" placeholder="0.5" />
        </div>
        <div class="form-group">
          <label>{{ t('monthlySpendLimit') }}</label>
          <input v-model.number="monthlySpendLimit" type="number" min="0" step="0.0001" placeholder="10" />
        </div>
        <div class="form-group">
          <label>{{ t('totalSpendLimit') }}</label>
          <input v-model.number="totalSpendLimit" type="number" min="0" step="0.0001" placeholder="100" />
        </div>
        <div class="form-group">
          <label>{{ t('keyExpiresAt') }}</label>
          <input v-model="expiresAt" type="datetime-local" />
        </div>
        <div class="form-group">
          <label>{{ t('ipAllowlist') }}</label>
          <input v-model="ipAllowlist" :placeholder="t('ipAllowlistPlaceholder')" />
        </div>
        <div class="form-group form-wide">
          <label>{{ t('allowedModels') }}</label>
          <div class="model-check-grid">
            <label v-for="model in modelCatalog" :key="model.model_id" class="model-check">
              <input v-model="selectedModels" type="checkbox" :value="model.model_id" />
              <span class="mono">{{ model.model_id }}</span>
            </label>
          </div>
        </div>
      </div>
      <div style="margin-top:16px;">
        <button class="btn btn-primary" @click="createKey">{{ t('createKey') }}</button>
      </div>
    </div>

    <div class="panel">
      <div class="panel-header"><h3>{{ t('apiKeys') }}</h3><span class="badge badge-accent">{{ keys.length }}</span></div>
      <div v-if="loading" class="empty-state">{{ t('loading') }}</div>
      <div v-else class="table-wrap">
        <table class="data-table">
          <thead><tr><th>{{ t('keyName') }}</th><th>{{ projectLabels.project }}</th><th>{{ t('keyPrefix') }}</th><th>{{ t('allowedModels') }}</th><th>{{ t('rateLimit') }}</th><th>{{ t('dailySpendLimit') }}</th><th>{{ t('monthlySpendLimit') }}</th><th>{{ t('totalSpendLimit') }}</th><th>{{ t('keyExpiresAt') }}</th><th>{{ t('ipAllowlist') }}</th><th>{{ t('lastUsed') }}</th><th>{{ t('status') }}</th><th>{{ t('actions') }}</th></tr></thead>
          <tbody>
            <tr v-for="key in keys" :key="key.uid">
              <td>{{ key.name }}</td>
              <td>{{ key.project_name || '--' }}</td>
              <td class="mono">{{ key.key_prefix }}</td>
              <td class="mono model-list">{{ formatModels(key.models_allowed) }}</td>
              <td>{{ key.rate_limit }}/h</td>
              <td>{{ formatSpendLimit(key.daily_spend_limit) }}</td>
              <td>{{ formatSpendLimit(key.monthly_spend_limit, 'month') }}</td>
              <td>{{ formatSpendLimit(key.total_spend_limit, 'total') }}</td>
              <td>{{ formatTime(key.expires_at) }}</td>
              <td class="mono model-list">{{ formatIpAllowlist(key.ip_allowlist) }}</td>
              <td>{{ formatTime(key.last_used_at) }}</td>
              <td><span :class="['badge', key.status === 0 ? 'badge-success' : 'badge-danger']">{{ key.status === 0 ? t('active') : t('disabled') }}</span></td>
              <td>
                <div class="table-actions">
                  <button v-if="key.status === 0" class="btn btn-danger btn-sm" @click="setKeyStatus(key, 'disable')">{{ t('disable') }}</button>
                  <button v-else class="btn btn-warn btn-sm" @click="setKeyStatus(key, 'enable')">{{ t('enable') }}</button>
                  <button class="btn btn-ghost btn-sm" @click="deleteKey(key)">{{ t('delete') }}</button>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>

    <div v-if="createdKey" class="modal-backdrop" role="dialog" aria-modal="true">
      <div class="modal-card">
        <h3>{{ t('createdKeyTitle') }}</h3>
        <p>{{ t('createdKeyDesc') }}</p>
        <div class="secret-box mono">{{ createdKey }}</div>
        <div class="modal-actions">
          <button class="btn btn-ghost" @click="copyCreatedKey">{{ t('copy') }}</button>
          <button class="btn btn-primary" @click="createdKey = ''">{{ t('close') }}</button>
        </div>
      </div>
    </div>

    <div v-if="deleteTarget" class="modal-backdrop" role="dialog" aria-modal="true">
      <div class="modal-card">
        <h3>{{ t('confirmDeleteKey') }}</h3>
        <p>{{ deleteTarget.name || deleteTarget.key_prefix }}</p>
        <div class="modal-actions">
          <button class="btn btn-ghost" @click="deleteTarget = null">{{ t('cancel') }}</button>
          <button class="btn btn-danger" @click="confirmDeleteKey">{{ t('delete') }}</button>
        </div>
      </div>
    </div>
  </ShellLayout>
</template>

<script setup>
import { computed, onMounted, ref, watch } from 'vue'
import { authHeaders, getSelectedWorkspace, setSelectedWorkspace } from '../auth.js'
import ShellLayout from '../components/ShellLayout.vue'
import { useI18n } from '../i18n.js'
import { notifyError, notifySuccess } from '../notify.js'

const { t, isZh } = useI18n()
const projectLabels = computed(() => isZh.value ? { project: '项目' } : { project: 'Project' })
const defaultKeyName = computed(() => t('productionApiPlaceholder'))
const keys = ref([])
const loading = ref(true)
const newKeyName = ref(defaultKeyName.value)
const rateLimit = ref(500)
const dailySpendLimit = ref(null)
const monthlySpendLimit = ref(null)
const totalSpendLimit = ref(null)
const expiresAt = ref('')
const ipAllowlist = ref('')
const modelCatalog = ref([])
const selectedModels = ref([])
const projects = ref([])
const selectedProjectUid = ref('')
const createdKey = ref('')
const deleteTarget = ref(null)

const fetchModels = async () => {
  const res = await fetch('/api/models')
  const data = await res.json()
  if (data.code === 0) {
    modelCatalog.value = data.data?.items || []
    if (!selectedModels.value.length && modelCatalog.value.length) {
      selectedModels.value = [modelCatalog.value[0].model_id]
    }
  }
}

const fetchKeys = async () => {
  try {
    const res = await fetch('/api/keys', { headers: authHeaders() })
    const data = await res.json()
    if (data.code === 0) keys.value = data.data?.items || []
  } catch (e) {
    console.error(e)
  } finally {
    loading.value = false
  }
}

const fetchProjects = async () => {
  const workspaceData = await fetch('/api/workspaces', { headers: authHeaders() }).then((res) => res.json())
  if (workspaceData.code !== 0) throw new Error(workspaceData.message || 'Failed to load workspaces')
  const workspace = (workspaceData.data?.items || []).find((item) => item.selected)
  if (!workspace?.uid) throw new Error('No workspace selected')
  if (getSelectedWorkspace() !== workspace.uid) setSelectedWorkspace(workspace.uid)
  const projectData = await fetch(`/api/workspaces/${encodeURIComponent(workspace.uid)}/projects`, { headers: authHeaders() }).then((res) => res.json())
  if (projectData.code !== 0) throw new Error(projectData.message || 'Failed to load projects')
  projects.value = projectData.data?.items || []
  if (!projects.value.some((project) => project.uid === selectedProjectUid.value)) {
    selectedProjectUid.value = projects.value.find((project) => project.is_default)?.uid || projects.value[0]?.uid || ''
  }
}

const createKey = async () => {
  const cleanName = String(newKeyName.value || '').trim()
  if (!cleanName) {
    notifyError(t('keyNameRequired'))
    return
  }
  if (cleanName.length > 50) {
    notifyError(t('keyNameMax'))
    return
  }
  if (!Number.isInteger(Number(rateLimit.value)) || Number(rateLimit.value) < 1 || Number(rateLimit.value) > 10000) {
    notifyError(t('invalidRateLimit'))
    return
  }
  if (!selectedModels.value.length) {
    notifyError(t('selectAtLeastOneModel'))
    return
  }
  try {
    const res = await fetch('/api/keys', {
      method: 'POST',
      headers: authHeaders({ 'Content-Type': 'application/json' }),
      body: JSON.stringify({
        name: cleanName,
        rate_limit: Number(rateLimit.value || 500),
        daily_spend_limit: dailySpendLimit.value ? Number(dailySpendLimit.value) : undefined,
        monthly_spend_limit: monthlySpendLimit.value ? Number(monthlySpendLimit.value) : undefined,
        total_spend_limit: totalSpendLimit.value ? Number(totalSpendLimit.value) : undefined,
        expires_at: expiresAt.value ? new Date(expiresAt.value).toISOString() : undefined,
        ip_allowlist: String(ipAllowlist.value || '').split(/[\s,]+/).filter(Boolean),
        models: selectedModels.value,
        project_uid: selectedProjectUid.value,
      }),
    })
    const data = await res.json()
    if (data.code === 0) {
      createdKey.value = data.data?.key || ''
      notifySuccess(t('saveKeyNow'))
      await fetchKeys()
    } else {
      notifyError(data.message)
    }
  } catch (e) {
    notifyError(e.message)
  }
}

const setKeyStatus = async (key, action) => {
  const res = await fetch(`/api/keys/${key.uid}/${action}`, {
    method: 'PUT',
    headers: authHeaders(),
  })
  const data = await res.json()
  if (data.code !== 0) {
    notifyError(data.message)
    return
  }
  await fetchKeys()
}

const deleteKey = async (key) => {
  deleteTarget.value = key
}

const confirmDeleteKey = async () => {
  if (!deleteTarget.value) return
  const target = deleteTarget.value
  const res = await fetch(`/api/keys/${target.uid}`, {
    method: 'DELETE',
    headers: authHeaders(),
  })
  const data = await res.json()
  if (data.code !== 0) {
    notifyError(data.message)
    return
  }
  deleteTarget.value = null
  await fetchKeys()
}

const copyCreatedKey = async () => {
  try {
    await navigator.clipboard.writeText(createdKey.value)
    notifySuccess(t('copied'))
  } catch (e) {
    notifyError(e.message)
  }
}

const formatModels = (models) => {
  return Array.isArray(models) && models.length ? models.join(', ') : t('allModels')
}

const formatSpendLimit = (value, period = 'day') => {
  if (!value) return t('noLimit')
  const suffix = period === 'total' ? '' : ` / ${t(period === 'month' ? 'perMonth' : 'perDay')}`
  return `$${Number(value).toFixed(4)}${suffix}`
}

const formatIpAllowlist = (value) => Array.isArray(value) && value.length ? value.join(', ') : t('noLimit')

const formatTime = (value) => {
  return value ? new Date(value).toLocaleString() : t('never')
}

watch(defaultKeyName, (nextName, previousName) => {
  if (newKeyName.value === previousName) {
    newKeyName.value = nextName
  }
})

onMounted(async () => {
  await fetchModels()
  await fetchProjects()
  await fetchKeys()
})
</script>
