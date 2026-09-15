<template>
  <ShellLayout :title="t('orders')" :subtitle="t('ordersSubtitle')">
    <div class="panel">
      <div class="panel-header"><h3>{{ t('orders') }}</h3><router-link to="/purchase" class="btn btn-primary btn-sm">{{ t('buyTokens') }}</router-link></div>
      <div v-if="loading" class="empty-state">{{ t('loading') }}</div>
      <div v-else-if="error" class="empty-state danger">{{ error }}</div>
      <div v-else-if="!orders.length" class="empty-state">{{ labels.noOrders }}</div>
      <div v-else class="table-wrap">
        <table class="data-table">
          <thead><tr><th>{{ t('orderNo') }}</th><th>{{ t('packageName') }}</th><th>{{ t('amount') }}</th><th>{{ t('status') }}</th><th>{{ t('time') }}</th><th>{{ t('action') }}</th></tr></thead>
          <tbody>
            <tr v-for="order in orders" :key="order.order_no">
              <td class="mono">{{ order.order_no }}</td>
              <td>{{ order.package_name }}</td>
              <td class="accent">${{ order.amount }}</td>
              <td><span :class="['badge', orderStatusClass(order.status)]">{{ orderStatusText(order.status) }}</span></td>
              <td class="mono">{{ formatTime(order.created_at) }}</td>
              <td>
                <button v-if="order.status === 0" class="btn btn-primary btn-sm" type="button" @click="paymentOrder = order">{{ t('payNow') }}</button>
                <span v-else class="muted">--</span>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
    </div>
    <PaymentQrModal :order="paymentOrder" @close="paymentOrder = null" />
  </ShellLayout>
</template>

<script setup>
import { computed, onMounted, ref } from 'vue'
import { apiErrorMessage, requestJson } from '../api.js'
import { authHeaders } from '../auth.js'
import PaymentQrModal from '../components/PaymentQrModal.vue'
import ShellLayout from '../components/ShellLayout.vue'
import { useI18n } from '../i18n.js'

const { t, isZh } = useI18n()
const orders = ref([])
const loading = ref(true)
const error = ref('')
const paymentOrder = ref(null)
const labels = computed(() => isZh.value ? {
  noOrders: '暂无订单。',
  loadFailed: '订单加载失败。',
} : {
  noOrders: 'No orders yet.',
  loadFailed: 'Failed to load orders.',
})
const formatTime = (value) => value ? new Date(value).toLocaleString() : '--'
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

onMounted(async () => {
  try {
    const data = await requestJson('/api/orders', { headers: authHeaders() })
    if (data.code !== 0) throw new Error(data.message || labels.value.loadFailed)
    orders.value = data.data?.items || []
  } catch (e) {
    error.value = apiErrorMessage(e, e?.message || labels.value.loadFailed)
  } finally {
    loading.value = false
  }
})
</script>
