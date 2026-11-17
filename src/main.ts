import { createApp } from 'vue'

import 'element-plus/theme-chalk/dark/css-vars.css'
import './styles/tokens.css'
import './styles/panel.css'

import { useTheme } from './composables/useTheme'
import { installHoverGuard } from './composables/useHoverGuard'
import { disableBrowserContextMenu } from './utils/disableBrowserContextMenu'
import App from './App.vue'
import router from './router'

/* 启动即应用主题偏好，而不是等用户打开设置页 */
useTheme()
disableBrowserContextMenu()
installHoverGuard()

createApp(App).use(router).mount('#app')
