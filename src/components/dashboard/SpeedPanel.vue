<template>
  <article class="panel speed-panel">
    <h3 class="panel__title">{{ t('dashboard.speed.title') }}</h3>
    <AppIcon name="settings" :size="17" class="panel__corner" />

    <div class="speed-panel__options">
      <button
        v-for="option in options"
        :key="option.mode"
        class="speed-mode"
        :class="{ 'speed-mode--active': option.mode === modelValue }"
        :title="option.label"
        @click="emit('update:modelValue', option.mode)"
      >
        <AppIcon
          :name="option.icon"
          :size="option.mode === modelValue ? 24 : 23"
          :stroke-width="1.6"
        />
      </button>
    </div>

    <span class="speed-panel__status">{{ currentLabel }}</span>
  </article>
</template>

<script setup lang="ts">
import { computed } from 'vue'

import AppIcon from '@/components/AppIcon.vue'
import { t } from '@/i18n'
import type { IconName } from '@/components/icons'
import type { SpeedMode } from '@/types/dashboard'

const props = defineProps<{ modelValue: SpeedMode }>()
const emit = defineEmits<{ 'update:modelValue': [value: SpeedMode] }>()

const options: { mode: SpeedMode; icon: IconName; label: string }[] = [
    { mode: 'smart', icon: 'squirrel', get label() { return t('speed.label.smart') } },
    { mode: 'limited', icon: 'turtle', get label() { return t('speed.label.limit') } },
]

const currentLabel = computed(
  () => options.find((item) => item.mode === props.modelValue)?.label ?? '',
)
</script>

<style scoped>
.speed-panel {
  justify-content: space-between;
  height: 100%;
  min-height: 0;
  /* 底部内边距 18，与系统卡的「出站 IP」行同一条水平线 */
  padding: 20px 20px 18px;
}

.speed-panel__options {
  display: flex;
  align-items: center;
  justify-content: space-between;
  /* 两个按钮固定 40px；行宽变化只改变它们之间的空白，不挤压按钮本身。 */
  gap: 0;
  margin: 0;
}

.speed-mode {
  display: grid;
  place-items: center;
  flex: 0 0 40px;
  width: 40px;
  height: 40px;
  overflow: hidden;
  border: none;
  border-radius: 11px;
  background: transparent;
  color: var(--text-secondary);
  cursor: default;
  transition: background-color 120ms ease, color 120ms ease;
}

.speed-mode:hover {
  background: var(--card-hover);
  color: var(--text);
}

.speed-mode--active {
  background: var(--control-selected);
  color: var(--text);
}

.speed-panel__status {
  flex: none;
  margin: 0;
  font-size: 12px;
  line-height: 1;
  color: var(--text-secondary);
}
</style>
