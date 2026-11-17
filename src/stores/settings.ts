import { reactive } from 'vue'

import { settingsSave, settingsSnapshot, type SettingsDto } from '@/bridge/settings'
import { showToast } from '@/composables/useToast'

const state = reactive<{
  value: SettingsDto | null
  error: string | null
}>({
  value: null,
  error: null,
})

let loadTask: Promise<SettingsDto> | null = null
let saveTimer: number | undefined
let saveTask: Promise<void> | null = null
let dirty = false
let waiters: Array<{ resolve: () => void; reject: (reason: unknown) => void }> = []

function notifyWaiters(error?: unknown) {
  if (!error && (dirty || saveTimer !== undefined || saveTask)) return
  const current = waiters.splice(0)
  current.forEach((waiter) => (error ? waiter.reject(error) : waiter.resolve()))
}

async function runSaveLoop() {
  if (saveTask) return saveTask

  saveTask = (async () => {
    while (dirty && state.value) {
      dirty = false
      const payload = { ...state.value }
      try {
        await settingsSave(payload)
        state.error = null
      } catch (reason) {
        const message = String(reason)
        state.error = message
        dirty = false
        showToast(`设置保存失败：${message}`)
        throw reason
      }
    }
  })().finally(() => {
    saveTask = null
  })

  return saveTask
}

function scheduleSave() {
  dirty = true
  window.clearTimeout(saveTimer)
  saveTimer = window.setTimeout(() => {
    saveTimer = undefined
    void runSaveLoop()
      .then(() => notifyWaiters())
      .catch((error) => notifyWaiters(error))
  }, 250)
}

export async function ensureSettings() {
  if (state.value) return state.value
  if (loadTask) return loadTask

  loadTask = settingsSnapshot()
    .then((settings) => {
      state.value = settings
      state.error = null
      return settings
    })
    .finally(() => {
      loadTask = null
    })

  return loadTask
}

export function currentSettings() {
  return state.value
}

export function patchSettings(patch: Partial<SettingsDto>) {
  if (!state.value) return
  Object.assign(state.value, patch)
  scheduleSave()
}

/** 正常退出时等待当前 250ms 防抖、在途写入和后续补写全部完成。 */
export async function waitForSettingsSave() {
  if (!dirty && saveTimer === undefined && !saveTask) return
  await new Promise<void>((resolve, reject) => {
    waiters.push({ resolve, reject })
  })
}

