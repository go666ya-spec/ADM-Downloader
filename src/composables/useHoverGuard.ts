/**
 * 窗口最小化/失焦时，鼠标不会触发 mouseleave，
 * 按钮的 :hover 状态会被浏览器保留，恢复窗口后出现"高亮残留"。
 * 这里在失焦/隐藏时给 html 挂 suppress-hover，抵消所有 hover 样式；
 * 鼠标再动一下即恢复。
 */
export function installHoverGuard() {
  const root = document.documentElement

  const suppress = () => root.classList.add('suppress-hover')
  const resume = () => root.classList.remove('suppress-hover')

  window.addEventListener('blur', suppress)
  window.addEventListener('focus', suppress)
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) suppress()
  })

  window.addEventListener('pointermove', resume, { passive: true })
  window.addEventListener('pointerdown', resume, { passive: true })
}
