<template>
  <div>
    <nav class="landing-nav">
      <div class="landing-nav-inner">
        <router-link to="/" class="sidebar-logo"><span class="dot"></span>{{ t('brand') }}</router-link>
        <div class="landing-links">
          <router-link to="/models">{{ t('models') }}</router-link>
          <router-link to="/docs">{{ t('docs') }}</router-link>
          <router-link to="/login" class="btn btn-primary">{{ t('login') }}</router-link>
        </div>
      </div>
    </nav>

    <main class="legal-page">
      <div class="section-label">{{ t('legalUpdated') }}</div>
      <h1>{{ labels.title }}</h1>
      <div class="panel legal-panel">
        <template v-for="section in labels.sections" :key="section.title">
          <h2>{{ section.title }}</h2>
          <p>
            {{ section.text }}
            <a v-if="section.contact" href="mailto:support@openbridgetech.ca">support@openbridgetech.ca</a>
          </p>
        </template>
      </div>
    </main>

    <SiteFooter />
  </div>
</template>

<script setup>
import { computed } from 'vue'
import SiteFooter from '../components/SiteFooter.vue'
import { useI18n } from '../i18n.js'

const { t, isZh } = useI18n()

const labels = computed(() => isZh.value ? {
  title: '隐私政策',
  sections: [
    { title: '我们收集的信息', text: '我们会收集运营 Nexus Gateway 所需的账号信息，例如邮箱、昵称、登录记录、接口密钥元数据、余额记录、订单和 API 使用记录。' },
    { title: 'API 请求处理', text: 'API 请求会被发送到用户选择的上游供应商，包括通过 BYOK 配置选定的供应商。服务会保存模型、Token 用量、费用、延迟和状态码等运营元数据，不会主动把完整提示词或响应正文写入调用日志。' },
    { title: 'BYOK 供应商凭据', text: '用户提交的供应商 API Key 只以加密形式保存在平台凭据库中，设置页不会再次展示明文。模型请求会把必要内容发送给用户选择的上游供应商，并受该供应商自身政策约束。' },
    { title: '凭据删除与保留边界', text: '用户可以在设置页删除 BYOK 凭据，使其停止用于新的模型请求。备份、审计记录或法律要求涉及的保留边界以实际运营政策和适用法律为准；我们不在此承诺固定保留期限或即时物理清除。' },
    { title: '信息用途', text: '我们使用账号和用量信息完成用户认证、接口访问安全、余额计算、账单记录、服务排障和滥用防护。' },
    { title: '服务供应商', text: '启用对应集成后，请求可能由 OpenAI、Anthropic、Google、托管服务、DNS/CDN 服务和支付服务处理。' },
    { title: '联系', text: '隐私相关请求请联系 ', contact: true },
  ],
} : {
  title: 'Privacy Policy',
  sections: [
    { title: 'Information we collect', text: 'We collect account information such as email address, nickname, login records, API key metadata, balance records, orders and API usage records needed to operate Nexus Gateway.' },
    { title: 'API request handling', text: 'API requests are sent to the upstream provider selected by the user, including providers selected through BYOK. The service stores operational metadata such as model, token usage, cost, latency and status code. It does not intentionally store full prompt or response text in call logs.' },
    { title: 'BYOK provider credentials', text: 'Provider API keys submitted by users are stored only in encrypted form in the platform credential store, and plaintext is not displayed again in Settings. Model requests send necessary content to the upstream provider selected by the user and remain subject to that provider\'s policies.' },
    { title: 'Credential deletion and retention boundary', text: 'Users can delete BYOK credentials in Settings so they stop being used for new model requests. Retention boundaries involving backups, audit records, or legal requirements follow actual operating policies and applicable law; this policy does not promise a fixed retention period or immediate physical erasure.' },
    { title: 'How we use information', text: 'We use account and usage information to authenticate users, secure API access, calculate token balance, provide billing records, troubleshoot service issues and prevent abuse.' },
    { title: 'Service providers', text: 'Requests may be processed by OpenAI, Anthropic, Google, hosting providers, DNS/CDN providers and payment providers when those integrations are enabled.' },
    { title: 'Contact', text: 'For privacy requests, contact ', contact: true },
  ],
})
</script>
