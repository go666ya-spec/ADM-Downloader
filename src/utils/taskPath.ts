export interface TaskPathLike {
  path?: string | null
  saveDir: string
  name: string
}

/** 任务对应的实际路径：成品路径优先，否则保存目录 + 任务名 */
export function taskTargetPath(task: TaskPathLike): string {
  const finished = task.path?.trim()
  if (finished) return finished

  const name = task.name?.trim() || ''
  if (!task.saveDir) return name
  return `${task.saveDir.replace(/[\\/]+$/, '')}\\${name}`
}
