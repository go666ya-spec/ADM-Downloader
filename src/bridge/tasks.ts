import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export type TaskStateDto =
  | 'queued'
  | 'downloading'
  | 'paused'
  | 'stopped'
  | 'completed'

export interface TaskDto {
  id: number
  name: string
  url: string
  state: TaskStateDto
  downloaded: number
  total: number
  speed: number
  /** 服务端算好的 0~100，前端只负责插值 */
  percent: number
  elapsedSeconds: number
  remainingSeconds: number | null
  sizeText: string
  downloadedText: string
  speedText: string
  peakSpeed: number
  peakSpeedText: string
  elapsedText: string
  remainingText: string
  connections: number
  activeConnections: number
  /** 需要用户填写用户名密码 */
  needsAuth: boolean
  /** 每个连接的实时速率（字节/秒） */
  connectionSpeeds: number[]
  /** 每个连接的状态：idle / connecting / downloading / error */
  connectionStates: string[]
  /** 是否来自磁盘恢复（重启后首次开始要播 0→当前 动画） */
  restored: boolean
  /** 任务是否带用户名密码 */
  hasAuth: boolean
  /** 已保存的用户名（密码不回传） */
  authUser: string | null
  /** basic / bearer；没有当前凭据时为 null */
  authType: 'basic' | 'bearer' | null
  /** 成品路径（完成后才有） */
  path: string | null
  /** 已完成之后的附加警告，例如最终文件改名失败 */
  completionWarning: string | null
  error: string | null
  saveDir: string
}

export const listTasks = () => invoke<TaskListSnapshot>('tasks_snapshot')

/** 启动时的配置恢复提示，取一次就清空 */
export const storageNotice = () => invoke<string | null>('storage_notice')

export const enqueueTask = (
  url: string,
  fileName: string,
  saveDir: string,
  referrer = '',
  cookies = '',
  user = '',
  password = '',
  fileSize = 0,
  userAgent = '',
  authorization = '',
) =>
  invoke<number>('task_enqueue', {
    url,
    fileName,
    saveDir,
    referrer,
    cookies,
    user: user || null,
    password: password || null,
    fileSize: fileSize || null,
    userAgent: userAgent || null,
    authorization: authorization || null,
  })

export const pauseTask = (id: number) => invoke<void>('task_pause', { id })
export const resumeTask = (id: number) => invoke<void>('task_resume', { id })
export const cancelTask = (id: number) => invoke<void>('task_cancel', { id })
export const removeTask = (id: number) => invoke<void>('task_remove', { id })

/** 重新下载已完成/已停止的任务（复用原任务卡） */
export const redownloadTask = (id: number) => invoke<void>('task_redownload', { id })

/** 认证失败后提交用户名密码，任务重新排队 */
export const setTaskAuth = (
  id: number,
  authType: 'basic' | 'bearer',
  user: string,
  password: string,
  token: string,
) =>
  invoke<void>('task_set_auth', { id, authType, user, password, token: token || null })
export const clearTasks = () => invoke<void>('task_clear')
export const setParallel = (parallel: number) => invoke<void>('queue_set_parallel', { parallel })

/** 整列表：全部开始 / 全部暂停 */
export const startAllTasks = () => invoke<void>('tasks_start_all')
export const pauseAllTasks = () => invoke<void>('tasks_pause_all')

/** 计划任务：所有任务结束后执行关机/休眠 */
export interface PlanInfoDto {
  active: boolean
  action: string
}

export const planEnable = () => invoke<PlanInfoDto>('plan_enable')
export const planCancelTask = () => invoke<PlanInfoDto>('plan_cancel')
export const planState = () => invoke<PlanInfoDto>('plan_state')

/** 打开文件（系统默认程序）与在资源管理器中定位 */
export const openFile = (path: string) => invoke<void>('open_file', { path })
export const openFolder = (path: string) => invoke<void>('open_folder', { path })

/** 列表结构变化：整表替换 */
/** 结构变化只推变化的任务 + 被删除的 id */
export interface TaskPatch {
  revision: number
  upserts: TaskDto[]
  removed: number[]
}

export interface TaskListSnapshot {
  revision: number
  tasks: TaskDto[]
}

export const onTasksChanged = (handler: (patch: TaskPatch) => void): Promise<UnlistenFn> =>
  listen<TaskPatch>('tasks:changed', (event) => handler(event.payload))

/** 每秒一次：只含并行池（下载中）的任务 */
/** 任务完成：服务端移除前推一次快照，前端留作历史 */
export const onTaskCompleted = (handler: (task: TaskDto) => void): Promise<UnlistenFn> =>
  listen<TaskDto>('task:completed', (event) => handler(event.payload))

export const onTasksProgress = (handler: (list: TaskDto[]) => void): Promise<UnlistenFn> =>
  listen<TaskDto[]>('tasks:progress', (event) => handler(event.payload))
