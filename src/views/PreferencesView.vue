<template>
  <div class="preferences">
    <PageHeader :title="t('page.preferences.title')" :subtitle="t('page.preferences.sub')" />

    <div class=page-scroll>

    <section class="card">
      <header class="card__head">
        <h2 class="card__title">{{ t('prefs.save.title') }}</h2>
        <AppIcon name="folder" :size="15" class="card__icon" />
      </header>
      <p class="card__lead">{{ t('prefs.save.lead') }}</p>
      <div class="path-row">
        <AppLineInput
          v-model="savePath"
          class="path-input"
          :placeholder="t('prefs.save.lead')"
        />
        <button class="path-browse" @click="browse">
          <AppIcon name="folder" :size="14" />
          <span>{{ t('common.browse') }}</span>
        </button>
      </div>
      <p class="card__note">{{ t('prefs.save.note') }}</p>
    </section>

    <section class="card">
      <header class="card__head">
        <h2 class="card__title">{{ t('prefs.perf.title') }}</h2>
        <AppIcon name="rabbit" :size="15" class="card__icon" />
      </header>

      <div class="row row--baseline">
        <p class="row__summary">{{ presetSummary }}</p>
        <AppSelect v-model="performance" :options="performanceOptions" width="140px" />
      </div>

      <div class="metrics">
        <div v-for="metric in metrics" :key="metric.label" class="metric">
          <AppIcon :name="metric.icon" :size="13" class="metric__icon" />
          <strong class="metric__value">{{ metric.value }}</strong>
          <span class="metric__label">{{ metric.label }}</span>
        </div>
      </div>

      <div class="row row--baseline row--spaced">
        <div>
          <p class="row__title">{{ t('prefs.perf.parallel') }}</p>
          <p class="row__note">{{ t('prefs.perf.parallelNote') }}</p>
        </div>
        <AppSelect v-model="parallel" :options="parallelOptions" width="96px" />
      </div>
    </section>

    <section class="card">
      <header class="card__head">
        <h2 class="card__title">{{ t('prefs.limit.title') }}</h2>
      </header>

      <div class="limit-row">
        <span class="limit-row__label">{{ t('prefs.limit.download') }}</span>
        <div class="limit-field">
          <input v-model="downloadLimit" class="limit-input" inputmode="numeric" :placeholder="t('common.unlimited')" />
          <span class="limit-unit">KiB/s</span>
        </div>
      </div>
    </section>

    <section class="card">
      <header class="card__head">
        <h2 class="card__title">{{ t('prefs.proxy.title') }}</h2>
      </header>
      <div class="row row--baseline row--spaced">
        <p class="row__title">{{ t('prefs.proxy.mode') }}</p>
        <AppSegmented v-model="proxyMode" :options="proxyOptions" width="300px" />
      </div>
    </section>

    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from 'vue'

import AppIcon from '@/components/AppIcon.vue'
import PageHeader from '@/components/PageHeader.vue'
import AppLineInput from '@/components/ui/AppLineInput.vue'
import AppSegmented from '@/components/ui/AppSegmented.vue'
import AppSelect from '@/components/ui/AppSelect.vue'
import { pickFolder } from '@/bridge/settings'
import { useNetworkSettings } from '@/composables/useNetworkSettings'
import { t } from '@/i18n'
import type { IconName } from '@/components/icons'
import { ensureSettings, patchSettings } from '@/stores/settings'

const savePath = ref('')
const performance = ref('balanced')
const parallel = ref('1')
const downloadLimit = ref('')
/* 代理模式与仪表盘的「下载线路」共用同一份状态 */
const { proxyMode } = useNetworkSettings()

const performanceOptions = computed(() => [
  { label: t('prefs.perf.balanced'), value: 'balanced' },
  { label: t('prefs.perf.fast'), value: 'fast' },
  { label: t('prefs.perf.turbo'), value: 'turbo' },
])

const parallelOptions = Array.from({ length: 10 }, (_, index) => ({
  label: String(index + 1),
  value: String(index + 1),
}))

const proxyOptions = computed(() => [
  { label: t('prefs.proxy.system'), value: 'system' },
  { label: t('prefs.proxy.direct'), value: 'direct' },
])

interface Preset {
  summaryKey: string
  connections: number
  minSegmentMib: number
  cacheMib: number
}

const PRESETS: Record<string, Preset> = {
  balanced: {
    summaryKey: 'prefs.perf.presetBalanced',
    connections: 16,
    minSegmentMib: 4,
    cacheMib: 8,
  },
  fast: {
    summaryKey: 'prefs.perf.presetFast',
    connections: 32,
    minSegmentMib: 2,
    cacheMib: 8,
  },
  turbo: {
    summaryKey: 'prefs.perf.presetTurbo',
    connections: 64,
    minSegmentMib: 1,
    cacheMib: 8,
  },
}

/* 实际生效的性能参数（后端也按这份存） */
const perf = reactive({ connections: 16, minSegmentMib: 4, cacheMib: 8 })

const presetSummary = computed(() => {
  const preset = PRESETS[performance.value]
  return preset ? t(preset.summaryKey) : ''
})

const metrics = computed<{ icon: IconName; value: string; label: string }[]>(() => [
  { icon: 'globe', value: String(perf.connections), label: t('prefs.perf.connections') },
  { icon: 'file', value: `${perf.minSegmentMib} MiB`, label: t('prefs.perf.minSegment') },
  { icon: 'clock', value: `${perf.cacheMib} MiB`, label: t('prefs.perf.cache') },
])

let hydrating = true

function persistSettings() {
  if (hydrating) return
  patchSettings({
    saveDir: savePath.value.trim(),
    connections: perf.connections,
    minSegmentMib: perf.minSegmentMib,
    cacheMib: perf.cacheMib,
    parallel: Number(parallel.value) || 1,
    downloadLimitKib: Number(downloadLimit.value) || 0,
    proxyMode: proxyMode.value === 'direct' ? 'direct' : 'system',
    speedMode: performance.value,
  })
}

async function browse() {
  try {
    const folder = await pickFolder(t('prefs.save.title'))
    if (folder) {
      savePath.value = folder
      persistSettings()
    }
  } catch {
    /* 取消选择或无权限时忽略 */
  }
}

/* 切换档位：把预设值写进实际参数，再保存 */
watch(performance, (name) => {
  const preset = PRESETS[name]
  if (!preset) return
  perf.connections = preset.connections
  perf.minSegmentMib = preset.minSegmentMib
  perf.cacheMib = preset.cacheMib
  persistSettings()
})

watch([savePath, parallel, downloadLimit, proxyMode], persistSettings)

onMounted(async () => {
  try {
    const settings = await ensureSettings()
    savePath.value = settings.saveDir
    performance.value = settings.speedMode || 'balanced'
    parallel.value = String(settings.parallel || 1)
    downloadLimit.value = settings.downloadLimitKib ? String(settings.downloadLimitKib) : ''
    proxyMode.value = settings.proxyMode === 'direct' ? 'direct' : 'system'
    perf.connections = settings.connections
    perf.minSegmentMib = settings.minSegmentMib
    perf.cacheMib = settings.cacheMib
  } catch {
    savePath.value = ''
  } finally {
    hydrating = false
  }
})
</script>

<style scoped>
.preferences {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding-bottom: 16px;
}

.card {
  padding: 20px;
  border-radius: 18px;
  background: var(--card);
}

.card__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.card__title {
  margin: 0;
  font-size: var(--fs-card);
  font-weight: 400;
  color: var(--text-muted);
}

.card__icon {
  color: var(--text-muted);
}

.card__lead {
  margin: 12px 0 0;
  font-size: var(--fs-title);
  font-weight: 700;
  color: var(--text);
}

.path-row {
  display: flex;
  gap: 8px;
  margin-top: 10px;
}

.path-input {
  flex: 1;
  min-width: 0;
  height: 32px;
  padding: 0 11px;
  border: none;
  border-radius: 9px;
  background: var(--input);
  color: var(--text);
  font-size: var(--fs-row);
  outline: none;
}

.path-browse {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  width: 92px;
  height: 32px;
  justify-content: center;
  border: none;
  border-radius: 9px;
  background: var(--input);
  color: var(--text-secondary);
  font-size: var(--fs-row);
  cursor: default;
}

.path-browse:hover {
  background: var(--card-hover);
  color: var(--text);
}

.card__note {
  margin: 10px 0 0;
  font-size: var(--fs-note);
  color: var(--text-muted);
}

.row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.row--baseline {
  align-items: baseline;
}

.row--spaced {
  margin-top: 12px;
}

.row__summary {
  margin: 12px 0 0;
  font-size: var(--fs-row);
  color: var(--text-muted);
}

.row__title {
  margin: 0;
  font-size: var(--fs-title);
  font-weight: 700;
  color: var(--text);
}

.row__note {
  margin: 2px 0 0;
  font-size: var(--fs-note);
  color: var(--text-muted);
}

.metrics {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 8px;
  margin-top: 10px;
  padding: 10px 16px 14px;
  border-radius: 12px;
  background: var(--input);
}

.metric {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding-top: 4px;
}

.metric__icon {
  color: var(--text-muted);
}

.metric__value {
  font-size: 14px;
  font-weight: 700;
  color: var(--text);
}

.metric__label {
  font-size: var(--fs-note);
  color: var(--text-muted);
}

.limit-row {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-top: 14px;
}

.limit-row__label {
  width: 40px;
  font-size: var(--fs-row);
  font-weight: 700;
  color: var(--text);
}

.limit-field {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 220px;
  height: 30px;
  padding: 0 12px 0 10px;
  border-radius: 8px;
  background: var(--input);
}

.limit-input {
  flex: 1;
  min-width: 0;
  border: none;
  background: transparent;
  color: var(--text);
  font-size: var(--fs-row);
  outline: none;
}

.limit-input::placeholder {
  color: var(--text-muted);
}

.limit-unit {
  font-size: var(--fs-row);
  color: var(--text-muted);
}
</style>
