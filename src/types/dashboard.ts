/** 仪表盘的数据形状：来自后端汇总快照，前端只负责展示与格式化。 */

export type SpeedMode = 'smart' | 'limited'

export type RouteKind = 'direct' | 'system-proxy'

/** 静态系统信息（CPU 型号、内存总量），启动读一次 */
export interface SystemInfo {
  cpuModel: string
  memory: string
}

/** 下载线路状态 */
export interface RouteInfo {
  routeKind: RouteKind
  routeDetail: string
  /** 系统默认路由是否被 TUN 接管 */
  tun: boolean
}

export interface TransferSnapshot {
  speedMode: SpeedMode
  uploadBps: number
  downloadBps: number
}

export interface TaskCounts {
  active: number
  queued: number
}

export interface PlanSnapshot {
  active: boolean
  downloading: number
  queued: number
  canStart: boolean
}

export type TaskState = 'downloading' | 'queued' | 'paused' | 'done' | 'failed'

export interface TaskPreview {
  id: string
  name: string
  state: TaskState
  progress: number
}
