import { reactive } from 'vue'

/**
 * 轻量提示条：
 * - 只有一行文本，没有按钮
 * - 出现后 2 秒自动消失，或任意位置按下鼠标立刻消失
 * - 刚出现的 250ms 内不响应点击，避免触发它的那次点击顺手把它关掉
 * - 只做提示，不触碰任务状态（缓存可能还在，任务还能重新下载）
 */
const DISMISS_MS = 2000
const GUARD_MS = 250

export const toast = reactive({
  visible: false,
  message: '',
  shownAt: 0,
})

let timer: number | undefined

export function showToast(message: string) {
  toast.message = message
  toast.visible = true
  toast.shownAt = performance.now()

  window.clearTimeout(timer)
  timer = window.setTimeout(() => {
    toast.visible = false
  }, DISMISS_MS)
}

export function hideToast(force = false) {
  if (!toast.visible) return
  if (!force && performance.now() - toast.shownAt < GUARD_MS) return

  toast.visible = false
  window.clearTimeout(timer)
  timer = undefined
}
