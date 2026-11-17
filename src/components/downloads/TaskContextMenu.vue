<template>
  <div class="context-menu" :style="positionStyle" @click.stop>
    <button
      v-for="item in items"
      :key="item.id"
      class="context-menu__item"
      :class="{ 'is-danger': item.danger }"
      @click="emit('select', item.id)"
    >
      {{ item.label }}
    </button>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'

import { t } from '@/i18n'

export interface ContextMenuItem {
  id: string
  label: string
  danger?: boolean
}

const props = defineProps<{ x: number; y: number }>()
const emit = defineEmits<{ select: [id: string] }>()

const items: ContextMenuItem[] = [
  { id: 'open-file', get label() { return t('menu.openFile') } },
  { id: 'open-folder', get label() { return t('menu.openFolder') } },
  { id: 'redownload', get label() { return t('menu.redownload') } },
  { id: 'copy-url', get label() { return t('menu.copyUrl') } },
  { id: 'copy-path', get label() { return t('menu.copyPath') } },
  { id: 'remove', get label() { return t('menu.remove') }, danger: true },
]

/** 贴边时向内收，避免菜单超出视口 */
const positionStyle = computed(() => {
  const width = 180
  const height = 188
  const x = Math.min(props.x, window.innerWidth - width - 8)
  const y = Math.min(props.y, window.innerHeight - height - 8)
  return { left: `${Math.max(x, 8)}px`, top: `${Math.max(y, 8)}px` }
})
</script>

<style scoped>
.context-menu {
  position: fixed;
  z-index: 40;
  width: 180px;
  padding: 4px;
  border-radius: 10px;
  background: var(--elevated);
  box-shadow: 0 8px 26px var(--shadow);
}

.context-menu__item {
  display: flex;
  align-items: center;
  width: 100%;
  height: 28px;
  margin-bottom: 2px;
  padding: 0 10px;
  border: none;
  border-radius: 7px;
  background: transparent;
  font-size: 11px;
  color: var(--text-secondary);
  text-align: left;
  cursor: default;
}

.context-menu__item:hover {
  background: var(--card-hover);
  color: var(--text);
}

.context-menu__item.is-danger {
  color: var(--danger);
}
</style>
