<template>
  <div class="app-shell">
    <TitleBar title="ADM 下载器" />

    <div class="app-shell__body">
      <AppSidebar />

      <main class="app-shell__content">
        <router-view v-slot="{ Component }">
          <transition name="page-fade" mode="out-in">
            <component :is="Component" />
          </transition>
        </router-view>
      </main>
    </div>

    <AppToast />

    <!-- 退出确认：只在「有任务在下载」且用户从托盘选择退出时出现 -->
    <div v-if="quitConfirmOpen" class="quit-mask" @click.self="quitConfirmOpen = false">
      <div class="quit-card">
        <p class="quit-card__title">{{ t('quit.title') }}</p>
        <p class="quit-card__note">{{ t('quit.note') }}</p>
        <div class="quit-card__actions">
          <button class="quit-btn" @click="quitConfirmOpen = false">{{ t('common.cancel') }}</button>
          <button class="quit-btn quit-btn--primary" @click="confirmQuit">
            {{ t('common.confirm') }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import AppSidebar from '@/layouts/AppSidebar.vue'
import TitleBar from '@/layouts/TitleBar.vue'
import AppToast from '@/components/ui/AppToast.vue'
import { storageNotice } from '@/bridge/tasks'
import { showToast } from '@/composables/useToast'
import { initLocale, t } from '@/i18n'
import { useTheme } from '@/composables/useTheme'
import { taskStore } from '@/stores/tasks'
import { ensureSettings, waitForSettingsSave } from '@/stores/settings'

const { mode: themeMode } = useTheme()
const quitConfirmOpen = ref(false)

let unlistenQuit: UnlistenFn | undefined
let unlistenStorage: UnlistenFn | undefined

/* 主题以配置文件为准，localStorage 只用于跨窗口即时同步 */
async function applyStoredTheme() {
  try {
    const settings = await ensureSettings()
    if (settings.theme === 'light' || settings.theme === 'dark' || settings.theme === 'system') {
      themeMode.value = settings.theme
    }
  } catch {
    /* 浏览器预览下没有 Tauri 命令 */
  }
}

/* 托盘「退出」：后端已经判断过要不要确认，这里只负责弹框或直接退出 */
async function onQuitRequested(payload: boolean) {
  if (!payload) {
    await exitApp()
    return
  }
  quitConfirmOpen.value = true
}

async function exitApp() {
  try {
    await waitForSettingsSave()
    await invoke('app_exit')
  } catch (reason) {
    showToast(`退出前设置保存失败：${String(reason)}`)
  }
}

async function confirmQuit() {
  quitConfirmOpen.value = false
  await exitApp()
}

/* 仅开发模式：模拟一次浏览器接管，方便调试二级窗 */
async function onDevShortcut(event: KeyboardEvent) {
  if (!event.ctrlKey || !event.shiftKey || event.key.toLowerCase() !== 't') return
  try {
    await invoke('takeover_begin', {
      url: 'https://releases.ubuntu.com/24.04.1/SHA256SUMS',
      fileName: null,
      saveDir: null,
    })
  } catch {
    /* 忽略 */
  }
}

/*
 * 任务数据在应用壳里订阅一次：
 * 无论当前停在哪个页面，下载进度事件都会进仓库，切页不用重新拉取。
 */
let dispose: (() => void) | undefined

onMounted(async () => {
  await initLocale()
  await applyStoredTheme()
  dispose = await taskStore.attach()
  await taskStore.load()

  try {
    const notice = await storageNotice()
    if (notice) showToast(notice)
  } catch {
    /* 浏览器预览下没有命令 */
  }

  try {
    unlistenQuit = await listen<boolean>('app:quit-requested', (event) =>
      onQuitRequested(event.payload),
    )
    unlistenStorage = await listen<string>('storage:error', (event) =>
      showToast(event.payload),
    )
  } catch {
    /* 浏览器预览下没有事件系统 */
  }

  /* 开发期测试接管弹窗：Ctrl+Shift+T 送一条示例请求 */
  if (import.meta.env.DEV) {
    window.addEventListener('keydown', onDevShortcut)
  }
})

onBeforeUnmount(() => {
  dispose?.()
  unlistenQuit?.()
  unlistenStorage?.()
  window.removeEventListener('keydown', onDevShortcut)
})
</script>

<style scoped>
.app-shell {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg);
  overflow: hidden;
}

.app-shell__body {
  display: flex;
  flex: 1;
  min-height: 0;
}

.app-shell__content {
  flex: 1;
  min-width: 0;
  padding: 0 var(--content-margin-right) 12px var(--content-margin-left);
  overflow: hidden;
}

.page-fade-enter-active,
.page-fade-leave-active {
  transition: opacity 120ms ease;
}

.page-fade-enter-from,
.page-fade-leave-to {
  opacity: 0;
}

/* 退出确认 */
.quit-mask {
  position: fixed;
  inset: 0;
  z-index: 80;
  display: grid;
  place-items: center;
  background: rgb(0 0 0 / 28%);
}

.quit-card {
  width: 340px;
  padding: 20px 22px 16px;
  border-radius: 16px;
  background: var(--card);
  box-shadow: 0 18px 48px var(--shadow);
}

.quit-card__title {
  margin: 0;
  font-size: var(--fs-title);
  font-weight: 700;
  color: var(--text);
}

.quit-card__note {
  margin: 8px 0 0;
  font-size: var(--fs-row);
  line-height: 1.6;
  color: var(--text-muted);
}

.quit-card__actions {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  margin-top: 18px;
}

.quit-btn {
  height: 30px;
  padding: 0 14px;
  border: none;
  border-radius: 8px;
  background: var(--input);
  color: var(--text-secondary);
  font-family: inherit;
  font-size: var(--fs-row);
  cursor: default;
}

.quit-btn:hover {
  color: var(--text);
}

.quit-btn--primary {
  background: var(--accent);
  color: var(--on-accent);
}

.quit-btn--primary:hover {
  background: var(--accent-hover);
  color: var(--on-accent);
}
</style>
