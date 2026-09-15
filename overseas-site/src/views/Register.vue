<template>
  <div class="auth-page">
    <div class="auth-card">
      <router-link to="/" class="sidebar-logo"><span class="dot"></span>{{ t('brand') }}</router-link>
      <h1>{{ t('register') }}</h1>
      <p>{{ t('landingDesc') }}</p>
      <form class="auth-form" @submit.prevent="handleRegister">
        <div class="form-group">
          <label>{{ t('email') }}</label>
          <input v-model="form.email" type="email" required placeholder="you@example.com" />
        </div>
        <div class="form-group">
          <label>{{ t('nickname') }}</label>
          <input v-model="form.nickname" required placeholder="Nexus user" />
        </div>
        <div class="form-group">
          <label>{{ t('password') }}</label>
          <input v-model="form.password" type="password" required placeholder="At least 6 characters" />
        </div>
        <button type="submit" class="btn btn-primary">{{ t('register') }}</button>
      </form>
      <p class="auth-footer">{{ t('hasAccount') }} <router-link to="/login">{{ t('login') }}</router-link></p>
      <button class="lang-toggle" style="margin-top:14px;" @click="toggleLang">{{ t('language') }}</button>
    </div>
  </div>
</template>

<script setup>
import { reactive } from 'vue'
import { useRouter } from 'vue-router'
import { useI18n } from '../i18n.js'
import { notifyError } from '../notify.js'

const { t, toggleLang } = useI18n()
const router = useRouter()
const form = reactive({ email: '', nickname: '', password: '' })

const handleRegister = async () => {
  try {
    const res = await fetch('/api/auth/register', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(form),
    })
    const data = await res.json()
    if (data.code === 0) {
      router.push('/login')
    } else {
      notifyError(data.message || 'Registration failed')
    }
  } catch (e) {
    notifyError(e.message)
  }
}
</script>
