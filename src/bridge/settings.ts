import { invoke } from '@tauri-apps/api/core'

export interface SettingsDto {
  /** 下载文件存放目录 */
  saveDir: string
  /** 单任务最大连接数 */
  connections: number
  /** 单服务器最大连接数 */
  /** 最小分段（MiB） */
  minSegmentMib: number
  /** 断点缓存（MiB） */
  cacheMib: number
  /** 并行任务数 */
  parallel: number
  /** 下载限速 KiB/s，0 为不限 */
  downloadLimitKib: number
  /** system | direct */
  proxyMode: string
  /** 性能档位，仅界面展示 */
  speedMode: string
  /** 开机启动 */
  autostart: boolean
  /** 有任务下载时，退出前先确认 */
  quitConfirm: boolean
  /** 下载完成或失败时发系统通知 */
  notifyOnFinish: boolean
  /** system | light | dark */
  theme: string
  /** system | zh-cn | zh-tw | en | ko | ja */
  language: string
  /** smart | limited */
  bandwidthMode: string
  /** shutdown | sleep */
  planAction: string
  /** 下载时阻止系统休眠 */
  keepAwake: boolean
}

export const settingsSnapshot = () => invoke<SettingsDto>('settings_snapshot')

export const settingsSave = (settings: SettingsDto) => invoke<void>('settings_save', { settings })

/** 弹系统文件夹选择框，取消时返回 null */
export const pickFolder = (title = '选择文件夹') =>
  invoke<string | null>('pick_folder', { title })
