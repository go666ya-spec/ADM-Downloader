<template>
  <button
    type="button"
    class="switch"
    :class="{ 'is-on': modelValue }"
    role="switch"
    :aria-checked="modelValue"
    @click.stop="emit('update:modelValue', !modelValue)"
  >
    <span class="switch__knob" />
  </button>
</template>

<script setup lang="ts">
defineProps<{ modelValue: boolean }>()
const emit = defineEmits<{ 'update:modelValue': [value: boolean] }>()
</script>

<style scoped>
.switch {
  display: flex;
  align-items: center;
  width: 40px;
  height: 22px;
  flex: none;
  padding: 2px;
  border: none;
  border-radius: 11px;
  background: var(--input);
  cursor: default;
  transition: background-color 140ms ease;
}

.switch.is-on {
  background: var(--accent);
}

.switch__knob {
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: #fff;
  transition: transform 140ms ease;
}

.switch.is-on .switch__knob {
  transform: translateX(18px);
}
</style>
