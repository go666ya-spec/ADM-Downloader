import { reactive } from 'vue'

import {
  listTasks,
  onTasksChanged,
  type TaskPatch,
  type TaskListSnapshot,
  onTasksProgress,
  type TaskDto,
} from '@/bridge/tasks'

/** 展示用任务：除服务端数据外，额外保存一个插值中的百分比 */
export interface TaskView extends TaskDto {
  displayPercent: number
  /** 动画中的已下载字节数：进度由它除以总大小得出 */
  displayDownloaded: number
  /** 重启恢复的任务是否已经播过 0→当前 的动画 */
  swept: boolean
}

/**
 * 任务仓库。
 *
 * 数据流是单向的：
 *   服务端每秒推一次「事实」，这里把目标值记下来，再用 requestAnimationFrame
 *   在一秒内把 displayPercent 从旧值线性推到新值，视觉上是连续的。
 *
 * 没有任何定时器：动画只在需要推进时申请一帧，全部到达目标后自动停止。
 */
const tasks = reactive<TaskView[]>([])

interface Animation {
  from: number
  to: number
  startedAt: number
}

const animations = new Map<number, Animation>()
let frameHandle = 0

const ANIMATION_MS = 1000

/** 已下载字节 → 百分比 */
function percentOf(downloaded: number, total: number) {
  if (total <= 0) return 0
  return Math.max(0, Math.min(100, (downloaded / total) * 100))
}

function easeLinear(t: number) {
  return t
}

function toView(dto: TaskDto): TaskView {
  const existing = tasks.find((task) => task.id === dto.id)
  /*
   * 结构变化时：
   * - 后端百分比更低（重新下载/断点失效）→ 跟随归零
   * - 后端百分比更高（动画还没追上）→ 保持显示值，交给插值追
   * 完成/停止则直接对齐最终值
   */
  /*
   * 结构变化时只更新"目标字节数"：
   * - 后端字节更少（重新下载/断点失效）→ 跟随归零
   * - 后端字节更多（动画没追上）→ 保持显示，交给插值追
   * 完成/停止直接对齐最终值
   */
  const displayDownloaded = existing
    ? dto.state === 'completed' || dto.state === 'stopped'
      ? dto.downloaded
      : dto.state === 'paused'
        ? existing.displayDownloaded
        : Math.min(existing.displayDownloaded, dto.downloaded)
    : dto.downloaded

  return {
    ...dto,
    displayDownloaded,
    displayPercent: percentOf(displayDownloaded, dto.total),
    swept: existing ? existing.swept : false,
  }
}

function step(now: number) {
  let active = false

  animations.forEach((animation, id) => {
    const task = tasks.find((item) => item.id === id)
    const elapsed = now - animation.startedAt
    const t = Math.min(elapsed / ANIMATION_MS, 1)

    if (task) {
      task.displayDownloaded =
        animation.from + (animation.to - animation.from) * easeLinear(t)
      task.displayPercent = percentOf(task.displayDownloaded, task.total)
    }

    if (t >= 1) {
      animations.delete(id)
    } else {
      active = true
    }
  })

  frameHandle = active ? window.requestAnimationFrame(step) : 0
}

function ensureAnimationLoop() {
  if (!frameHandle) {
    frameHandle = window.requestAnimationFrame(step)
  }
}

function animateTo(task: TaskView, targetBytes: number) {
  animations.set(task.id, {
    from: task.displayDownloaded,
    to: targetBytes,
    startedAt: performance.now(),
  })
  ensureAnimationLoop()
}

export const taskStore = {
  tasks,
  revision: 0,
  loaded: false,
  loading: false,
  reloading: false,
  pendingPatches: [] as TaskPatch[],

  get activeCount() {
    return tasks.filter((task) => task.state === 'downloading').length
  },

  get queuedCount() {
    return tasks.filter((task) => task.state === 'queued').length
  },

  get pausedCount() {
    return tasks.filter((task) => task.state === 'paused').length
  },

  /** 首屏拉一次；期间到达的事件先缓存，拿到列表后按版本号补 */
  async load() {
    this.loading = true
    try {
      const snapshot: TaskListSnapshot = await listTasks()
      tasks.splice(0, tasks.length, ...snapshot.tasks.map(toView))
      snapshot.tasks.forEach((dto) => {
        const task = tasks.find((item) => item.id === dto.id)
        if (task) {
          task.displayDownloaded = dto.downloaded
          task.displayPercent = percentOf(dto.downloaded, dto.total)
        }
      })
      this.revision = snapshot.revision
      this.loaded = true
    } catch {
      /* 浏览器预览下没有 Tauri 命令 */
    } finally {
      this.loading = false
      this.flushPending()
    }
  },

  /** 版本号不对时重新拉一次完整列表 */
  reloadSoon() {
    if (this.reloading) return
    this.reloading = true
    window.setTimeout(() => {
      this.reloading = false
      void this.load()
    }, 0)
  },

  flushPending() {
    const pending = this.pendingPatches.splice(0)
    pending.sort((left, right) => left.revision - right.revision)
    for (const patch of pending) this.applyPatch(patch)
  },

  /** 列表结构变化：整表替换（已完成任务也由后端一起下发） */
  /** 主动删除：前端立即移除，不等后端推送 */
  removeLocal(id: number) {
    animations.delete(id)
    const index = tasks.findIndex((task) => task.id === id)
    if (index >= 0) tasks.splice(index, 1)
  },

  /** 用户操作后的即时反馈；后端状态事件随后校正。 */
  setStateLocal(id: number, state: TaskDto['state']) {
    const task = tasks.find((item) => item.id === id)
    if (!task) return

    animations.delete(id)
    task.state = state
    if (state === 'paused' || state === 'queued') {
      task.speed = 0
      task.speedText = '0 KB/s'
    }
  },

  setAllStateLocal(from: TaskDto['state'][], state: TaskDto['state']) {
    const allowed = new Set(from)
    tasks.forEach((task) => {
      if (!allowed.has(task.state)) return
      animations.delete(task.id)
      task.state = state
      task.speed = 0
      task.speedText = '0 KB/s'
    })
  },

  /** 主动清空：前端立即清空 */
  clearLocal() {
    animations.clear()
    tasks.splice(0, tasks.length)
  },

  /** 结构变化：只更新变化的任务，删除消失的任务 */
  applyList(list: TaskDto[]) {
    const existingById = new Map(tasks.map((task) => [task.id, task]))
    const next = list.map((dto) => {
      const existing = existingById.get(dto.id)
      if (!existing) return toView(dto)
      Object.assign(existing, toView(dto))

      if (dto.state === 'paused' || dto.state === 'stopped' || dto.state === 'completed') {
        animations.delete(dto.id)
      }
      if (dto.state === 'stopped' || dto.state === 'completed') {
        existing.displayDownloaded = dto.downloaded
        existing.displayPercent = percentOf(dto.downloaded, dto.total)
      }
      return existing
    })
    tasks.splice(0, tasks.length, ...next)
  },

  applyPatch(patch: TaskPatch) {
    if (this.loading || !this.loaded) {
      this.pendingPatches.push(patch)
      return
    }

    if (patch.revision <= this.revision) return

    if (patch.revision > this.revision + 1) {
      /* 中间漏了事件：重新拉完整列表 */
      this.reloadSoon()
      return
    }

    this.revision = patch.revision

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
      if (
        dto.state === 'completed' ||
        dto.state === 'stopped' ||
        dto.state === 'paused' ||
        resets
      ) {
        animations.delete(dto.id)
      }

      Object.assign(existing, toView(dto))

      if (dto.state === 'paused' || dto.state === 'stopped' || dto.state === 'completed') {
        animations.delete(dto.id)
      }
      if (dto.state === 'stopped' || dto.state === 'completed') {
        existing.displayDownloaded = dto.downloaded
        existing.displayPercent = percentOf(dto.downloaded, dto.total)
      }
    }

    tasks.sort((left, right) => left.id - right.id)
  },

  /** 每秒进度：只更新下载中的任务，并启动插值 */
  applyProgress(list: TaskDto[]) {
    for (const dto of list) {
      const task = tasks.find((item) => item.id === dto.id)
      if (!task) continue
      if (task.state === 'paused' || task.state === 'stopped' || task.state === 'completed') continue

      /* 控制句柄刚重建时会出现一帧 0%；下载中不做倒退，避免进度条闪归零 */
      if (
        task.state === 'downloading' &&
        dto.downloaded === 0 &&
        task.downloaded > 0 &&
        dto.state === 'downloading'
      ) {
        continue
      }

      if (dto.state === 'paused') {
        animations.delete(dto.id)
        continue
      }

      if (dto.state !== 'downloading') {
        animations.delete(dto.id)
        Object.assign(task, { state: dto.state, downloaded: dto.downloaded, total: dto.total })
        task.displayDownloaded = dto.downloaded
        task.displayPercent = percentOf(dto.downloaded, dto.total)
        continue
      }

      Object.assign(task, {
        downloaded: dto.downloaded,
        total: dto.total,
        speed: dto.speed,
        percent: dto.percent,
        elapsedSeconds: dto.elapsedSeconds,
        remainingSeconds: dto.remainingSeconds,
        sizeText: dto.sizeText,
        downloadedText: dto.downloadedText,
        speedText: dto.speedText,
        elapsedText: dto.elapsedText,
        remainingText: dto.remainingText,
        connections: dto.connections,
        activeConnections: dto.activeConnections,
        needsAuth: dto.needsAuth,
        connectionSpeeds: dto.connectionSpeeds ?? [],
        connectionStates: dto.connectionStates ?? [],
        hasAuth: dto.hasAuth,
        authUser: dto.authUser,
        path: dto.path,
      })

      /* 重启恢复的任务：第一次开始下载时，从 0 播到当前进度 */
      if (task.restored && !task.swept && task.state === 'downloading' && dto.downloaded > 0) {
        task.swept = true
        task.displayDownloaded = 0
        task.displayPercent = 0
      }

      /* 下载中进度只前进不倒退，避免控制句柄切换时闪归零 */
      /* 目标字节：下载中只前进；其它状态对齐后端 */
      const targetBytes =
        task.state === 'downloading'
          ? Math.max(dto.downloaded, task.displayDownloaded)
          : dto.downloaded
      animateTo(task, targetBytes)
    }
  },

  /** 挂上两个事件，返回解绑函数 */
  async attach(): Promise<() => void> {
    const disposers: Array<() => void> = []

    try {
      disposers.push(await onTasksChanged((patch) => this.applyPatch(patch)))
      disposers.push(await onTasksProgress((list) => this.applyProgress(list)))
    } catch {
      /* 浏览器预览下没有事件系统 */
    }

    return () => disposers.forEach((dispose) => dispose())
  },
}
