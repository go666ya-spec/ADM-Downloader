<template>
  <article class="panel system-panel">
    <h3 class="panel__title">{{ t('dashboard.system.title') }}</h3>
    <AppIcon name="cpu" :size="16" class="panel__corner" />

    <!-- 每一行都是独立弹性项：首尾贴住固定内边距，余量只分给行间空白 -->
    <span class="system-panel__caption">CPU</span>
    <p class="system-panel__model" :title="info.cpuModel">{{ info.cpuModel || t('cpu.unknown') }}</p>

    <div class="system-panel__row">
      <span class="system-panel__caption">{{ t('dashboard.memory') }}</span>
      <span class="system-panel__value">{{ info.memory || t('memory.unknown') }}</span>
    </div>

    <div class="system-panel__row">
      <span class="system-panel__caption">{{ t('dashboard.route') }}</span>
      <span class="system-panel__value" :class="routeClass">{{ routeText }}</span>
    </div>
  </article>
</template>

<script setup lang="ts">
import { computed } from 'vue'

import AppIcon from '@/components/AppIcon.vue'
import { t } from '@/i18n'
import type { RouteInfo, SystemInfo } from '@/types/dashboard'

const props = defineProps<{ info: SystemInfo; route: RouteInfo }>()

const routeText = computed(() => {
  const { routeKind, routeDetail, tun } = props.route

  if (routeKind === 'system-proxy') {
    if (routeDetail === 'PAC') return t('route.pac')
    return routeDetail || t('prefs.proxy.system')
  }

  /* 我们按直连发请求；若系统被 TUN 接管，追加标记说明流量在底层被改道 */
  return tun ? t('route.tun') : t('route.direct')
})

const routeClass = computed(() => {
  const { routeKind, tun } = props.route
  if (routeKind === 'direct' && !tun) return 'is-direct'
  return 'is-proxy'
})
</script>

<style scoped>
.system-panel {
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  height: 100%;
  min-height: 0;
  /* 底部 18 与限速卡一致，保证最后一行基线相同 */
  padding: 16px 20px 18px;
}

.system-panel__caption {
  flex: none;
  width: 52px;
  font-size: var(--fs-row);
  line-height: 1.2;
  color: var(--text-muted);
}

.system-panel__model {
  flex: none;
  margin: 0;
  overflow: hidden;
  font-size: 13px;
  font-weight: 700;
  line-height: 1.3;
  color: var(--text);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.system-panel__row {
  display: flex;
  flex: none;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
}

.system-panel__row:last-child .system-panel__caption,
.system-panel__row:last-child .system-panel__value {
  line-height: 1;
}

.system-panel__value {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  font-size: var(--fs-row);
  line-height: 1.2;
  color: var(--text-secondary);
  font-variant-numeric: tabular-nums;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.system-panel__value.is-direct {
  color: var(--text-secondary);
}

/* 走代理或 TUN 时用强调色，一眼能看出线路不同 */
.system-panel__value.is-proxy {
  color: var(--accent);
}
</style>
