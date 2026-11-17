<template>
  <article class="panel plan-panel">
    <h3 class="panel__title">{{ t('plan.title') }}</h3>
    <AppIcon name="clock" :size="16" class="panel__corner" />

    <p class="plan-panel__state" :class="plan.active ? 'is-active' : 'is-idle'">
      {{ plan.active ? t('plan.active') : t('plan.off') }}
    </p>

    <p v-if="!plan.active" class="plan-panel__line">
      {{ t('plan.note') }}
    </p>

    <p v-if="!plan.canStart" class="plan-panel__line is-warn">
      {{ t('plan.blocked') }}
    </p>

    <p v-if="plan.active" class="plan-panel__line is-ok">
      {{ t('plan.waiting') }}
    </p>

    <button
      class="plan-panel__toggle"
      :disabled="!plan.active && !plan.canStart"
      @click="emit('toggle')"
    >
      {{ plan.active ? t('plan.stop') : t('plan.start') }}
    </button>
  </article>
</template>

<script setup lang="ts">
import AppIcon from '@/components/AppIcon.vue'
import { t } from '@/i18n'
import type { PlanSnapshot } from '@/types/dashboard'

defineProps<{ plan: PlanSnapshot }>()
const emit = defineEmits<{ toggle: [] }>()
</script>

<style scoped>
.plan-panel {
  justify-content: space-between;
  height: 100%;
  min-height: 0;
  padding: 20px;
}

.plan-panel__state {
  flex: none;
  margin: 0;
  font-size: 13.5px;
  font-weight: 700;
  line-height: 1.2;
}

.plan-panel__state.is-active {
  color: var(--success);
}

.plan-panel__state.is-idle {
  color: var(--text-muted);
}

.plan-panel__line {
  flex: none;
  margin: 0;
  overflow: hidden;
  font-size: var(--fs-row);
  line-height: 1.4;
  color: var(--text-muted);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.plan-panel__line.is-ok {
  color: var(--success);
}

.plan-panel__line.is-warn {
  color: var(--warning);
}

.plan-panel__toggle {
  flex: none;
  width: 132px;
  max-width: 100%;
  height: 32px;
  margin: 0;
  border: none;
  border-radius: 8px;
  background: var(--accent);
  color: var(--on-accent);
  font-size: 12px;
  cursor: default;
  transition: background-color 120ms ease;
}

.plan-panel__toggle:hover:not(:disabled) {
  background: var(--accent-hover);
}

.plan-panel__toggle:disabled {
  background: var(--input);
  color: var(--text-muted);
}
</style>
