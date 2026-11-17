<template>
  <button
    type="button"
    class="checkbox"
    :class="{ 'is-checked': modelValue }"
    role="checkbox"
    :aria-checked="modelValue"
    @click.stop="toggle"
  >
    <AppIcon v-if="modelValue" name="check" :size="10" :stroke-width="1.8" />
  </button>
</template>

<script setup lang="ts">
import AppIcon from '@/components/AppIcon.vue'

const props = defineProps<{ modelValue: boolean }>()
const emit = defineEmits<{ 'update:modelValue': [value: boolean] }>()

function toggle() {
  emit('update:modelValue', !props.modelValue)
}
</script>

<style scoped>
.checkbox {
  display: grid;
  place-items: center;
  width: 16px;
  height: 16px;
  flex: none;
  padding: 0;
  border: none;
  border-radius: 4px;
  background: var(--input);
  color: var(--on-accent);
  cursor: default;
  transition: background-color 120ms ease;
}

.checkbox.is-checked {
  background: var(--accent);
}
</style>
