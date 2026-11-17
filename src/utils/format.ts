/** 数值格式化：单位、速率、时长。 */

const SIZE_UNITS = ['B', 'KB', 'MB', 'GB', 'TB']

export function formatSize(bytes: number): string {
  if (!bytes || bytes <= 0) return '--'
  if (bytes < 1024) return `${Math.round(bytes)} B`
  let value = bytes
  let index = 0
  while (value >= 1024 && index < SIZE_UNITS.length - 1) {
    value /= 1024
    index += 1
  }
  return `${value.toFixed(1)} ${SIZE_UNITS[index]}`
}

export function formatSpeed(bytesPerSecond: number): string {
  if (!bytesPerSecond || bytesPerSecond <= 0) return '0 KB/s'
  if (bytesPerSecond < 1024) return `${Math.round(bytesPerSecond)} B/s`
  const kb = bytesPerSecond / 1024
  if (kb < 1024) return `${Math.round(kb)} KB/s`
  const mb = kb / 1024
  if (mb < 1024) return `${mb.toFixed(1)} MB/s`
  return `${(mb / 1024).toFixed(1)} GB/s`
}

/**
 * 时长统一用 HH:MM:SS 表示。
 * 未知（暂停 / 失败 / 未开始 / 丢包测不出剩余）一律显示 00:00:00，
 * 避免 "--:--:--" 与数字形态来回切换造成视觉跳动。
 */
export function formatDuration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || seconds < 0) return '00:00:00'
  const total = Math.floor(seconds)
  const hh = String(Math.floor(total / 3600)).padStart(2, '0')
  const mm = String(Math.floor((total % 3600) / 60)).padStart(2, '0')
  const ss = String(total % 60).padStart(2, '0')
  return `${hh}:${mm}:${ss}`
}
