/**
 * 全局禁用 WebView2 自带的浏览器右键菜单（返回 / 刷新 / 另存为 / 打印 / 检查…）。
 * 软件里唯一允许的右键菜单是下载列表里我们自己画的那个——
 * 它自己处理 contextmenu 事件并渲染自定义菜单，不依赖浏览器菜单。
 */
export function disableBrowserContextMenu() {
  document.addEventListener('contextmenu', (event) => {
    event.preventDefault()
  })
}
