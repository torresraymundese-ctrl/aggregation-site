<template>
  <div class="auth-page">
    <button class="lang-toggle auth-lang-top" @click="toggleLang">{{ t('language') }}</button>
    <div class="auth-card">
      <router-link to="/" class="sidebar-logo"><span class="dot"></span>{{ t('brand') }}</router-link>
      <h1>{{ t('login') }}</h1>
      <p>{{ t('loginDesc') }}</p>
      <form class="auth-form" @submit.prevent="handleLogin">
        <div class="form-group">
          <label>{{ t('email') }}</label>
          <input v-model="form.email" type="email" required placeholder="you@example.com" />
        </div>
        <div class="form-group">
          <label>{{ t('password') }}</label>
          <input v-model="form.password" type="password" required placeholder="••••••••" />
        </div>
        <button type="submit" class="btn btn-primary">{{ t('login') }}</button>
      </form>
    </div>
  </div>
</template>

<script setup>
import { reactive } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { apiErrorMessage, requestJson } from '../api.js'
import { setAuthToken } from '../auth.js'
import { useI18n } from '../i18n.js'
import { notifyError } from '../notify.js'

const { t, toggleLang, isZh } = useI18n()
const router = useRouter()
const route = useRoute()
const form = reactive({ email: '', password: '' })
const emailPattern = /^[^\s@]+@[^\s@]+\.[^\s@]+$/

const handleLogin = async () => {
  if (!emailPattern.test(form.email.trim())) {
    notifyError(t('invalidEmail'))
    return
  }
  if (form.password.length < 6) {
    notifyError(t('passwordMin'))
    return
  }

  try {
    const data = await requestJson('/api/auth/login', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ email: form.email.trim(), password: form.password }),
    })
    if (data.code === 0) {
      if (!data.data?.token) {
        notifyError(isZh.value ? '登录响应缺少令牌。' : 'The sign-in response did not include a token.')
        return
      }
      setAuthToken(data.data.token)
      router.push(route.query.redirect || '/dashboard')
    } else {
      notifyError(data.message || t('loginFailed'))
    }
  } catch (e) {
    notifyError(apiErrorMessage(e, isZh.value
      ? '登录服务暂不可用，请确认后端已启动。'
      : 'The sign-in service is unavailable. Please verify the backend is running.'))
  }
}
</script>
