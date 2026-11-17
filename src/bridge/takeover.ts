import { invoke } from '@tauri-apps/api/core'

export interface PendingTakeover {
  url: string
  fileName: string
  saveDir: string
  referrer: string
  cookies: string
  userAgent: string
  fileSize: number
}

/** 扩展送来接管请求：后端登记并弹出二级窗 */
export const takeoverBegin = (url: string, fileName?: string, saveDir?: string) =>
  invoke<PendingTakeover>('takeover_begin', { url, fileName, saveDir })

export const takeoverPending = () => invoke<PendingTakeover | null>('takeover_pending')

export const takeoverClose = () => invoke<void>('takeover_close')

/** 新内容已渲染完成：允许后端显示/抬起窗口 */
export const takeoverReady = () => invoke<void>('takeover_ready')
