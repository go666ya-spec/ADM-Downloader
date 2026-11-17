import { onBeforeUnmount, onMounted, reactive, watch } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import { fetchRouteInfo, fetchSystemInfo } from '@/bridge/system'
import { useNetworkSettings } from '@/composables/useNetworkSettings'
import type { RouteInfo, SystemInfo } from '@/types/dashboard'

/** 内存只显示总量，保留一位小数 */
function formatMemory(total: number): string {
  if (!total) return ''
  return `${(total / 1024 / 1024 / 1024).toFixed(1)} GB`
}

/**
 * 系统卡数据，全程无轮询：
 * - 静态信息（CPU 型号、内存总量）：挂载时读一次，之后不再刷新
 * - 线路信息：由后端向系统挂号，代理设置变化时主动通知；另外在窗口恢复/获得焦点/手动点击时补一次
 */
export function useSystemSnapshot() {
  const { proxyMode } = useNetworkSettings()

  const info = reactive<SystemInfo>({ cpuModel: '', memory: '' })
  const route = reactive<RouteInfo>({ routeKind: 'direct', routeDetail: '', tun: false })

  let unlisten: UnlistenFn | undefined

  async function loadStaticInfo() {
    try {
      const dto = await fetchSystemInfo()
      info.cpuModel = dto.cpuModel
      info.memory = formatMemory(dto.memoryTotal)
    } catch {
      /* 浏览器预览下没有 Tauri 命令，保持空值 */
    }
  }

  async function refreshRoute(force = false) {
    try {
      const dto = await fetchRouteInfo(proxyMode.value, force)
      route.routeKind = dto.routeKind
      route.routeDetail = dto.routeDetail
      route.tun = dto.tun
    } catch {
      /* 浏览器预览下没有 Tauri 命令，保持初始态 */
    }
  }

  function onFocus() {
    void refreshRoute()
  }

  function onVisibilityChange() {
    if (!document.hidden) void refreshRoute()
  }

  /* 代理模式切换后立刻重判线路 */
  watch(proxyMode, () => {
    void refreshRoute(true)
  })

  onMounted(async () => {
    await loadStaticInfo()
    await refreshRoute()

    /* 后端挂号发现代理设置变化时会发这个事件 */
    try {
      unlisten = await listen('route-changed', () => {
        void refreshRoute(true)
      })
    } catch {
      /* 浏览器预览下没有 Tauri 事件系统 */
    }

    window.addEventListener('focus', onFocus)
    document.addEventListener('visibilitychange', onVisibilityChange)
  })

  onBeforeUnmount(() => {
    unlisten?.()
    unlisten = undefined
    window.removeEventListener('focus', onFocus)
    document.removeEventListener('visibilitychange', onVisibilityChange)
  })

  return { info, route, refreshRoute }
}
