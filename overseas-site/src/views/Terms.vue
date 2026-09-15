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
  title: '服务条款',
  sections: [
    { title: '服务使用', text: 'Nexus Gateway 为支持的 AI 供应商提供统一 API 接入和平台余额管理。你需要自行保护账号凭证和接口密钥安全。' },
    { title: 'BYOK 凭据与上游处理', text: '使用 BYOK 时，你授权平台加密保存所提交的供应商凭据，并将模型请求发送给你选择的上游供应商。你需要确保有权使用该凭据，并遵守对应供应商的条款和政策。' },
    { title: '凭据删除', text: '你可以在设置页删除 BYOK 凭据，使其停止用于新的模型请求。删除不代表对备份、审计记录或法律要求范围内数据作出即时物理清除或固定保留期限承诺。' },
    { title: '生产测试状态', text: '当前生产环境处于受控测试阶段。公开注册已关闭，账号由运营方创建。' },
    { title: '计费与余额', text: 'Token 用量根据模型输入和输出 Token 数计算。付费计费功能在商业使用前必须完成验证。' },
    { title: '可接受使用', text: '不得将服务用于滥用、未授权访问、凭证共享、违法内容生成或绕过供应商安全系统。' },
    { title: '可用性', text: '服务依赖上游 AI 供应商、托管基础设施和网络服务。服务可用性和模型访问能力可能发生变化。' },
    { title: '联系', text: '需要支持请联系 ', contact: true },
  ],
} : {
  title: 'Terms of Service',
  sections: [
    { title: 'Use of the service', text: 'Nexus Gateway provides unified API access and platform balance management for supported AI providers. You are responsible for keeping account credentials and API keys secure.' },
    { title: 'BYOK credentials and upstream processing', text: 'When using BYOK, you authorize the platform to store the submitted provider credential in encrypted form and send model requests to the upstream provider you select. You must have authority to use that credential and comply with the provider\'s terms and policies.' },
    { title: 'Credential deletion', text: 'You can delete a BYOK credential in Settings so it stops being used for new model requests. Deletion is not a promise of immediate physical erasure or a fixed retention period for data within backups, audit records, or legal requirements.' },
    { title: 'Production testing status', text: 'The current production environment is in controlled testing. Public registration is closed, and accounts are created by the operator.' },
    { title: 'Billing and balance', text: 'Token usage is calculated from model input and output token counts. Paid billing features must be verified before commercial use.' },
    { title: 'Acceptable use', text: 'You may not use the service for abuse, unauthorized access, credential sharing, illegal content generation or attempts to bypass provider safety systems.' },
    { title: 'Availability', text: 'The service depends on upstream AI providers, hosting infrastructure and network providers. Service availability and model access may change.' },
    { title: 'Contact', text: 'For support, contact ', contact: true },
  ],
})
</script>
