<template>
  <div v-if="guest" class="guest-shell">
    <nav class="landing-nav">
      <div class="landing-nav-inner">
        <router-link to="/" class="sidebar-logo"><span class="dot"></span>{{ t('brand') }}</router-link>
        <div class="landing-links">
          <router-link to="/models">{{ t('models') }}</router-link>
          <router-link to="/docs">{{ t('docs') }}</router-link>
          <button class="lang-toggle" @click="toggleLang">{{ t('language') }}</button>
          <router-link to="/login" class="btn btn-ghost">{{ t('login') }}</router-link>
          <router-link to="/login" class="btn btn-primary">{{ t('startNow') }}</router-link>
        </div>
      </div>
    </nav>
    <main class="main guest-main">
      <div class="topbar">
        <div>
          <h1>{{ title }}</h1>
          <p v-if="subtitle">{{ subtitle }}</p>
        </div>
      </div>
      <slot />
      <SiteFooter compact />
    </main>
  </div>

  <div v-else class="app-shell" :class="{ 'admin-mode': admin }">
    <aside class="sidebar">
      <router-link :to="admin ? '/admin' : '/dashboard'" class="sidebar-logo">
        <span class="dot"></span>{{ admin ? t('adminBrand') : t('brand') }}
      </router-link>
      <span v-if="admin" class="admin-badge">{{ t('adminPanel') }}</span>

      <template v-for="group in navGroups" :key="group.title">
        <div class="nav-section">{{ group.title }}</div>
        <router-link v-for="item in group.items" :key="item.to" :to="item.to" class="nav-item">
          <span class="nav-icon">{{ item.icon }}</span>{{ item.label }}
        </router-link>
      </template>

      <div v-if="admin || canSeeAdmin" style="margin-top:auto;padding-top:16px;border-top:1px solid var(--border);">
        <router-link :to="admin ? '/dashboard' : '/admin'" class="nav-item">
          <span class="nav-icon">{{ admin ? '<' : 'AD' }}</span>{{ admin ? t('backConsole') : t('adminPanel') }}
        </router-link>
      </div>
      <div v-else style="margin-top:auto;"></div>
      <button class="nav-item nav-button" type="button" @click="logout">
        <span class="nav-icon">LO</span>{{ t('logout') }}
      </button>
    </aside>

    <main class="main">
      <div class="topbar">
        <div>
          <h1>{{ title }}</h1>
          <p v-if="subtitle">{{ subtitle }}</p>
        </div>
        <div class="topbar-right">
          <select
            v-if="!admin && workspaces.length"
            v-model="selectedWorkspace"
            class="workspace-switcher"
            :aria-label="t('team')"
            @change="switchWorkspace"
          >
            <option v-for="workspace in workspaces" :key="workspace.uid" :value="workspace.uid">
              {{ workspace.name }}
            </option>
          </select>
          <div v-if="!admin" class="balance-pill">{{ t('balanceLabel') }} <span class="amount">{{ balanceText }}</span></div>
          <span v-else class="admin-badge">{{ t('navAdmin') }}</span>
          <button class="lang-toggle" @click="toggleLang">{{ t('language') }}</button>
          <div class="avatar-sm">{{ admin ? 'AD' : 'NX' }}</div>
        </div>
      </div>
      <slot />
      <SiteFooter compact />
    </main>
  </div>
</template>

<script setup>
import { computed, onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { authHeaders, clearAuthToken, getAuthToken, getSelectedWorkspace, setSelectedWorkspace } from '../auth.js'
import { requestJson } from '../api.js'
import { useI18n } from '../i18n.js'
import SiteFooter from './SiteFooter.vue'
import { notifyError } from '../notify.js'

const props = defineProps({
  title: { type: String, required: true },
  subtitle: { type: String, default: '' },
  balanceText: { type: String, default: '$0.00' },
  admin: { type: Boolean, default: false },
  guest: { type: Boolean, default: false },
})

const { t, toggleLang } = useI18n()
const router = useRouter()
const canSeeAdmin = ref(false)
const workspaces = ref([])
const selectedWorkspace = ref(getSelectedWorkspace())

onMounted(async () => {
  if (props.admin) {
    canSeeAdmin.value = true
    return
  }

  try {
    const token = getAuthToken()
    if (!token) return

    const res = await fetch('/api/auth/me', { headers: authHeaders() })
    const data = await res.json()
    canSeeAdmin.value = data.code === 0 && data.data?.is_admin === true

    const workspaceData = await requestJson('/api/workspaces', { headers: authHeaders() })
    workspaces.value = workspaceData.data?.items || []
    const current = workspaces.value.find((workspace) => workspace.selected)
    if (!selectedWorkspace.value && current?.uid) {
      selectedWorkspace.value = current.uid
      setSelectedWorkspace(current.uid)
    }
  } catch {
    canSeeAdmin.value = false
  }
})

const switchWorkspace = () => {
  setSelectedWorkspace(selectedWorkspace.value)
  window.location.reload()
}

const logout = async () => {
  try {
    await requestJson('/api/auth/logout', { method: 'POST', headers: authHeaders() })
    clearAuthToken()
    await router.push('/login')
  } catch (error) {
    if (error?.status === 401 || error?.status === 403) {
      clearAuthToken()
      await router.push('/login')
      return
    }
    notifyError(error?.message || 'Logout failed')
  }
}

const navGroups = computed(() => {
  if (props.admin) {
    return [
      { title: t('navAdmin'), items: [
        { to: '/admin', label: t('dashboard'), icon: 'DB' },
        { to: '/admin/users', label: t('users'), icon: 'US' },
        { to: '/admin/orders', label: t('orders'), icon: 'OR' },
        { to: '/admin/logs', label: t('logs'), icon: 'LG' },
        { to: '/admin/models', label: t('modelPricing'), icon: 'MP' },
        { to: '/admin/providers', label: t('providers'), icon: 'PV' },
        { to: '/admin/reports', label: t('reports'), icon: 'RP' },
        { to: '/admin/risk', label: t('riskControl'), icon: 'RK' },
        { to: '/admin/audit-logs', label: t('auditLogs'), icon: 'AL' },
      ] },
    ]
  }

  return [
    { title: t('navMain'), items: [
      { to: '/dashboard', label: t('dashboard'), icon: 'DB' },
      { to: '/keys', label: t('apiKeys'), icon: 'KY' },
      { to: '/models', label: t('models'), icon: 'MD' },
      { to: '/purchase', label: t('purchase'), icon: 'TK' },
    ] },
    { title: t('navMonitor'), items: [
      { to: '/logs', label: t('logs'), icon: 'LG' },
      { to: '/balance', label: t('balance'), icon: '$' },
      { to: '/orders', label: t('orders'), icon: 'OR' },
    ] },
    { title: t('navResources'), items: [
      { to: '/docs', label: t('docs'), icon: 'DC' },
      { to: '/team', label: t('team'), icon: 'TM' },
      { to: '/settings', label: t('settings'), icon: 'ST' },
    ] },
  ]
})
</script>

<style scoped>
.guest-shell {
  min-height: 100vh;
}

.guest-main {
  width: min(1400px, 100%);
  margin: 0 auto;
  padding: 32px clamp(20px, 4vw, 64px);
}
.workspace-switcher {
  min-width: 150px;
  max-width: 240px;
}
</style>
