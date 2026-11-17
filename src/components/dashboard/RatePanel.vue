<template>
  <article class="panel rate-panel">
    <h3 class="panel__title">{{ label }}</h3>
    <div class="rate-panel__value">
      <strong>{{ formatted.value }}</strong>
      <span>{{ formatted.unit }}</span>
    </div>
  </article>
</template>

<script setup lang="ts">
import { computed } from 'vue'

const props = defineProps<{ label: string; bytesPerSecond: number }>()

const UNITS = ['B/s', 'KB/s', 'MB/s', 'GB/s']

const formatted = computed(() => {
  let value = Math.max(props.bytesPerSecond, 0)
  let index = 0
  while (value >= 1024 && index < UNITS.length - 1) {
    value /= 1024
    index += 1
  }
  return { value: value.toFixed(1), unit: UNITS[index] }
})
</script>

<style scoped>
.rate-panel {
  position: relative;
  justify-content: space-between;
  height: 100%;
  min-height: 0;
  padding: 20px;
  border-radius: 18px;
  background: var(--card);
}

.rate-panel__value {
  display: flex;
  align-items: baseline;
  gap: 5px;
  margin: 0;
}

.rate-panel__value strong {
  font-size: 32px;
  font-weight: 700;
  line-height: 1;
  color: var(--text);
}

.rate-panel__value span {
  font-size: 12.5px;
  color: var(--text-muted);
}
</style>
