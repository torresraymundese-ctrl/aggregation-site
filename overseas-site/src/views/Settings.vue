<template>
  <ShellLayout :title="t('settings')" :subtitle="t('settingsSubtitle')">
    <div class="dash-grid">
      <div class="panel">
        <div class="panel-header"><h3>{{ t('profile') }}</h3></div>
        <div v-if="profileLoading" class="empty-state">{{ t('loading') }}</div>
        <div v-else-if="profileError" class="empty-state danger">{{ profileError }}</div>
        <div v-else class="form-grid">
          <div class="form-group"><label>{{ labels.userId }}</label><input :value="profile.uid || '--'" readonly /></div>
          <div class="form-group"><label>{{ t('email') }}</label><input :value="profile.email || '--'" readonly /></div>
          <div class="form-group"><label>{{ labels.role }}</label><input :value="profile.is_admin ? labels.admin : labels.member" readonly /></div>
        </div>
      </div>
      <div class="panel">
        <div class="panel-header"><h3>{{ t('security') }}</h3></div>
        <p class="muted">{{ t('securityDesc') }}</p>
      </div>

      <div class="panel dash-full byok-panel">
        <div class="panel-header">
          <div>
            <h3>{{ labels.byokTitle }}</h3>
            <p class="muted">{{ labels.byokDesc }}</p>
          </div>
          <span class="badge badge-accent">BYOK</span>
        </div>

        <div v-if="credentialError" class="empty-state danger byok-error">{{ credentialError }}</div>
        <div v-else-if="!credentialsLoading && !providerOptions.length" class="empty-state byok-error">{{ labels.noProviders }}</div>

        <form class="byok-form" @submit.prevent="saveCredential">
          <div class="form-group">
            <label for="byok-provider">{{ labels.provider }}</label>
            <select id="byok-provider" v-model="credentialForm.providerId" required :disabled="credentialsLoading || saving">
              <option value="" disabled>{{ labels.selectProvider }}</option>
              <option v-for="provider in providerOptions" :key="String(provider.value)" :value="provider.value">
                {{ provider.name }}
              </option>
            </select>
          </div>
          <div class="form-group">
            <label for="byok-name">{{ labels.credentialName }}</label>
            <input id="byok-name" v-model.trim="credentialForm.name" maxlength="100" required :placeholder="labels.namePlaceholder" />
          </div>
          <div class="form-group">
            <label for="byok-key">{{ labels.providerKey }}</label>
            <input
              id="byok-key"
              v-model="credentialForm.apiKey"
              type="password"
              autocomplete="off"
              spellcheck="false"
              required
              :placeholder="labels.keyPlaceholder"
            />
            <small>{{ labels.keyPrivacy }}</small>
          </div>
          <button class="btn btn-primary" type="submit" :disabled="saving || credentialsLoading || !providerOptions.length">
            {{ saving ? labels.saving : labels.saveCredential }}
          </button>
        </form>

        <div class="credential-list-head">
          <h4>{{ labels.configuredCredentials }}</h4>
          <button class="btn btn-ghost btn-sm" type="button" :disabled="credentialsLoading" @click="loadCredentials">
            {{ labels.refresh }}
          </button>
        </div>
        <div v-if="credentialsLoading" class="empty-state">{{ t('loading') }}</div>
        <div v-else-if="!credentials.length" class="empty-state">{{ labels.noCredentials }}</div>
        <div v-else class="table-wrap">
          <table class="data-table">
            <thead>
              <tr>
                <th>{{ labels.provider }}</th>
                <th>{{ labels.prefix }}</th>
                <th>{{ t('status') }}</th>
                <th>{{ labels.time }}</th>
                <th>{{ t('actions') }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="credential in credentials" :key="credential.uid">
                <td>{{ credentialProviderName(credential) }}</td>
                <td class="mono">{{ credential.key_prefix || credential.prefix || '--' }}</td>
                <td><span :class="['badge', credentialStatusClass(credential.status)]">{{ credentialStatusText(credential.status) }}</span></td>
                <td class="mono">{{ formatTime(credential.updated_at || credential.created_at || credential.last_used_at) }}</td>
                <td><button class="btn btn-danger btn-sm" type="button" @click="deleteCandidate = credential">{{ labels.delete }}</button></td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>
    </div>

    <div v-if="deleteCandidate" class="modal-backdrop" role="presentation" @click.self="deleteCandidate = null">
      <section class="modal-card" role="dialog" aria-modal="true" :aria-label="labels.deleteTitle">
        <h3>{{ labels.deleteTitle }}</h3>
        <p>{{ labels.deleteDesc.replace('{name}', deleteCandidate.name || credentialProviderName(deleteCandidate)) }}</p>
        <div class="modal-actions">
          <button class="btn btn-ghost" type="button" :disabled="deleting" @click="deleteCandidate = null">{{ labels.cancel }}</button>
          <button class="btn btn-danger" type="button" :disabled="deleting" @click="deleteCredential">
            {{ deleting ? labels.deleting : labels.confirmDelete }}
          </button>
        </div>
      </section>
    </div>
  </ShellLayout>
</template>

<script setup>
import { computed, onBeforeUnmount, onMounted, reactive, ref } from 'vue'
import { apiErrorMessage, requestJson } from '../api.js'
import { authHeaders } from '../auth.js'
import ShellLayout from '../components/ShellLayout.vue'
import { useI18n } from '../i18n.js'
import { notifyError, notifySuccess } from '../notify.js'

const { t, isZh } = useI18n()
const profile = ref({ uid: '', email: '', is_admin: false })
const profileLoading = ref(true)
const profileError = ref('')
const providers = ref([])
const credentials = ref([])
const credentialsLoading = ref(true)
const credentialError = ref('')
const saving = ref(false)
const deleting = ref(false)
const deleteCandidate = ref(null)
const credentialForm = reactive({ providerId: '', name: '', apiKey: '' })

const labels = computed(() => isZh.value ? {
  userId: '用户 ID',
  role: '账号角色',
  admin: '管理员',
  member: '普通用户',
  loadFailed: '账号资料加载失败。',
  byokTitle: '供应商密钥（BYOK）',
  byokDesc: '保存自己的上游供应商 Key。这里只显示前缀和状态，不会再次展示明文。',
  provider: '供应商',
  selectProvider: '选择供应商',
  credentialName: '凭据名称',
  namePlaceholder: '例如：生产环境 OpenAI',
  providerKey: '供应商 API Key',
  keyPlaceholder: '仅本次输入',
  keyPrivacy: '明文只存在于当前输入框内存中，保存成功后立即清空。',
  saveCredential: '加密保存凭据',
  saving: '保存中',
  configuredCredentials: '已配置凭据',
  refresh: '刷新',
  noCredentials: '尚未配置供应商凭据。',
  noProviders: '当前没有可配置的 BYOK 供应商。',
  prefix: 'Key 前缀',
  time: '更新时间',
  active: '可用',
  disabled: '停用',
  unknown: '未知',
  delete: '删除',
  deleteTitle: '确认删除供应商凭据',
  deleteDesc: '删除“{name}”后，它将不能再用于新的模型请求。',
  cancel: '取消',
  confirmDelete: '确认删除',
  deleting: '删除中',
  credentialsLoadFailed: '供应商凭据加载失败。',
  saveFailed: '供应商凭据保存失败。',
  saved: '供应商凭据已保存。',
  deleteFailed: '供应商凭据删除失败。',
  deleted: '供应商凭据已删除。',
  fieldsRequired: '请选择供应商，并填写凭据名称和 API Key。',
} : {
  userId: 'User ID',
  role: 'Account role',
  admin: 'Administrator',
  member: 'Member',
  loadFailed: 'Failed to load account profile.',
  byokTitle: 'Provider credentials (BYOK)',
  byokDesc: 'Save your own upstream provider key. Only its prefix and status are shown; plaintext is never displayed again.',
  provider: 'Provider',
  selectProvider: 'Select a provider',
  credentialName: 'Credential name',
  namePlaceholder: 'Example: Production OpenAI',
  providerKey: 'Provider API key',
  keyPlaceholder: 'Enter once',
  keyPrivacy: 'Plaintext exists only in this input field memory and is cleared immediately after a successful save.',
  saveCredential: 'Encrypt and save',
  saving: 'Saving',
  configuredCredentials: 'Configured credentials',
  refresh: 'Refresh',
  noCredentials: 'No provider credentials configured.',
  noProviders: 'No BYOK providers are currently available.',
  prefix: 'Key prefix',
  time: 'Updated',
  active: 'Active',
  disabled: 'Disabled',
  unknown: 'Unknown',
  delete: 'Delete',
  deleteTitle: 'Confirm credential deletion',
  deleteDesc: 'After deleting “{name}”, it cannot be used for new model requests.',
  cancel: 'Cancel',
  confirmDelete: 'Delete credential',
  deleting: 'Deleting',
  credentialsLoadFailed: 'Failed to load provider credentials.',
  saveFailed: 'Failed to save provider credential.',
  saved: 'Provider credential saved.',
  deleteFailed: 'Failed to delete provider credential.',
  deleted: 'Provider credential deleted.',
  fieldsRequired: 'Select a provider and enter a credential name and API key.',
})

const providerOptions = computed(() => providers.value.map((provider) => ({
  value: provider.id ?? provider.provider_id,
  code: provider.provider_id ?? provider.id,
  name: provider.display_name || provider.name || String(provider.provider_id ?? provider.id),
})).filter((provider) => provider.value !== undefined && provider.value !== null))

const loadProfile = async () => {
  profileLoading.value = true
  profileError.value = ''
  try {
    const data = await requestJson('/api/auth/me', { headers: authHeaders() })
    if (data.code !== 0) throw new Error(data.message || labels.value.loadFailed)
    profile.value = { ...profile.value, ...(data.data || {}) }
  } catch (e) {
    profileError.value = apiErrorMessage(e, e?.message || labels.value.loadFailed)
  } finally {
    profileLoading.value = false
  }
}

const loadCredentials = async () => {
  credentialsLoading.value = true
  credentialError.value = ''
  try {
    const data = await requestJson('/api/provider-credentials', { headers: authHeaders() })
    if (data.code !== 0) throw new Error(data.message || labels.value.credentialsLoadFailed)
    const payload = data.data || data
    credentials.value = Array.isArray(payload.credentials)
      ? payload.credentials
      : Array.isArray(payload.items)
        ? payload.items
        : []
    providers.value = Array.isArray(payload.providers)
      ? payload.providers
      : credentials.value.map((credential) => ({
          id: credential.provider_id,
          name: credential.provider_name,
        }))
  } catch (e) {
    credentialError.value = apiErrorMessage(e, e?.message || labels.value.credentialsLoadFailed)
  } finally {
    credentialsLoading.value = false
  }
}

const saveCredential = async () => {
  credentialError.value = ''
  if (credentialForm.providerId === '' || !credentialForm.name || !credentialForm.apiKey) {
    notifyError(labels.value.fieldsRequired)
    return
  }
  saving.value = true
  try {
    const data = await requestJson('/api/provider-credentials', {
      method: 'POST',
      headers: { ...authHeaders(), 'Content-Type': 'application/json' },
      body: JSON.stringify({
        provider_id: credentialForm.providerId,
        name: credentialForm.name,
        api_key: credentialForm.apiKey,
      }),
    })
    if (data.code !== 0) throw new Error(data.message || labels.value.saveFailed)
    credentialForm.apiKey = ''
    credentialForm.name = ''
    credentialForm.providerId = ''
    notifySuccess(data.message || labels.value.saved)
    await loadCredentials()
  } catch (e) {
    const message = apiErrorMessage(e, e?.message || labels.value.saveFailed)
    credentialError.value = message
    notifyError(message)
  } finally {
    saving.value = false
  }
}

const deleteCredential = async () => {
  const uid = deleteCandidate.value?.uid
  if (!uid) return
  deleting.value = true
  credentialError.value = ''
  try {
    const data = await requestJson(`/api/provider-credentials/${encodeURIComponent(uid)}`, {
      method: 'DELETE',
      headers: authHeaders(),
    })
    if (data.code !== 0) throw new Error(data.message || labels.value.deleteFailed)
    deleteCandidate.value = null
    notifySuccess(data.message || labels.value.deleted)
    await loadCredentials()
  } catch (e) {
    const message = apiErrorMessage(e, e?.message || labels.value.deleteFailed)
    credentialError.value = message
    notifyError(message)
  } finally {
    deleting.value = false
  }
}

const credentialProviderName = (credential) => {
  if (credential.provider_name) return credential.provider_name
  const match = providers.value.find((provider) => String(provider.id ?? provider.provider_id) === String(credential.provider_id))
  return match?.display_name || match?.name || credential.provider_id || '--'
}
const credentialStatusText = (status) => status === 0 || status === '0' || status === 'active'
  ? labels.value.active
  : status === 1 || status === '1' || status === 'disabled'
    ? labels.value.disabled
    : labels.value.unknown
const credentialStatusClass = (status) => status === 0 || status === '0' || status === 'active' ? 'badge-success' : 'badge-warn'
const formatTime = (value) => value ? new Date(value).toLocaleString() : '--'

onMounted(() => {
  loadProfile()
  loadCredentials()
})

onBeforeUnmount(() => {
  credentialForm.apiKey = ''
})
</script>

<style scoped>
.byok-panel { margin-top: 0; }
.byok-panel .panel-header p { margin-top: 6px; }
.byok-error { margin-bottom: 14px; }
.byok-form { display: grid; grid-template-columns: 1fr 1.2fr 1.6fr auto; gap: 12px; align-items: end; padding: 16px; border: 1px solid var(--border); border-radius: var(--radius); background: #111; }
.byok-form select { width: 100%; }
.byok-form small { display: block; margin-top: 6px; color: var(--muted); }
.byok-form .btn { min-height: 42px; }
.credential-list-head { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin: 24px 0 12px; }
button:disabled { opacity: .58; cursor: wait; }
@media (max-width: 1040px) {
  .byok-form { grid-template-columns: repeat(2, minmax(0, 1fr)); }
}
@media (max-width: 720px) {
  .byok-form { grid-template-columns: 1fr; }
}
</style>
