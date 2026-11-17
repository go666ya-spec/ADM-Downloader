import { invoke } from '@tauri-apps/api/core'

export type RouteKindDto = 'direct' | 'system-proxy'

/** 静态系统信息：启动时读一次，之后不再刷新 */
export interface SystemInfoDto {
  cpuModel: string
  memoryTotal: number
}

/** 下载线路：随代理设置变化 */
export interface RouteInfoDto {
  routeKind: RouteKindDto
  routeDetail: string
  tun: boolean
}

export function fetchSystemInfo(): Promise<SystemInfoDto> {
  return invoke<SystemInfoDto>('system_static')
}

/**
 * 取下载线路。
 * proxyMode 传「使用系统代理 / 不使用代理」，force 为 true 时清掉探测缓存立即重判。
 */
export function fetchRouteInfo(proxyMode: 'system' | 'direct', force = false) {
  return invoke<RouteInfoDto>('system_route', { proxyMode, force })
}
