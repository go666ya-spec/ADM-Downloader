<template>
  <article class="panel activity-panel">
    <h3 class="panel__title">{{ t('activity.title') }}</h3>
    <span class="activity-panel__dot" />

    <strong class="activity-panel__total">{{ total }}</strong>

    <div class="activity-panel__columns">
      <div v-for="item in columns" :key="item.label" class="activity-column">
        <strong class="activity-column__value">{{ item.value }}</strong>
        <span class="activity-column__label">{{ item.label }}</span>
      </div>
    </div>
  </article>
</template>

<script setup lang="ts">
import { computed } from 'vue'

import { t } from '@/i18n'

import type { TaskCounts } from '@/types/dashboard'

const props = defineProps<{ counts: TaskCounts }>()

const total = computed(() => props.counts.active + props.counts.queued)

const columns = computed(() => [
  { label: t('activity.downloading'), value: props.counts.active },
  { label: t('activity.queued'), value: props.counts.queued },
])
</script>

<style scoped>
.activity-panel {
  justify-content: space-between;
  height: 100%;
  min-height: 0;
  padding: 20px;
}

.activity-panel__dot {
  position: absolute;
  top: 22px;
  right: 14px;
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: color-mix(in srgb, var(--text-muted) 55%, transparent);
}

.activity-panel__total {
  /* 左对齐：首位数字与第一列数字首位对齐 */
  align-self: flex-start;
  flex: none;
  margin: 0;
  font-size: var(--fs-big);
  font-weight: 700;
  line-height: 1;
  color: var(--text);
  font-variant-numeric: tabular-nums;
}

.activity-panel__columns {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  flex: none;
  margin: 0;
  /* 首位数字「中心」对齐：40px 字号数字宽约 22px、22px 字号约 12px，
     两者中心差 (22 − 12) / 2 ≈ 5px，把两列整体右移这么多即可 */
  transform: translateX(5px);
}

.activity-column {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.activity-column__value {
  align-self: flex-start;
  font-size: 22px;
  font-weight: 700;
  line-height: 1;
  color: var(--text);
  font-variant-numeric: tabular-nums;
}

.activity-column__label {
  font-size: var(--fs-row);
  color: var(--text-muted);
}
</style>
