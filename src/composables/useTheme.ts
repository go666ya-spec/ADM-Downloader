import { computed, ref, watch } from 'vue'

export type ThemeMode = 'system' | 'light' | 'dark'

const STORAGE_KEY = 'adm.theme'

function readStoredMode(): ThemeMode {
  const stored = window.localStorage.getItem(STORAGE_KEY)
  return stored === 'light' || stored === 'dark' || stored === 'system' ? stored : 'system'
}

/**
 * 主题状态放在模块作用域：
 * - 同一窗口内切页不会重置
 * - 写进 localStorage，供另一个窗口（接管窗）读取，实现跨窗口同步
 */
const mode = ref<ThemeMode>(readStoredMode())

const media = window.matchMedia('(prefers-color-scheme: dark)')

const resolved = computed<ThemeMode>(() =>
  mode.value === 'system' ? (media.matches ? 'dark' : 'light') : mode.value,
)

function apply(value: ThemeMode) {
  document.documentElement.dataset.theme = value

  /* 启动底色只是首帧兜底，主题接管后交给 CSS 变量 */
  document.getElementById('boot-bg')?.remove()
}

/* 系统主题变化：跟随模式下实时反映 */
function applySystemPreference() {
  if (mode.value === 'system') apply(media.matches ? 'dark' : 'light')
}

watch(resolved, (value) => {
  apply(value)
  window.localStorage.setItem(STORAGE_KEY, mode.value)
})

/* 另一个窗口改了主题 → 本窗口跟着变 */
window.addEventListener('storage', (event) => {
  if (event.key !== STORAGE_KEY) return
  const next = event.newValue
  if (next === 'light' || next === 'dark' || next === 'system') mode.value = next
})

media.addEventListener('change', applySystemPreference)

export function useTheme() {
  /* 首次调用时把当前主题落到 DOM，避免接管窗沿用 HTML 里的硬编码默认值 */
  apply(resolved.value)
  return { mode, resolved }
}
