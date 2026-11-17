import { ref, watch } from 'vue'

export type ProxyMode = 'system' | 'direct'

const STORAGE_KEY = 'adm.proxyMode'

function readStored(): ProxyMode {
  return window.localStorage.getItem(STORAGE_KEY) === 'direct' ? 'direct' : 'system'
}

/**
 * 网络相关偏好：代理模式。
 * 放在模块级，偏好页与仪表盘共享同一份状态；写 localStorage 以便下次启动恢复。
 */
const proxyMode = ref<ProxyMode>(readStored())

watch(proxyMode, (value) => {
  window.localStorage.setItem(STORAGE_KEY, value)
})

export function useNetworkSettings() {
  return { proxyMode }
}
