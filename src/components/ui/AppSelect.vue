<template>
  <div ref="root" class="select" :style="{ width }">
    <button class="select__trigger" @click.stop="open = !open">
      <span class="select__label">{{ currentLabel }}</span>
      <AppIcon name="chevron-down" :size="10" :stroke-width="1.6" />
    </button>

    <ul v-if="open" class="select__menu">
      <li
        v-for="option in options"
        :key="option.value"
        class="select__item"
        :class="{ 'is-active': option.value === modelValue }"
        @click.stop="pick(option.value)"
      >
        {{ option.label }}
      </li>
    </ul>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'

import AppIcon from '@/components/AppIcon.vue'

const props = withDefaults(
  defineProps<{
    modelValue: string
    options: { label: string; value: string }[]
    width?: string
  }>(),
  { width: 'auto' },
)

const emit = defineEmits<{ 'update:modelValue': [value: string] }>()

const open = ref(false)
const root = ref<HTMLElement | null>(null)

const currentLabel = computed(
  () => props.options.find((option) => option.value === props.modelValue)?.label ?? '',
)

function pick(value: string) {
  emit('update:modelValue', value)
  open.value = false
}

function onDocumentClick(event: MouseEvent) {
  if (!root.value?.contains(event.target as Node)) open.value = false
}

onMounted(() => document.addEventListener('click', onDocumentClick))
onBeforeUnmount(() => document.removeEventListener('click', onDocumentClick))
</script>

<style scoped>
.select {
  position: relative;
}

.select__trigger {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  width: 100%;
  height: 32px;
  padding: 0 10px;
  border: none;
  border-radius: 8px;
  background: var(--input);
  color: var(--text);
  font-size: var(--fs-row);
  cursor: default;
}

.select__trigger:hover {
  background: var(--card-hover);
}

.select__label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.select__menu {
  position: absolute;
  z-index: 20;
  top: calc(100% + 4px);
  left: 0;
  width: 100%;
  margin: 0;
  padding: 4px;
  border-radius: 10px;
  background: var(--elevated);
  box-shadow: 0 8px 26px var(--shadow);
  list-style: none;
}

.select__item {
  display: flex;
  align-items: center;
  height: 30px;
  padding: 0 10px;
  border-radius: 7px;
  font-size: var(--fs-row);
  color: var(--text-secondary);
  cursor: default;
}

.select__item:hover {
  background: var(--card-hover);
  color: var(--text);
}

.select__item.is-active {
  background: var(--elevated);
  color: var(--text);
}
</style>
