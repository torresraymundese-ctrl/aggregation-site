<template>
  <div class="galaxy-home">
    <nav class="landing-nav">
      <div class="landing-nav-inner">
        <router-link to="/" class="sidebar-logo"><span class="dot"></span>{{ t('brand') }}</router-link>
        <div class="landing-links">
          <router-link to="/models">{{ t('models') }}</router-link>
          <router-link to="/docs">{{ t('docs') }}</router-link>
          <button class="lang-toggle" @click="toggleLang">{{ t('language') }}</button>
          <router-link to="/login" class="btn btn-ghost">{{ t('login') }}</router-link>
          <router-link to="/login" class="btn btn-primary">{{ t('startNow') }}</router-link>
        </div>
      </div>
    </nav>

    <main class="galaxy-stage">
      <section class="platform-intro-copy" aria-label="platform introduction">
        <h2>{{ labels.introTitle }}</h2>
        <p>{{ labels.introText }}</p>
      </section>

      <div v-if="modelsLoading" class="empty-state galaxy-empty">{{ t('loading') }}</div>
      <div v-else-if="modelsError" class="empty-state danger galaxy-empty">{{ modelsError }}</div>
      <div v-else-if="!filteredModels.length" class="empty-state galaxy-empty">{{ labels.noModels }}</div>
      <section v-else class="lego-workbench-layout">
        <TokenBlockScene
          :models="filteredModels"
          :selected-id="selectedModelId"
          @select="toggleModelDock"
          @open="goLogin"
        />

        <Transition name="dock-fade">
          <aside v-if="isModelDockOpen && selectedModel" class="galaxy-model-dock lego-dock">
            <span :class="['provider-mark', providerTone(selectedModel?.provider)]">{{ providerShort(selectedModel?.provider) }}</span>
            <h2>{{ displayModelName(selectedModel) || labels.selectModel }}</h2>
            <p class="mono">{{ selectedModel?.model_id || labels.modelId }}</p>
            <div class="block-wallet-strip">
              <span>{{ labels.blockWallet }}</span>
              <strong>{{ labels.sharedBlocks }}</strong>
            </div>

            <div class="galaxy-facts">
              <div>
                <span>{{ labels.provider }}</span>
                <strong>{{ displayProvider(selectedModel?.provider) || '-' }}</strong>
              </div>
              <div>
                <span>{{ labels.context }}</span>
                <strong>{{ compactNumber(selectedModel?.context_len) }}</strong>
              </div>
              <div>
                <span>{{ labels.inputPrice }}</span>
                <strong>${{ money(selectedModel?.input_rate) }}</strong>
              </div>
              <div>
                <span>{{ labels.outputPrice }}</span>
                <strong>${{ money(selectedModel?.output_rate) }}</strong>
              </div>
            </div>

            <button class="btn btn-primary" @click="goLogin(selectedModel)">{{ labels.loginToUse }}</button>
          </aside>
        </Transition>
      </section>
    </main>
  </div>
</template>

<script setup>
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import TokenBlockScene from '../components/TokenBlockScene.vue'
import { apiErrorMessage, requestJson } from '../api.js'
import { useI18n } from '../i18n.js'

const { t, toggleLang, isZh } = useI18n()
const router = useRouter()

const models = ref([])
const modelsLoading = ref(true)
const modelsError = ref('')
const activeProvider = ref('all')
const selectedModel = ref(null)
const isModelDockOpen = ref(false)
let modelDockTimer = null
let modelDockCleanupTimer = null

const labels = computed(() => isZh.value ? {
  kicker: 'TOKEN SCENE',
  title: 'Token 3D 积木场景',
  introTitle: '一次充值，一把 Key，调用多种 AI 模型',
  introText: '把 Token 当作积木来组合：选择模型、发送请求、统一余额自动扣费，国内外模型都从同一个网关进入。',
  modelUnit: '模型积木',
  providerUnit: '供应商',
  noModels: '暂无开放模型。',
  selectModel: '选择模型',
  modelId: 'model-id',
  provider: '供应商',
  context: '上下文',
  inputPrice: '输入 / 每 100 万 Token',
  outputPrice: '输出 / 每 100 万 Token',
  loginToUse: '登录使用',
  blockWallet: '统一积木余额',
  sharedBlocks: '所有模型共用',
} : {
  kicker: 'TOKEN SCENE',
  title: 'Token 3D Block Scene',
  introTitle: 'One balance. One API key. Many AI models.',
  introText: 'Build with token blocks: choose a model in each request, route through one gateway, and let the shared balance handle billing.',
  modelUnit: 'model blocks',
  providerUnit: 'providers',
  noModels: 'No open models are configured yet.',
  selectModel: 'Select model',
  modelId: 'model-id',
  provider: 'Provider',
  context: 'Context',
  inputPrice: 'Input / 1M tokens',
  outputPrice: 'Output / 1M tokens',
  loginToUse: 'Sign in to use',
  blockWallet: 'Unified block balance',
  sharedBlocks: 'Shared by every model',
})

const providerFilters = computed(() => [
  { value: 'all', label: isZh.value ? '全部' : 'All' },
  { value: 'Volcengine', label: isZh.value ? '火山引擎' : 'Volcengine' },
  { value: 'DeepSeek', label: 'DeepSeek' },
  { value: 'Zhipu', label: isZh.value ? '智谱 GLM' : 'GLM' },
  { value: 'Qwen', label: isZh.value ? '通义千问' : 'Qwen' },
  { value: 'Moonshot', label: 'Kimi' },
  { value: 'MiniMax', label: 'MiniMax' },
  { value: 'StepFun', label: isZh.value ? '阶跃星辰' : 'StepFun' },
  { value: 'OpenAI', label: 'OpenAI' },
  { value: 'Anthropic', label: 'Claude' },
  { value: 'Google', label: 'Gemini' },
])

const filteredModels = computed(() => {
  if (activeProvider.value === 'all') return models.value
  return models.value.filter((model) => sameProvider(model.provider, activeProvider.value))
})

const providerCount = computed(() => new Set(filteredModels.value.map((model) => providerShort(model.provider))).size)
const selectedModelId = computed(() => isModelDockOpen.value ? selectedModel.value?.model_id || '' : '')

const sameProvider = (value, providerName) => {
  const left = String(value || '').toLowerCase()
  const right = String(providerName || '').toLowerCase()
  if (right.includes('google')) return left.includes('google') || left.includes('gemini')
  if (right.includes('anthropic')) return left.includes('anthropic') || left.includes('claude')
  if (right.includes('volc')) return left.includes('volc') || left.includes('火山')
  if (right.includes('zhipu')) return left.includes('zhipu') || left.includes('glm') || left.includes('智谱')
  if (right.includes('qwen')) return left.includes('qwen') || left.includes('dashscope') || left.includes('千问')
  if (right.includes('moonshot')) return left.includes('moonshot') || left.includes('kimi')
  return left.includes(right)
}

const providerShort = (provider) => {
  const value = String(provider || '').toLowerCase()
  if (value.includes('openai')) return 'OA'
  if (value.includes('anthropic') || value.includes('claude')) return 'CL'
  if (value.includes('google') || value.includes('gemini')) return 'GM'
  if (value.includes('volc') || value.includes('火山')) return 'VC'
  if (value.includes('deepseek')) return 'DS'
  if (value.includes('zhipu') || value.includes('glm') || value.includes('智谱')) return 'GL'
  if (value.includes('qwen') || value.includes('dashscope') || value.includes('千问')) return 'QW'
  if (value.includes('moonshot') || value.includes('kimi')) return 'KM'
  if (value.includes('minimax')) return 'MM'
  if (value.includes('stepfun')) return 'SF'
  return 'AI'
}

const providerTone = (provider) => {
  const short = providerShort(provider)
  if (short === 'OA') return 'tone-green'
  if (short === 'CL') return 'tone-amber'
  if (short === 'GM') return 'tone-sky'
  if (short === 'VC') return 'tone-blue'
  if (short === 'DS') return 'tone-red'
  if (short === 'GL') return 'tone-purple'
  if (short === 'QW') return 'tone-cyan'
  if (short === 'KM') return 'tone-pink'
  if (short === 'MM') return 'tone-gray'
  if (short === 'SF') return 'tone-slate'
  return 'tone-blue'
}

const displayProvider = (provider) => {
  if (!provider) return ''
  if (!isZh.value) return provider
  if (sameProvider(provider, 'Volcengine')) return '火山引擎'
  if (sameProvider(provider, 'Zhipu')) return '智谱 GLM'
  if (sameProvider(provider, 'Qwen')) return '通义千问'
  if (sameProvider(provider, 'Moonshot')) return 'Kimi'
  if (sameProvider(provider, 'StepFun')) return '阶跃星辰'
  if (sameProvider(provider, 'Anthropic')) return 'Claude'
  if (sameProvider(provider, 'Google')) return 'Gemini'
  return provider
}

const compactNumber = (value) => {
  const number = Number(value || 0)
  if (number >= 1000000) return `${number / 1000000}M`
  if (number >= 1000) return `${number / 1000}K`
  return String(number)
}

const money = (value) => Number(value || 0).toFixed(6).replace(/0+$/, '').replace(/\.$/, '.0')

const displayModelName = (model) => {
  const name = String(model?.display_name || model?.model_id || '')
  return name.replace(/\s+Placeholder$/i, '')
}

const goLogin = (model) => {
  router.push({ path: '/login', query: model?.model_id ? { model: model.model_id } : {} })
}

const clearModelDockTimer = () => {
  if (!modelDockTimer) return
  window.clearTimeout(modelDockTimer)
  modelDockTimer = null
}

const clearModelDockCleanupTimer = () => {
  if (!modelDockCleanupTimer) return
  window.clearTimeout(modelDockCleanupTimer)
  modelDockCleanupTimer = null
}

const closeModelDock = () => {
  clearModelDockTimer()
  clearModelDockCleanupTimer()
  isModelDockOpen.value = false
  modelDockCleanupTimer = window.setTimeout(() => {
    if (!isModelDockOpen.value) selectedModel.value = null
    modelDockCleanupTimer = null
  }, 380)
}

const scheduleModelDockClose = () => {
  clearModelDockTimer()
  modelDockTimer = window.setTimeout(closeModelDock, 5000)
}

const toggleModelDock = (model) => {
  if (isModelDockOpen.value && selectedModel.value?.model_id === model?.model_id) {
    closeModelDock()
    return
  }
  clearModelDockCleanupTimer()
  selectedModel.value = model
  isModelDockOpen.value = true
  scheduleModelDockClose()
}

watch(filteredModels, (items) => {
  if (!items.some((model) => model.model_id === selectedModel.value?.model_id)) {
    closeModelDock()
  }
})

onMounted(async () => {
  try {
    const data = await requestJson('/api/models')
    if (data.code !== 0) {
      modelsError.value = data.message || (isZh.value ? '模型加载失败。' : 'Failed to load models.')
      return
    }
    models.value = data.data?.items || []
  } catch (e) {
    modelsError.value = apiErrorMessage(e, isZh.value
      ? '模型服务暂不可用，请确认后端已启动。'
      : 'The model service is unavailable. Please verify the backend is running.')
  } finally {
    modelsLoading.value = false
  }
})

onBeforeUnmount(() => {
  clearModelDockTimer()
  clearModelDockCleanupTimer()
})
</script>
