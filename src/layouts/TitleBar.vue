<template>
  <header class="title-bar" data-tauri-drag-region>
    <div class="title-bar__brand" data-tauri-drag-region>
      <span class="title-bar__dot" />
      <span class="title-bar__name">{{ title }}</span>
    </div>

    <div class="title-bar__drag-zone" data-tauri-drag-region />

    <div class="title-bar__controls">
      <button class="window-button" :title="t('window.minimize')" @click="minimize">
        <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
          <path d="M3 8h10" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
        </svg>
      </button>

      <button
        class="window-button"
        :title="maximized ? t('window.restore') : t('window.maximize')"
        @click="toggleMaximize"
      >
        <svg v-if="!maximized" viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
          <rect
            x="3.5"
            y="3.5"
            width="9"
            height="9"
            rx="1.6"
            fill="none"
            stroke="currentColor"
            stroke-width="1.4"
          />
        </svg>
        <svg v-else viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
          <rect
            x="3"
            y="5.5"
            width="7.5"
            height="7.5"
            rx="1.4"
            fill="none"
            stroke="currentColor"
            stroke-width="1.3"
          />
          <path
            d="M5.6 5.4V4.6a1.4 1.4 0 0 1 1.4-1.4h4.6a1.4 1.4 0 0 1 1.4 1.4v4.6a1.4 1.4 0 0 1-1.4 1.4h-.8"
            fill="none"
            stroke="currentColor"
            stroke-width="1.3"
            stroke-linecap="round"
          />
        </svg>
      </button>

      <button class="window-button window-button--close" :title="t('window.close')" @click="close">
        <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
          <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
        </svg>
      </button>
    </div>
  </header>
</template>

<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'

import { t } from '@/i18n'

defineProps<{ title: string }>()

const appWindow = getCurrentWindow()
const maximized = ref(false)
let unlisten: (() => void) | undefined

async function minimize() {
  try {
    await appWindow.minimize()
  } catch {
    /* 浏览器预览下没有窗口 API */
  }
}

async function toggleMaximize() {
  try {
    await appWindow.toggleMaximize()
    maximized.value = await appWindow.isMaximized()
  } catch {
    /* 浏览器预览下没有窗口 API */
  }
}

async function close() {
  try {
    await appWindow.close()
  } catch {
    /* 浏览器预览下没有窗口 API */
  }
}

onMounted(async () => {
  try {
    maximized.value = await appWindow.isMaximized()
    /* 拖拽缩放、双击标题栏等外部改变大小的情况也要同步图标 */
    unlisten = await appWindow.onResized(async () => {
      maximized.value = await appWindow.isMaximized()
    })
  } catch {
    /* 浏览器预览下没有窗口 API */
  }
})

onUnmounted(() => {
  unlisten?.()
})
</script>

<style scoped>
.title-bar {
  display: flex;
  align-items: center;
  height: var(--title-bar-height);
  flex: 0 0 var(--title-bar-height);
  padding-left: 14px;
  -webkit-app-region: drag;
}

.title-bar__brand {
  display: flex;
  align-items: center;
  gap: 8px;
}

.title-bar__dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--accent);
  box-shadow: 0 0 8px var(--accent);
}

.title-bar__name {
  font-size: 12px;
  font-weight: 700;
  color: var(--text-secondary);
  letter-spacing: 0.02em;
}

.title-bar__drag-zone {
  flex: 1;
  height: 100%;
}

.title-bar__controls {
  display: flex;
  align-items: center;
  gap: 2px;
  height: 100%;
  padding-right: 6px;
  -webkit-app-region: no-drag;
}

.window-button {
  display: grid;
  place-items: center;
  width: 30px;
  height: 26px;
  border: none;
  border-radius: 8px;
  background: transparent;
  color: var(--text-secondary);
  cursor: default;
  transition: background-color 120ms ease, color 120ms ease;
}

html.suppress-hover .window-button:hover {
  background: transparent;
  color: var(--text-secondary);
}

html.suppress-hover .window-button--close:hover {
  background: transparent;
  color: var(--text-secondary);
}

.window-button:hover {
  background: var(--elevated);
  color: var(--text);
}

.window-button--close:hover {
  background: #e81123;
  color: #fff;
}
</style>
