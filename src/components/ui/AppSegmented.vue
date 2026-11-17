<template>
  <div class="segmented" :style="{ width }">
    <button
      v-for="option in options"
      :key="option.value"
      class="segmented__item"
      :class="{ 'is-active': option.value === modelValue }"
      @click="emit('update:modelValue', option.value)"
    >
      {{ option.label }}
    </button>
  </div>
</template>

<script setup lang="ts">
export interface SegmentedOption {
  label: string
  value: string
}

withDefaults(
  defineProps<{ modelValue: string; options: SegmentedOption[]; width?: string }>(),
  { width: 'auto' },
)

const emit = defineEmits<{ 'update:modelValue': [value: string] }>()
</script>

<style scoped>
.segmented {
  display: flex;
  gap: 2px;
  padding: 3px;
  border-radius: 9px;
  background: var(--input);
}

.segmented__item {
  flex: 1;
  height: 26px;
  padding: 0 12px;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: var(--text-secondary);
  font-size: 12.5px;
  white-space: nowrap;
  cursor: default;
  transition: background-color 120ms ease, color 120ms ease;
}

.segmented__item:hover {
  color: var(--text);
}

.segmented__item.is-active {
  background: var(--control-selected);
  color: var(--text);
}
</style>
