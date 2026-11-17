import { ref } from 'vue'

import { ensureSettings, patchSettings } from '@/stores/settings'

export type BandwidthMode = 'smart' | 'limited'

const MODES: BandwidthMode[] = ['smart', 'limited']

/** 带宽模式：仪表盘限速卡与偏好页共用同一份状态 */
const mode = ref<BandwidthMode>('smart')
let loaded = false

export function useBandwidthMode() {
  async function load() {
    if (loaded) return
    try {
      const settings = await ensureSettings()
      if (MODES.includes(settings.bandwidthMode as BandwidthMode)) {
        mode.value = settings.bandwidthMode as BandwidthMode
      }
      loaded = true
    } catch {
      /* 浏览器预览下没有 Tauri 命令 */
    }
  }

  /** 切换模式：立即写回配置，后端据此重算实际限速 */
  async function apply(next: BandwidthMode) {
    mode.value = next
    try {
      await ensureSettings()
      patchSettings({ bandwidthMode: next })
    } catch {
      /* 浏览器预览下没有 Tauri 命令 */
    }
  }

  return { mode, load, apply }
}
