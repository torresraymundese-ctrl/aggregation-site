<template>
  <ShellLayout :title="t('orders')" :subtitle="t('adminOrdersSubtitle')" admin>
    <div class="metric-grid">
      <div class="metric-card"><div class="label">{{ labels.paidRevenue }}</div><div class="value">${{ paidRevenue }}</div><div class="sub">{{ labels.loadedOrders }}</div></div>
      <div class="metric-card"><div class="label">{{ t('paidOrders') }}</div><div class="value">{{ paidOrders }}</div><div class="sub">{{ t('settled') }}</div></div>
      <div class="metric-card"><div class="label">{{ t('pending') }}</div><div class="value">{{ pendingOrders }}</div><div class="sub">{{ t('awaitingPayment') }}</div></div>
      <div class="metric-card"><div class="label">{{ labels.refundedOrders }}</div><div class="value">{{ refundedOrders }}</div><div class="sub">{{ labels.loadedOrders }}</div></div>
    </div>
    <div class="panel">
      <div class="panel-header"><h3>{{ t('orders') }}</h3></div>
      <div v-if="loading" class="empty-state">{{ t('loading') }}</div>
      <div v-else-if="error" class="empty-state danger">{{ error }}</div>
      <div v-else-if="!orders.length" class="empty-state">{{ labels.noOrders }}</div>
      <div v-else class="table-wrap">
        <table class="data-table">
          <thead><tr><th>{{ t('orderNo') }}</th><th>{{ t('email') }}</th><th>{{ t('packageName') }}</th><th>{{ t('amount') }}</th><th>{{ t('status') }}</th><th>{{ t('action') }}</th></tr></thead>
          <tbody>
            <tr v-for="order in orders" :key="order.order_no">
              <td class="mono">{{ order.order_no }}</td>
              <td>{{ order.user_email || '--' }}</td>
              <td>{{ order.package_name }}</td>
              <td class="accent">${{ order.amount }}</td>
              <td><span :class="['badge', orderStatusClass(order.payment_status)]">{{ orderStatusText(order.payment_status) }}</span></td>
              <td>
                <div class="table-actions">
                  <button v-if="order.payment_status === 0" class="btn btn-primary btn-sm" @click="confirmPayment(order.id)">{{ t('confirmPaid') }}</button>
                  <button v-if="order.payment_status === 1" class="btn btn-danger btn-sm" @click="refundOrder(order.id)">{{ t('refund') }}</button>
                  <span v-if="order.payment_status !== 0 && order.payment_status !== 1" class="muted">--</span>
                </div>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
  </ShellLayout>
</template>

<script setup>
import { computed, onMounted, ref } from 'vue'
import { apiErrorMessage, requestJson } from '../api.js'
import { getAuthToken } from '../auth.js'
import ShellLayout from '../components/ShellLayout.vue'
import { useI18n } from '../i18n.js'
import { notifyError, notifySuccess } from '../notify.js'

const { t, isZh } = useI18n()
const orders = ref([])
const loading = ref(true)
const error = ref('')
const labels = computed(() => isZh.value ? {
  paidRevenue: '已支付收入',
  refundedOrders: '已退款订单',
  loadedOrders: '根据当前真实订单计算',
  noOrders: '暂无订单。',
  loadFailed: '订单加载失败。',
} : {
  paidRevenue: 'Paid revenue',
  refundedOrders: 'Refunded orders',
  loadedOrders: 'Calculated from loaded orders',
  noOrders: 'No orders yet.',
  loadFailed: 'Failed to load orders.',
})
const paidOrders = computed(() => orders.value.filter((order) => order.payment_status === 1).length)
const pendingOrders = computed(() => orders.value.filter((order) => order.payment_status === 0).length)
const refundedOrders = computed(() => orders.value.filter((order) => order.payment_status === 4).length)
const paidRevenue = computed(() => orders.value
  .filter((order) => order.payment_status === 1)
  .reduce((sum, order) => sum + Number(order.amount || 0), 0)
  .toFixed(4))
const orderStatusText = (status) => {
  if (status === 1) return t('paid')
  if (status === 4) return t('refunded')
  return t('pending')
}
const orderStatusClass = (status) => {
  if (status === 1) return 'badge-success'
  if (status === 4) return 'badge-danger'
  return 'badge-warn'
}

const fetchOrders = async () => {
  const token = getAuthToken()
  error.value = ''
  try {
    const data = await requestJson('/api/admin/orders', { headers: { Authorization: `Bearer ${token}` } })
    if (data.code !== 0) throw new Error(data.message || labels.value.loadFailed)
    orders.value = data.data?.items || []
  } catch (e) {
    error.value = apiErrorMessage(e, e?.message || labels.value.loadFailed)
  } finally {
    loading.value = false
  }
}

const confirmPayment = async (id) => {
  if (!id) return
  const token = getAuthToken()
  try {
    const data = await requestJson(`/api/admin/orders/${id}/confirm`, { method: 'POST', headers: { Authorization: `Bearer ${token}` } })
    if (data.code !== 0) throw new Error(data.message || t('confirmPaidFailed'))
    notifySuccess(t('confirmPaidSuccess'))
    await fetchOrders()
  } catch (e) {
    notifyError(apiErrorMessage(e, e?.message || t('confirmPaidFailed')))
  }
}

const refundOrder = async (id) => {
  if (!id) return
  const token = getAuthToken()
  try {
    const data = await requestJson(`/api/admin/orders/${id}/refund`, { method: 'POST', headers: { Authorization: `Bearer ${token}` } })
    if (data.code !== 0) throw new Error(data.message || t('refundFailed'))
    notifySuccess(t('refundSuccess'))
    await fetchOrders()
  } catch (e) {
    notifyError(apiErrorMessage(e, e?.message || t('refundFailed')))
  }
}

onMounted(fetchOrders)
</script>
