<template>
  <footer class="site-footer" :class="{ compact }">
    <div class="site-footer-inner">
      <div>
        <strong>{{ t('brand') }}</strong>
        <p>{{ t('footerDesc') }}</p>
      </div>
      <nav class="footer-links">
        <router-link to="/privacy">{{ t('privacyPolicy') }}</router-link>
        <router-link to="/terms">{{ t('termsService') }}</router-link>
        <a href="mailto:support@openbridgetech.ca">support@openbridgetech.ca</a>
      </nav>
      <div class="footer-copy">© 2026 OpenBridge Technology Inc.</div>
    </div>
  </footer>
</template>

<script setup>
import { useI18n } from '../i18n.js'

defineProps({
  compact: { type: Boolean, default: false },
})

const { t } = useI18n()
</script>
