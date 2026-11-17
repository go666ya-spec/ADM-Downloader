/** 创建任务前校验 HTTP / HTTPS 下载链接。 */
export function isValidTaskLink(text: string): boolean {
  try {
    const parsed = new URL(text.trim())
    return parsed.protocol === 'http:' || parsed.protocol === 'https:'
  } catch {
    return false
  }
}
