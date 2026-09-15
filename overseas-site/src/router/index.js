import { createRouter, createWebHistory } from 'vue-router'
import Home from '../views/Home.vue'
import { requestJson } from '../api.js'
import { authHeaders, clearAuthToken, clearSelectedWorkspace, getAuthToken, getSelectedWorkspace } from '../auth.js'

const routes = [
  { path: '/', name: 'Home', component: Home, meta: { title: 'Nexus Gateway - AI Model Aggregation Platform' } },
  { path: '/login', name: 'Login', component: () => import('../views/Login.vue'), meta: { title: 'Sign in | Nexus Gateway' } },
  { path: '/register', redirect: '/login' },
  { path: '/dashboard', name: 'Dashboard', component: () => import('../views/Dashboard.vue'), meta: { title: 'Dashboard | Nexus Gateway', requiresAuth: true } },
  { path: '/keys', name: 'Keys', component: () => import('../views/Keys.vue'), meta: { title: 'API Keys | Nexus Gateway', requiresAuth: true } },
  { path: '/models', name: 'Models', component: () => import('../views/Models.vue'), meta: { title: 'Model Marketplace | Nexus Gateway' } },
  { path: '/purchase', name: 'Purchase', component: () => import('../views/Purchase.vue'), meta: { title: 'Add Balance | Nexus Gateway', requiresAuth: true } },
  { path: '/balance', name: 'Balance', component: () => import('../views/Balance.vue'), meta: { title: 'Balance | Nexus Gateway', requiresAuth: true } },
  { path: '/orders', name: 'Orders', component: () => import('../views/Orders.vue'), meta: { title: 'Orders | Nexus Gateway', requiresAuth: true } },
  { path: '/logs', name: 'Logs', component: () => import('../views/Logs.vue'), meta: { title: 'Call Logs | Nexus Gateway', requiresAuth: true } },
  { path: '/docs', name: 'Docs', component: () => import('../views/Docs.vue'), meta: { title: 'API Docs | Nexus Gateway' } },
  { path: '/settings', name: 'Settings', component: () => import('../views/Settings.vue'), meta: { title: 'Settings | Nexus Gateway', requiresAuth: true } },
  { path: '/team', name: 'Team', component: () => import('../views/Team.vue'), meta: { title: 'Team | Nexus Gateway', requiresAuth: true } },
  { path: '/privacy', name: 'Privacy', component: () => import('../views/Privacy.vue'), meta: { title: 'Privacy Policy | Nexus Gateway' } },
  { path: '/terms', name: 'Terms', component: () => import('../views/Terms.vue'), meta: { title: 'Terms of Service | Nexus Gateway' } },
  { path: '/admin', name: 'AdminDashboard', component: () => import('../views/AdminDashboard.vue'), meta: { title: 'Admin | Nexus Gateway', requiresAdmin: true } },
  { path: '/admin/users', name: 'AdminUsers', component: () => import('../views/AdminUsers.vue'), meta: { title: 'Admin Users | Nexus Gateway', requiresAdmin: true } },
  { path: '/admin/orders', name: 'AdminOrders', component: () => import('../views/AdminOrders.vue'), meta: { title: 'Admin Orders | Nexus Gateway', requiresAdmin: true } },
  { path: '/admin/logs', name: 'AdminLogs', component: () => import('../views/Logs.vue'), props: { admin: true }, meta: { title: 'Admin Logs | Nexus Gateway', requiresAdmin: true } },
  { path: '/admin/models', name: 'AdminModels', component: () => import('../views/AdminModels.vue'), meta: { title: 'Admin Model Pricing | Nexus Gateway', requiresAdmin: true } },
  { path: '/admin/providers', name: 'AdminProviders', component: () => import('../views/AdminProviders.vue'), meta: { title: 'Admin Providers | Nexus Gateway', requiresAdmin: true } },
  { path: '/admin/reports', name: 'AdminReports', component: () => import('../views/AdminReports.vue'), meta: { title: 'Admin Reports | Nexus Gateway', requiresAdmin: true } },
  { path: '/admin/risk', name: 'AdminRisk', component: () => import('../views/AdminRisk.vue'), meta: { title: 'Admin Risk Control | Nexus Gateway', requiresAdmin: true } },
  { path: '/admin/audit-logs', name: 'AdminAuditLogs', component: () => import('../views/AdminAuditLogs.vue'), meta: { title: 'Admin Audit Logs | Nexus Gateway', requiresAdmin: true } },
]

const router = createRouter({
  history: createWebHistory(),
  routes,
})

router.afterEach((to) => {
  document.title = to.meta?.title || 'Nexus Gateway'
})

router.beforeEach(async (to) => {
  if (!to.meta?.requiresAuth && !to.meta?.requiresAdmin) return true

  const token = getAuthToken()
  if (!token) return { path: '/login', query: { redirect: to.fullPath } }

  try {
    let data
    try {
      data = await requestJson('/api/auth/me', { headers: authHeaders() })
    } catch (error) {
      if ((error?.status === 401 || error?.status === 403) && getSelectedWorkspace()) {
        clearSelectedWorkspace()
        data = await requestJson('/api/auth/me', { headers: authHeaders() })
      } else {
        throw error
      }
    }
    if (data.code !== 0 || !data.data?.uid) {
      clearAuthToken()
      return { path: '/login', query: { redirect: to.fullPath } }
    }
    if (to.meta?.requiresAdmin && data.data?.is_admin !== true) return { path: '/dashboard' }
    return true
  } catch (error) {
    if (error?.status === 401 || error?.status === 403) {
      clearAuthToken()
      return { path: '/login', query: { redirect: to.fullPath } }
    }
    // 后端暂时不可用时保留已有登录态，由页面显示真实接口错误。
    return true
  }
})

export default router
