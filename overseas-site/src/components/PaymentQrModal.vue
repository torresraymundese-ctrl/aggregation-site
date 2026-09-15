<template>
  <Teleport to="body">
    <Transition name="payment-fade">
      <div v-if="order" class="payment-backdrop" role="dialog" aria-modal="true">
        <section class="payment-card">
          <button class="payment-close" type="button" @click="$emit('close')">×</button>

          <div class="payment-art">
            <div class="payment-art-inner">
              <img :src="qrSrc" alt="OpenBridge Technology Inc. payment QR code" />
            </div>
            <div class="payment-methods">
              <span>WeChat Pay</span>
              <span>Alipay</span>
              <span>UnionPay</span>
            </div>
          </div>

          <div class="payment-info">
            <span class="badge badge-accent">OpenBridge Technology Inc.</span>
            <h3>{{ labels.title }}</h3>
            <p>{{ labels.desc }}</p>

            <div class="payment-amount">
              <span>{{ labels.amount }}</span>
              <strong>${{ money(order.amount) }}</strong>
              <small>{{ labels.balanceCredit }}：${{ money(creditAmount) }} USD</small>
            </div>

            <div class="payment-meta">
              <div>
                <span>{{ labels.orderNo }}</span>
                <strong class="mono">{{ order.order_no }}</strong>
              </div>
              <div>
                <span>{{ labels.packageName }}</span>
                <strong>{{ order.package_name || '--' }}</strong>
              </div>
            </div>

            <div class="payment-steps">
              <div v-for="step in labels.steps" :key="step.no">
                <span>{{ step.no }}</span>
                <p>{{ step.text }}</p>
              </div>
            </div>

            <p class="payment-note">{{ labels.note }}</p>

            <div class="modal-actions">
              <button class="btn btn-ghost" type="button" @click="copyOrderNo">{{ labels.copyOrder }}</button>
              <button class="btn btn-primary" type="button" @click="$emit('close')">{{ labels.done }}</button>
            </div>
          </div>
        </section>
      </div>
    </Transition>
  </Teleport>
</template>

<script setup>
import { computed } from 'vue'
import { useI18n } from '../i18n.js'
import { notifyError, notifySuccess } from '../notify.js'

const props = defineProps({
  order: { type: Object, default: null },
})

defineEmits(['close'])

const { isZh } = useI18n()
const qrSrc = '/payment/openbridge-iotpay-qr.jpg'

const labels = computed(() => isZh.value ? {
  title: '扫码付款',
  desc: '请使用微信、支付宝或银联扫码，并手动输入下方应付金额。',
  amount: '应付金额',
  balanceCredit: '确认到账余额',
  orderNo: '订单号',
  packageName: '套餐',
  copyOrder: '复制订单号',
  done: '我已完成付款',
  note: '付款后订单会保持待确认状态。管理员核对到账后，按实际付款美元金额增加账户 USD 余额。',
  steps: [
    { no: '01', text: '打开扫码功能' },
    { no: '02', text: '扫描左侧收款码' },
    { no: '03', text: '输入完全一致的金额并付款' },
  ],
} : {
  title: 'Scan to pay',
  desc: 'Use WeChat Pay, Alipay or UnionPay, then enter the exact amount below.',
  amount: 'Amount due',
  balanceCredit: 'Balance credited after confirmation',
  orderNo: 'Order No.',
  packageName: 'Package',
  copyOrder: 'Copy order No.',
  done: 'I have paid',
  note: 'The order stays pending until an admin confirms the payment. Your USD balance is then credited by the actual USD payment amount.',
  steps: [
    { no: '01', text: 'Open scan function' },
    { no: '02', text: 'Scan the payment QR' },
    { no: '03', text: 'Enter the exact amount and pay' },
  ],
})

const money = (value) => Number(value || 0).toFixed(2)
const creditAmount = computed(() => props.order?.balance_credit ?? props.order?.credit_amount ?? props.order?.amount ?? 0)

const copyOrderNo = async () => {
  try {
    await navigator.clipboard.writeText(props.order?.order_no || '')
    notifySuccess(isZh.value ? '订单号已复制' : 'Order number copied')
  } catch (error) {
    notifyError(error.message)
  }
}
</script>

<style scoped>
.payment-backdrop {
  position: fixed;
  inset: 0;
  z-index: 72;
  display: grid;
  place-items: center;
  padding: 22px;
  background:
    radial-gradient(circle at 24% 20%, rgba(77,139,247,.16), transparent 34%),
    rgba(0,0,0,.68);
  backdrop-filter: blur(14px);
}

.payment-card {
  position: relative;
  display: grid;
  grid-template-columns: minmax(280px, 390px) minmax(320px, 460px);
  gap: 22px;
  width: min(900px, 100%);
  border: 1px solid rgba(255,255,255,.13);
  border-radius: 8px;
  background:
    linear-gradient(145deg, rgba(255,255,255,.075), transparent 38%),
    rgba(18,22,30,.96);
  box-shadow: 0 34px 110px rgba(0,0,0,.56);
  padding: 18px;
}

.payment-close {
  position: absolute;
  top: 12px;
  right: 12px;
  z-index: 2;
  width: 34px;
  height: 34px;
  border: 1px solid rgba(255,255,255,.14);
  border-radius: 999px;
  background: rgba(0,0,0,.34);
  color: var(--fg);
  font-size: 20px;
  line-height: 1;
}

.payment-art {
  display: grid;
  align-content: start;
  gap: 12px;
  min-width: 0;
}

.payment-art-inner {
  overflow: hidden;
  border: 1px solid rgba(45,212,191,.38);
  border-radius: 8px;
  background: linear-gradient(180deg, #0aa7bd, #0896ad);
  box-shadow:
    0 24px 70px rgba(0,0,0,.36),
    0 0 54px rgba(20,184,166,.16);
}

.payment-art-inner img {
  display: block;
  width: 100%;
  aspect-ratio: 800 / 1140;
  object-fit: cover;
}

.payment-methods {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}

.payment-methods span {
  border: 1px solid rgba(255,255,255,.11);
  border-radius: 999px;
  background: rgba(255,255,255,.055);
  color: rgba(232,239,251,.88);
  padding: 6px 10px;
  font-size: 12px;
  font-weight: 750;
}

.payment-info {
  display: grid;
  align-content: start;
  gap: 16px;
  padding: 12px 10px 8px;
}

.payment-info h3 {
  font-size: 30px;
  line-height: 1.05;
}

.payment-info p {
  color: var(--muted);
}

.payment-amount {
  border: 1px solid rgba(77,139,247,.32);
  border-radius: 8px;
  background:
    linear-gradient(135deg, rgba(77,139,247,.18), rgba(20,184,166,.1)),
    rgba(8,13,20,.86);
  padding: 16px;
}

.payment-amount span,
.payment-meta span {
  display: block;
  color: var(--muted);
  font-size: 12px;
}

.payment-amount strong {
  display: block;
  margin-top: 5px;
  color: #fff;
  font-size: 38px;
  line-height: 1;
}

.payment-amount small {
  display: block;
  margin-top: 9px;
  color: var(--muted);
}

.payment-meta {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
}

.payment-meta div,
.payment-steps div {
  min-width: 0;
  border: 1px solid rgba(255,255,255,.1);
  border-radius: 8px;
  background: rgba(7,10,15,.72);
  padding: 12px;
}

.payment-meta strong {
  display: block;
  margin-top: 5px;
  overflow-wrap: anywhere;
}

.payment-steps {
  display: grid;
  gap: 9px;
}

.payment-steps div {
  display: grid;
  grid-template-columns: 34px 1fr;
  gap: 10px;
  align-items: center;
}

.payment-steps span {
  color: var(--accent);
  font-family: var(--font-mono);
  font-weight: 850;
}

.payment-note {
  border-left: 3px solid var(--accent);
  padding-left: 12px;
}

.payment-fade-enter-active,
.payment-fade-leave-active {
  transition: opacity .18s linear;
}

.payment-fade-enter-from,
.payment-fade-leave-to {
  opacity: 0;
}

@media (max-width: 760px) {
  .payment-backdrop {
    align-items: start;
    overflow: auto;
    padding: 14px;
  }

  .payment-card {
    grid-template-columns: 1fr;
  }

  .payment-art-inner img {
    max-height: 420px;
    object-fit: contain;
  }

  .payment-meta {
    grid-template-columns: 1fr;
  }
}
</style>
