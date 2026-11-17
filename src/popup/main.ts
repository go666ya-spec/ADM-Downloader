import { createApp } from 'vue'

import 'element-plus/theme-chalk/dark/css-vars.css'
import '@/styles/tokens.css'
import '@/styles/panel.css'

import { useTheme } from '@/composables/useTheme'
import { initLocale } from '@/i18n'
import { disableBrowserContextMenu } from '@/utils/disableBrowserContextMenu'
import { installHoverGuard } from '@/composables/useHoverGuard'
import TakeoverApp from './TakeoverApp.vue'

/* 接管窗与主窗口同源，读取同一份主题偏好 */
useTheme()
disableBrowserContextMenu()
installHoverGuard()

/* 语言包同样按需加载：只加载设置里选中的那一种 */
void initLocale().then(() => {
  createApp(TakeoverApp).mount('#popup')
})
