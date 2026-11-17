<template>
  <div class="appearance">
    <PageHeader :title="t('page.appearance.title')" :subtitle="t('page.appearance.sub')" />

    <div class="page-scroll">

    <section class="card">
      <header class="card__head">
        <h2 class="card__title">{{ t('appearance.outlook.title') }}</h2>
        <AppIcon name="sparkles" :size="16" class="card__icon" />
      </header>

      <p class="theme-title">{{ t('appearance.theme.title') }}</p>
      <p class="theme-desc">{{ t('appearance.theme.note') }}</p>

      <AppSegmented v-model="themeMode" :options="themeOptions" width="360px" class="theme-switch" />

      <p class="theme-status">
        <span class="theme-status__dot" />
        <span>{{ themeStatus }}</span>
      </p>
    </section>

    <section class="card">
      <header class="card__head">
        <h2 class="card__title">{{ t('appearance.startup.title') }}</h2>
      </header>

      <div class="toggle-row">
        <div>
          <p class="toggle-row__title">{{ t('appearance.autostart') }}</p>
          <p class="toggle-row__note">{{ t('appearance.autostartNote') }}</p>
        </div>
        <AppSwitch v-model="launchAtStartup" />
      </div>

      <div class="toggle-row">
        <div>
          <p class="toggle-row__title">{{ t('appearance.quit') }}</p>
          <p class="toggle-row__note">{{ t('appearance.quitNote') }}</p>
        </div>
        <AppSwitch v-model="confirmOnExit" />
      </div>
    </section>

    <section class="card">
      <header class="card__head">
        <h2 class="card__title">{{ t('appearance.notify.title') }}</h2>
        <AppIcon name="bell" :size="16" class="card__icon" />
      </header>
      <div class="inline-row">
        <div>
          <p class="toggle-row__title">{{ t('appearance.notify.finish') }}</p>
          <p class="toggle-row__note">{{ t('appearance.notify.note') }}</p>
        </div>
        <AppSwitch v-model="notifyOnFinish" />
      </div>
    </section>

    <section class="card">
      <header class="card__head">
        <h2 class="card__title">{{ t('appearance.ext.title') }}</h2>
        <AppIcon name="plugin" :size="16" class="card__icon" />
      </header>

      <p class="card__note">{{ t('appearance.ext.note') }}</p>

      <div class="extension-row">
        <AppSelect v-model="browser" :options="browserOptions" width="150px" />
        <AppLineInput v-model="extensionId" class="extension-input" :placeholder="t('appearance.ext.placeholder')" />
        <button class="extension-add" :disabled="!extensionId.trim()" @click="registerExtension">
          {{ t('appearance.ext.register') }}
        </button>
      </div>

      <p class="card__note">{{ t('appearance.ext.countPrefix') }}{{ extensions.length }}</p>

      <p v-if="!extensions.length" class="card__empty">{{ t('appearance.ext.empty') }}</p>

      <div v-for="item in extensions" :key="item.id" class="extension-item">
        <AppIcon name="globe" :size="14" class="extension-item__icon" />
        <div class="extension-item__text">
          <span class="extension-item__browser">{{ item.browser }}</span>
          <span class="extension-item__id">{{ item.id }}</span>
        </div>
        <button class="extension-item__remove" @click="removeExtension(item.browser)">{{ t('appearance.ext.remove') }}</button>
      </div>
    </section>

    <section class="card">
      <header class="card__head">
        <h2 class="card__title">{{ t('appearance.language.title') }}</h2>
      </header>
      <div class="inline-row">
        <div>
          <p class="toggle-row__title">{{ t('appearance.language.label') }}</p>
          <p class="toggle-row__note">{{ t('appearance.language.note') }}</p>
        </div>
        <AppSelect v-model="language" :options="languageOptions" width="180px" />
      </div>
    </section>

    <section class="card">
      <header class="card__head">
        <h2 class="card__title">{{ t('appearance.plan.title') }}</h2>
      </header>
      <div class="inline-row">
        <p class="toggle-row__title">{{ t('appearance.plan.label') }}</p>
        <AppSelect v-model="planAction" :options="planActionOptions" width="160px" />
      </div>
    </section>

    <section class="card">
      <header class="card__head">
        <h2 class="card__title">{{ t('appearance.sleep.title') }}</h2>
      </header>
      <div class="inline-row">
        <div>
          <p class="toggle-row__title">{{ t('appearance.sleep.label') }}</p>
          <p class="toggle-row__note">{{ t('appearance.sleep.note') }}</p>
        </div>
        <AppSwitch v-model="keepAwake" />
      </div>
    </section>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'

import AppIcon from '@/components/AppIcon.vue'
import PageHeader from '@/components/PageHeader.vue'
import AppLineInput from '@/components/ui/AppLineInput.vue'
import AppSegmented from '@/components/ui/AppSegmented.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import AppSwitch from '@/components/ui/AppSwitch.vue'
import {
  listExtensions,
  registerExtensionHost,
  unregisterExtensionHost,
  type ExtensionEntry,
} from '@/bridge/extension'
import { showToast } from '@/composables/useToast'
import { setLanguage, t } from '@/i18n'
import { useTheme } from '@/composables/useTheme'
import { ensureSettings, patchSettings } from '@/stores/settings'

/* 主题状态在模块级保存，进出页面不会重置 */
const { mode: themeMode, resolved } = useTheme()

const launchAtStartup = ref(false)
const confirmOnExit = ref(true)
const notifyOnFinish = ref(true)
const keepAwake = ref(true)
const browser = ref('edge')
const extensionId = ref('')
const language = ref('system')
const planAction = ref('shutdown')

const extensions = ref<ExtensionEntry[]>([])

let hydrating = true

function persist() {
  if (hydrating) return
  patchSettings({
    autostart: launchAtStartup.value,
    quitConfirm: confirmOnExit.value,
    notifyOnFinish: notifyOnFinish.value,
    theme: themeMode.value,
    planAction: planAction.value,
    keepAwake: keepAwake.value,
  })
}

const themeOptions = computed(() => [
  { label: t('theme.follow'), value: 'system' },
  { label: t('theme.light'), value: 'light' },
  { label: t('theme.dark'), value: 'dark' },
])

const browserOptions = [
  { label: 'Edge', value: 'edge' },
  { label: 'Chrome', value: 'chrome' },
]

const languageOptions = computed(() => [
  { label: t('language.followSystem'), value: 'system' },
  { label: t('language.zhCn'), value: 'zh-cn' },
  { label: t('language.zhTw'), value: 'zh-tw' },
  { label: t('language.en'), value: 'en' },
  { label: t('language.ko'), value: 'ko' },
  { label: t('language.ja'), value: 'ja' },
])

const planActionOptions = computed(() => [
  { label: t('appearance.plan.shutdown'), value: 'shutdown' },
  { label: t('appearance.plan.sleep'), value: 'sleep' },
])

const themeStatus = computed(() => {
  const current = resolved.value === 'dark' ? t('theme.dark') : t('theme.light')
  if (themeMode.value === 'system') return `${t('theme.statusFollow')}${current}`
  return `${t('theme.statusFixed')}${current}`
})

async function refreshExtensions() {
  try {
    extensions.value = await listExtensions()
  } catch {
    /* 浏览器预览下没有 Tauri 命令 */
  }
}

async function registerExtension() {
  const id = extensionId.value.trim()
  if (!id) return

  try {
    await registerExtensionHost(browser.value, id)
    extensionId.value = ''
    await refreshExtensions()
  } catch (reason) {
    showToast(String(reason))
  }
}

async function removeExtension(browserName: string) {
  try {
    await unregisterExtensionHost(browserName)
    await refreshExtensions()
  } catch (reason) {
    showToast(String(reason))
  }
}

watch([launchAtStartup, confirmOnExit, notifyOnFinish, themeMode, planAction, keepAwake], persist)

/* 语言切换：立即加载对应语言包并写回配置 */
watch(language, (value) => {
  if (hydrating) return
  void setLanguage(value)
})

onMounted(async () => {
  try {
    const settings = await ensureSettings()
    launchAtStartup.value = settings.autostart
    confirmOnExit.value = settings.quitConfirm
    notifyOnFinish.value = settings.notifyOnFinish
    language.value = settings.language || 'system'
    planAction.value = settings.planAction === 'sleep' ? 'sleep' : 'shutdown'
    keepAwake.value = settings.keepAwake
    void refreshExtensions()
    if (settings.theme === 'light' || settings.theme === 'dark' || settings.theme === 'system') {
      themeMode.value = settings.theme
    }
  } catch {
    /* 浏览器预览下没有 Tauri 命令 */
  } finally {
    hydrating = false
  }
})
</script>

<style scoped>
.appearance {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding-bottom: 16px;
}

.card {
  padding: 20px;
  border-radius: 18px;
  background: var(--card);
}

.card__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.card__title {
  margin: 0;
  font-size: var(--fs-card);
  font-weight: 400;
  color: var(--text-muted);
}

.card__icon {
  color: var(--text-muted);
}

.card__note {
  margin: 8px 0 0;
  font-size: var(--fs-note);
  color: var(--text-muted);
}

.card__empty {
  margin: 14px 0 0;
  font-size: var(--fs-row);
  color: var(--text-muted);
}

.theme-title {
  margin: 12px 0 0;
  font-size: 22px;
  font-weight: 700;
  color: var(--text);
}

.theme-desc {
  margin: 8px 0 0;
  font-size: var(--fs-row);
  color: var(--text-muted);
}

.theme-switch {
  margin-top: 14px;
}

.theme-status {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 16px 0 0;
  font-size: var(--fs-row);
  color: var(--text-secondary);
}

.theme-status__dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--accent);
  box-shadow: 0 0 0 5px color-mix(in srgb, var(--accent) 12%, transparent);
}

.toggle-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  margin-top: 16px;
}

.toggle-row__title {
  margin: 0;
  font-size: var(--fs-title);
  font-weight: 700;
  color: var(--text);
}

.toggle-row__note {
  margin: 3px 0 0;
  font-size: var(--fs-note);
  color: var(--text-muted);
}

.inline-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  margin-top: 12px;
}

.extension-row {
  display: flex;
  gap: 8px;
  margin-top: 10px;
}

.extension-input {
  flex: 1;
  min-width: 0;
  height: 32px;
  padding: 0 11px;
  border: none;
  border-radius: 8px;
  background: var(--input);
  color: var(--text);
  font-size: var(--fs-row);
  outline: none;
}

.extension-input::placeholder {
  color: var(--text-muted);
}

.extension-add {
  width: 110px;
  height: 32px;
  border: none;
  border-radius: 8px;
  background: var(--accent);
  color: var(--on-accent);
  font-size: var(--fs-row);
  cursor: default;
}

.extension-add:hover:not(:disabled) {
  background: var(--accent-hover);
}

.extension-add:disabled {
  background: var(--input);
  color: var(--text-muted);
}

.extension-item {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 44px;
  margin-top: 8px;
  padding: 0 12px;
  border-radius: 9px;
  background: var(--input);
}

.extension-item__icon {
  color: var(--text-muted);
}

.extension-item__text {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
  gap: 2px;
}

.extension-item__browser {
  font-size: var(--fs-row);
  font-weight: 700;
  color: var(--text);
}

.extension-item__id {
  overflow: hidden;
  font-size: var(--fs-row);
  color: var(--text-muted);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.extension-item__remove {
  width: 88px;
  height: 28px;
  border: none;
  border-radius: 7px;
  background: var(--card);
  color: var(--text-secondary);
  font-size: var(--fs-row);
  cursor: default;
}

.extension-item__remove:hover {
  background: var(--card-hover);
  color: var(--text);
}
</style>
