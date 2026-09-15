<template>
  <ShellLayout :title="t('purchase')" :subtitle="t('purchaseSubtitle')">
    <div class="market-hero">
      <div>
        <span class="badge badge-success">{{ t('unifiedWallet') }}</span>
        <h2>{{ t('noPerModelRecharge') }}</h2>
        <p>{{ t('noPerModelRechargeDesc') }}</p>
      </div>
      <router-link to="/models" class="btn btn-ghost">{{ t('viewModelPrices') }}</router-link>
    </div>

    <div v-if="loading" class="empty-state">{{ t('loading') }}</div>
    <div v-else-if="error" class="empty-state danger">{{ error }}</div>
    <div v-else-if="!plans.length" class="empty-state">{{ t('noPackages') }}</div>
    <div v-else class="pricing-grid">
      <div v-for="plan in plans" :key="plan.id" class="pricing-card">
        <span v-if="plan.featured" class="badge badge-accent">{{ t('popular') }}</span>
        <h3>{{ plan.name }}</h3>
        <p>{{ labels.balanceCredit }}</p>
        <div class="price">${{ money(creditAmount(plan)) }}</div>
        <p class="unit-price">{{ labels.payAndReceive.replaceAll('{amount}', money(creditAmount(plan))) }}</p>
        <button class="btn btn-primary" @click="createOrder(plan.id)">{{ labels.addBalance }}</button>
      </div>
    </div>

    <div class="panel">
      <div class="panel-header">
        <h3>{{ t('billingWorks') }}</h3>
        <span class="badge badge-accent">{{ t('manualConfirm') }}</span>
      </div>
      <div class="balance-story tight">
        <div>
          <h3>{{ t('recharge') }}</h3>
          <p>{{ t('rechargeDesc') }}</p>
        </div>
        <div>
          <h3>{{ t('call') }}</h3>
          <p>{{ t('callDesc') }}</p>
        </div>
        <div>
          <h3>{{ t('deduct') }}</h3>
          <p>{{ t('deductDesc') }}</p>
        </div>
      </div>
    </div>
    <PaymentQrModal :order="paymentOrder" @close="paymentOrder = null" />
  </ShellLayout>
</template>

<script setup>
import { computed, onMounted, ref } from 'vue'
import { apiErrorMessage, requestJson } from '../api.js'
import { authHeaders, getAuthToken } from '../auth.js'
import PaymentQrModal from '../components/PaymentQrModal.vue'
import ShellLayout from '../components/ShellLayout.vue'
import { useI18n } from '../i18n.js'
import { notifyError, notifySuccess } from '../notify.js'

const { t, isZh } = useI18n()
const plans = ref([])
const loading = ref(true)
const error = ref('')
const paymentOrder = ref(null)
const labels = computed(() => isZh.value ? {
  balanceCredit: '到账 USD 余额',
  payAndReceive: '支付 ${amount}，到账 ${amount} USD 余额',
  addBalance: '充值 USD 余额',
} : {
  balanceCredit: 'USD balance credit',
  payAndReceive: 'Pay ${amount}, receive ${amount} in USD balance',
  addBalance: 'Add USD balance',
})

const money = (value) => Number(value || 0).toFixed(2)
const creditAmount = (plan) => plan.balance_credit ?? plan.credit_amount ?? plan.amount ?? plan.price ?? 0

const fetchPackages = async () => {
  try {
    const data = await requestJson('/api/packages')
    if (data.code !== 0) {
      error.value = data.message || t('packagesLoadFailed')
      return
    }
    const items = data.data?.items || []
    plans.value = items.map((item, index) => ({ ...item, featured: index === 1 }))
  } catch (e) {
    error.value = apiErrorMessage(e, e?.message || t('packagesLoadFailed'))
  } finally {
    loading.value = false
  }
}

const createOrder = async (packageId) => {
  const token = getAuthToken()
  if (!token) {
    notifyError(t('signInFirst'))
    return
  }

  try {
    const data = await requestJson('/api/orders', {
      method: 'POST',
      headers: authHeaders({ 'Content-Type': 'application/json' }),
      body: JSON.stringify({ package_id: packageId }),
    })
    if (data.code === 0) {
      paymentOrder.value = data.data
      notifySuccess(data.message || t('orderCreated'))
    } else {
      notifyError(data.message || t('orderCreated'))
    }
  } catch (e) {
    notifyError(apiErrorMessage(e, e?.message || t('orderCreated')))
  }
}

onMounted(fetchPackages)
</script>
