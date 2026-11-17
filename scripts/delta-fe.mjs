import { readFileSync, writeFileSync } from 'node:fs'

function patch(file, pairs) {
  let text = readFileSync(file, 'utf8')
  for (const [from, to] of pairs) {
    if (!text.includes(from)) {
      console.log(`MISS ${file}: ${from.slice(0, 60)}`)
      continue
    }
    text = text.split(from).join(to)
  }
  writeFileSync(file, text)
}

patch('src/bridge/tasks.ts', [
  [
    `export const onTasksChanged = (handler: (list: TaskDto[]) => void): Promise<UnlistenFn> =>
  listen<TaskDto[]>('tasks:changed', (event) => handler(event.payload))`,
    `/** 结构变化只推变化的任务 + 被删除的 id */
export interface TaskPatch {
  upserts: TaskDto[]
  removed: number[]
}

export const onTasksChanged = (handler: (patch: TaskPatch) => void): Promise<UnlistenFn> =>
  listen<TaskPatch>('tasks:changed', (event) => handler(event.payload))`,
  ],
])

/* store：应用增量补丁 */
let store = readFileSync('src/stores/tasks.ts', 'utf8')
store = store.replace(
  `onTasksChanged,`,
  `onTasksChanged,
  type TaskPatch,`,
)
store = store.replace(
  `disposers.push(await onTasksChanged((list) => this.applyList(list)))`,
  `disposers.push(await onTasksChanged((patch) => this.applyPatch(patch)))`,
)

const start = store.indexOf('  applyList(list: TaskDto[]) {')
const end = store.indexOf('  /** 每秒进度：只更新下载中的任务，并启动插值 */')
if (start < 0 || end < 0) {
  console.log('MISS applyList block')
} else {
  const block = `  /** 结构变化：只更新变化的任务，删除消失的任务 */
  applyList(list: TaskDto[]) {
    const existingById = new Map(tasks.map((task) => [task.id, task]))
    const next = list.map((dto) => {
      const existing = existingById.get(dto.id)
      if (!existing) return toView(dto)
      Object.assign(existing, toView(dto))
      return existing
    })
    tasks.splice(0, tasks.length, ...next)
  },

  applyPatch(patch: TaskPatch) {
    const existingById = new Map(tasks.map((task) => [task.id, task]))

    for (const id of patch.removed) {
      animations.delete(id)
      const index = tasks.findIndex((task) => task.id === id)
      if (index >= 0) tasks.splice(index, 1)
    }

    for (const dto of patch.upserts) {
      const existing = existingById.get(dto.id)

      if (!existing) {
        tasks.push(toView(dto))
        continue
      }

      const resets = dto.downloaded < existing.displayDownloaded
      if (dto.state === 'completed' || dto.state === 'stopped' || resets) {
        animations.delete(dto.id)
      }

      Object.assign(existing, toView(dto))
    }

    tasks.sort((left, right) => left.id - right.id)
  },

`
  store = store.slice(0, start) + block + store.slice(end)
}

writeFileSync('src/stores/tasks.ts', store)

/* 二级窗：合并两个 changed 监听 */
let popup = readFileSync('src/popup/TakeoverApp.vue', 'utf8')
popup = popup.replace(
  `  type TaskDto,`,
  `  type TaskDto,
  type TaskPatch,`,
)
popup = popup.replace(
  `    unlistenChanged = await listen<TaskDto[]>('tasks:changed', (event) => {
      const id = taskId.value
      if (id === null) return
      const hit = event.payload.find((item) => item.id === id)
      if (hit) task.value = hit
    })

`,
  '',
)
popup = popup.replace(
  `    unlistenChanged = await listen<TaskDto[]>('tasks:changed', (event) => {
      const id = taskId.value
      if (id === null) return
      if (event.payload.some((item) => item.id === id)) return

      /* 当前弹窗对应的任务被删除/清空：直接关闭弹窗 */
      taskId.value = null
      task.value = null
      completed.value = false
      void takeoverClose()
    })`,
  `    unlistenChanged = await listen<TaskPatch>('tasks:changed', (event) => {
      const id = taskId.value
      if (id === null) return

      const hit = event.payload.upserts.find((item) => item.id === id)
      if (hit) task.value = hit

      if (event.payload.removed.includes(id)) {
        /* 当前弹窗对应的任务被删除/清空：直接关闭弹窗 */
        taskId.value = null
        task.value = null
        completed.value = false
        void takeoverClose()
      }
    })`,
)
writeFileSync('src/popup/TakeoverApp.vue', popup)

console.log('delta patch wired')
