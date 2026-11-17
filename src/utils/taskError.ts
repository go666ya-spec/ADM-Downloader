import { t } from '@/i18n'
import { formatSize } from '@/utils/format'

/**
 * 后端错误以「代码|参数」的形式返回，与语言无关；
 * 这里按当前界面语言渲染，避免英文界面里冒出中文报错。
 */
const KEY_BY_CODE: Record<string, string> = {
  http_status: 'error.httpStatus',
  empty_file: 'error.emptyFile',
  create_dir: 'error.createDir',
  disk_space: 'error.diskSpace',
  create_file: 'error.createFile',
  reserve: 'error.reserve',
  rename: 'error.rename',
  no_size: 'error.noSize',
  cancelled: 'error.cancelled',
  incomplete: 'error.incomplete',
  probe_failed: 'error.probeFailed',
  network: 'error.network',
  write: 'error.write',
  range_ignored: 'error.rangeIgnored',
  http_416: 'error.http416',
  invalid_url: 'error.invalidUrl',
  paused: 'error.paused',
  name_taken: 'error.nameTaken',
  auth_required: 'error.authRequired',
}

export function localizeError(raw?: string | null): string {
  if (!raw) return ''

  /* 新格式：code|arg1|arg2 */
  const parts = raw.split('|')
  if (parts.length >= 2) {
    const [code, ...args] = parts
    const key = KEY_BY_CODE[code]
    if (!key) return raw
    return render(key, code, args)
  }

  /* 旧任务里存的是中文句子，这里做一次兼容翻译 */
  const legacy = matchLegacy(raw)
  if (legacy) {
    return render(legacy.key, legacy.code, legacy.args)
  }

  return raw
}

function render(key: string, code: string, args: string[]): string {
  let text = t(key)

  if (code === 'disk_space') {
    return text
      .replace('{0}', formatSize(Number(args[0]) || 0))
      .replace('{1}', formatSize(Number(args[1]) || 0))
  }

  args.forEach((value, index) => {
    text = text.replaceAll(`{${index}}`, value)
  })

  return text
}

/** 兼容旧版本写入的中文错误文案 */
function matchLegacy(raw: string): { key: string; code: string; args: string[] } | null {
  const http = raw.match(/^服务器返回 HTTP (\d+)$/)
  if (http) return { key: 'error.httpStatus', code: 'http_status', args: [http[1]] }

  const rules: Array<[RegExp, string, string]> = [
    [/^文件大小为 0/, 'error.emptyFile', 'empty_file'],
    [/^创建目录失败[：:](.*)$/, 'error.createDir', 'create_dir'],
    [/^创建临时文件失败[：:](.*)$/, 'error.createFile', 'create_file'],
    [/^预占磁盘空间失败[：:](.*)$/, 'error.reserve', 'reserve'],
    [/^重命名失败[：:](.*)$/, 'error.rename', 'rename'],
    [/^探测请求失败[：:](.*)$/, 'error.probeFailed', 'probe_failed'],
    [/^写入失败[：:](.*)$/, 'error.write', 'write'],
    [/^下载已取消$/, 'error.cancelled', 'cancelled'],
    [/^下载未完成/, 'error.incomplete', 'incomplete'],
  ]

  for (const [pattern, key, code] of rules) {
    const hit = raw.match(pattern)
    if (hit) return { key, code, args: hit[1] ? [hit[1]] : [] }
  }

  return null
}
