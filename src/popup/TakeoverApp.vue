<template>
  <div class="takeover">
    <header class="takeover__bar">
      <span class="takeover__dot" />
      <span class="takeover__title">{{ t('popup.title') }}</span>
      <span class="takeover__spacer" />
      <button class="takeover__win-btn" :title="t('window.minimize')" @click="minimize">
        <svg viewBox="0 0 16 16" width="11" height="11" aria-hidden="true">
          <path d="M3 8h10" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
        </svg>
      </button>
      <button class="takeover__win-btn takeover__win-btn--close" :title="t('window.close')" @click="close">
        <svg viewBox="0 0 16 16" width="11" height="11" aria-hidden="true">
          <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
        </svg>
      </button>
    </header>

    <section class="takeover__body">
      <label class="field">
        <span class="field__label">{{ t('popup.fileName') }}</span>
        <AppLineInput
          v-model="fileName"
          :readonly="started"
          :title="displayName"
          class="field__input"
        />
      </label>

      <div class="field">
        <span class="field__label">{{ t('popup.saveTo') }}</span>
        <span class="field__path" :title="savePath">{{ savePath || t('popup.unspecified') }}</span>
        <button class="field__browse" :disabled="started" @click="chooseFolder">{{ t('common.browse') }}</button>
      </div>

      <div class="meta">
        <div class="meta-row">
          <span class="meta__label">{{ t('popup.size') }}</span>
          <span class="meta__value">{{ sizeText }}</span>
        </div>

        <div class="meta-row">
          <span class="meta__label">{{ t('popup.used') }}</span>
          <span class="meta__value">{{ usedText }}</span>
          <span class="meta__label meta__label--gap">{{ t('popup.remaining') }}</span>
          <span class="meta__value">{{ remainingText }}</span>
          <span class="meta__label meta__label--gap">{{ t('stat.peakSpeed') }}</span>
          <span class="meta__value">{{ task?.peakSpeedText ?? '0 KB/s' }}</span>
        </div>

        <div class="meta-row">
          <span class="meta__label">{{ t('popup.source') }}</span>
          <span class="meta__value meta__value--ellipsis" :title="source">{{ source }}</span>
        </div>
      </div>

      <section class="segments">
        <header class="segments__head">
          <span class="segments__title">{{ t('popup.progress') }}</span>
          <span v-if="taskId !== null" class="segments__conn">{{ t('detail.connection') }} {{ connText }}</span>
          <span class="segments__meta">{{ percentText }} · {{ speedText }}</span>
        </header>

        <div class="segments__bar">
          <span class="segments__fill" :style="{ width: `${percent}%`, background: barColor }" />
        </div>
      </section>
    </section>

    <footer class="takeover__footer">
      <!-- 只有下载完成后才出现：此时文件已落盘，缓存文件不提供打开 -->
      <div v-if="completed" class="takeover__footer-group">
        <button class="btn" @click="openTargetFile">
          <AppIcon name="file" :size="13" :stroke-width="1.4" />
          <span>{{ t('common.openFile') }}</span>
        </button>
        <button class="btn" @click="openTargetFolder">
          <AppIcon name="folder" :size="13" :stroke-width="1.4" />
          <span>{{ t('common.openFolder') }}</span>
        </button>
      </div>

      <!-- 下载出错：错误信息占左侧空白，与右侧按钮同一水平线 -->
      <span v-if="errorText" class="takeover__error" :title="errorText">{{ errorText }}</span>

      <div class="takeover__footer-group">
        <!-- 完成时取消消失：任务已结束，没有可取消的对象 -->
        <button v-if="!completed" class="btn" @click="cancel">{{ t('common.cancel') }}</button>
        <!-- 复用按钮：下载 ↔ 暂停；已终止/已完成时冻结 -->
        <button class="btn btn--primary" :disabled="primaryDisabled" @click="onPrimary">
          {{ primaryLabel }}
        </button>
      </div>
    </footer>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

import AppIcon from '@/components/AppIcon.vue'
import AppLineInput from '@/components/ui/AppLineInput.vue'
import {
  cancelTask,
  enqueueTask,
  listTasks,
  openFile,
  pauseTask,
  resumeTask,
  openFolder,
  type TaskDto,
  type TaskPatch,
} from '@/bridge/tasks'
import {
  takeoverClose,
  takeoverPending,
  takeoverReady,
  type PendingTakeover,
} from '@/bridge/takeover'
import { pickFolder } from '@/bridge/settings'
import { t } from '@/i18n'
import { localizeError } from '@/utils/taskError'
import { formatSize } from '@/utils/format'
import { taskTargetPath } from '@/utils/taskPath'

const appWindow = getCurrentWindow()

const pending = ref<PendingTakeover | null>(null)
const fileName = ref('')
const taskId = ref<number | null>(null)
const task = ref<TaskDto | null>(null)
const completed = ref(false)
const busy = ref(false)
const submitError = ref('')

/* 弹窗自己的进度动画：服务端每秒给一次目标值，这里在一秒内线性推过去 */
const displayDownloaded = ref(0)
const ANIM_MS = 1000
let animFrame = 0
let animFrom = 0
let animTo = 0
let animStart = 0

function stopAnimation() {
  if (animFrame) {
    cancelAnimationFrame(animFrame)
    animFrame = 0
  }
}

function snapTo(bytes: number) {
  stopAnimation()
  displayDownloaded.value = Math.max(0, bytes)
}

function animateTo(bytes: number) {
  if (!Number.isFinite(bytes) || bytes < 0) return

  if (bytes <= displayDownloaded.value) {
    snapTo(bytes)
    return
  }

  stopAnimation()
  animFrom = displayDownloaded.value
  animTo = bytes
  animStart = performance.now()

  const tick = (now: number) => {
    const t = Math.min(1, (now - animStart) / ANIM_MS)
    displayDownloaded.value = animFrom + (animTo - animFrom) * t

    if (t < 1) {
      animFrame = requestAnimationFrame(tick)
    } else {
      displayDownloaded.value = animTo
      animFrame = 0
    }
  }

  animFrame = requestAnimationFrame(tick)
}

const started = computed(() => taskId.value !== null)
const displayName = computed(() => fileName.value || pending.value?.fileName || t('popup.unnamed'))
const savePath = computed(() => pending.value?.saveDir ?? '')
const source = computed(() => pending.value?.url ?? '')

/* 完成时任务已从服务移除，这里按 100% 收尾 */
const percent = computed(() => {
  if (completed.value) return 100
  const item = task.value
  if (!item || item.total <= 0) return 0
  return Math.max(0, Math.min(100, (displayDownloaded.value / item.total) * 100))
})
const percentText = computed(() => `${Math.round(percent.value)}%`)

const barColor = computed(() => {
  if (completed.value) return 'var(--success)'
  if (task.value?.state === 'paused') return 'var(--warning)'
  if (task.value?.state === 'stopped' || task.value?.error) return 'var(--danger)'
  if (task.value?.state === 'queued') {
    return 'var(--text-muted)'
  }
  return 'var(--accent)'
})
/* 与主窗口任务卡保持一致：活跃连接 / 实际分配连接 */
const connText = computed(
  () => `${task.value?.activeConnections ?? 0}/${task.value?.connections ?? 0}`,
)
const speedText = computed(() => task.value?.speedText ?? '0 KB/s')
const sizeText = computed(() => {
  if (task.value) return task.value.sizeText
  const expected = pending.value?.fileSize ?? 0
  return expected > 0 ? formatSize(expected) : '—'
})
const usedText = computed(() => task.value?.elapsedText ?? '00:00:00')
const remainingText = computed(() => task.value?.remainingText ?? '00:00:00')

/* 任务失败时的错误文案：本地化后显示在底部左侧 */
const errorText = computed(() => {
  if (completed.value) return ''
  if (submitError.value) return localizeError(submitError.value)
  const raw = task.value?.error
  return raw ? localizeError(raw) : ''
})

function setTaskSnapshot(next: TaskDto) {
  task.value = next
  completed.value =
    next.state === 'completed' && next.total > 0 && next.downloaded >= next.total
}

async function refreshTask(id: number) {
  try {
    const snapshot = await listTasks()
    const hit = snapshot.tasks.find((item) => item.id === id)
    if (hit) setTaskSnapshot(hit)
  } catch {
    /* 命令失败时尽量保留当前画面 */
  }
}

function apply(next: PendingTakeover) {
  pending.value = next
  fileName.value = next.fileName
  taskId.value = null
  task.value = null
  completed.value = false
  submitError.value = ''
  busy.value = false
  snapTo(0)

  /* 等内容真正渲染后再让后端显示窗口，避免残留上一单的界面 */
  void nextTick().then(() => takeoverReady().catch(() => {}))
}

async function load() {
  try {
    const item = await takeoverPending()
    if (item) apply(item)
  } catch {
    /* 浏览器预览下没有 Tauri 命令 */
  }
}

async function chooseFolder() {
  if (started.value) return
  try {
    const folder = await pickFolder(t('popup.saveTo'))
    if (folder && pending.value) {
      pending.value = { ...pending.value, saveDir: folder }
    }
  } catch {
    /* 取消选择 */
  }
}

async function confirm() {
  const item = pending.value
  if (!item || started.value || busy.value) return
  busy.value = true
  submitError.value = ''
  try {
    taskId.value = await enqueueTask(
      item.url,
      fileName.value.trim() || item.fileName,
      item.saveDir,
      item.referrer,
      item.cookies,
      '',
      '',
      item.fileSize,
      item.userAgent,
    )

    /* 创建后立刻取一次快照：连接配额、大小等静态数据马上渲染 */
    try {
      const snapshot = await listTasks()
      const hit = snapshot.tasks.find((task) => task.id === taskId.value)
      if (hit) setTaskSnapshot(hit)
      snapTo(hit?.downloaded ?? 0)
    } catch {
      /* 等事件推送 */
    }
  } catch (reason) {
    submitError.value = String(reason)
  } finally {
    busy.value = false
  }
}

/* 下载按钮复用：下载 ↔ 暂停；已终止、已完成时冻结 */
type Phase = 'idle' | 'running' | 'paused' | 'stopped' | 'completed'

const phase = computed<Phase>(() => {
  if (taskId.value === null) return 'idle'
  if (completed.value) return 'completed'
  if (task.value?.state === 'stopped' || task.value?.error) return 'stopped'
  if (task.value?.state === 'paused') return 'paused'
  return 'running'
})

const primaryLabel = computed(() => {
  switch (phase.value) {
    case 'idle':
    case 'paused':
      return t('common.download')
    case 'running':
      return t('common.pause')
    case 'stopped':
      return t('popup.stopped')
    default:
      return t('popup.completed')
  }
})

const primaryDisabled = computed(
  () => busy.value || phase.value === 'stopped' || phase.value === 'completed',
)

async function onPrimary() {
  const id = taskId.value

  if (phase.value === 'idle') {
    await confirm()
    return
  }
  if (id === null) return

  try {
    if (phase.value === 'running') {
      if (task.value) task.value = { ...task.value, state: 'paused', speed: 0 }
      await pauseTask(id)
    } else if (phase.value === 'paused') {
      if (task.value) task.value = { ...task.value, state: 'queued', speed: 0 }
      await resumeTask(id)
    }
  } catch {
    await refreshTask(id)
  }
}

async function cancel() {
  if (completed.value) return
  const id = taskId.value
  if (id !== null) {
    try {
      await cancelTask(id)
    } catch {
      /* 任务可能已被移除 */
    }
  }
  await closeWindow()
}

async function closeWindow() {
  try {
    await takeoverClose()
  } catch {
    /* 浏览器预览下没有 Tauri 命令 */
  }
}

function targetPath() {
  const current = task.value
  if (current) {
    return taskTargetPath({
      path: current.path,
      saveDir: current.saveDir,
      name: current.name,
    })
  }

  const item = pending.value
  if (!item) return ''
  return `${item.saveDir}\\${fileName.value.trim() || item.fileName}`
}

async function openTargetFile() {
  try {
    await openFile(targetPath())
  } catch {
    /* 文件尚未生成时忽略 */
  }
}

async function openTargetFolder() {
  try {
    await openFolder(targetPath())
  } catch {
    /* 文件尚未生成时忽略 */
  }
}

function minimize() {
  void appWindow.minimize()
}

function close() {
  /* X 只关窗口；取消任务由「取消」按钮负责 */
  void closeWindow()
}

let unlistenPending: UnlistenFn | undefined
let unlistenProgress: UnlistenFn | undefined
let unlistenChanged: UnlistenFn | undefined
let unlistenCompleted: UnlistenFn | undefined

onMounted(async () => {
  try {
    unlistenProgress = await listen<TaskDto[]>('tasks:progress', (event) => {
      const id = taskId.value
      if (id === null) return
      const hit = event.payload.find((item) => item.id === id)
      if (!hit) return

      /* 恢复下载时可能先来一帧 0%，下载中的进度不倒退 */
      if (hit.percent <= 0 && (task.value?.percent ?? 0) > 1) return
      setTaskSnapshot(hit)

      if (hit.state === 'paused') {
        stopAnimation()
      } else if (hit.state === 'stopped' || hit.state === 'completed') {
        snapTo(hit.downloaded)
      } else {
        animateTo(hit.downloaded)
      }
    })

    /* 状态变化（入队/暂停/失败）也更新一次，排队阶段没有进度事件 */
    /* 单窗口复用：新接管来了就重置界面 */
    unlistenChanged = await listen<TaskPatch>('tasks:changed', (event) => {
      const id = taskId.value
      if (id === null) return

      const hit = event.payload.upserts.find((item) => item.id === id)
      if (hit) {
        setTaskSnapshot(hit)
        if (hit.state === 'paused') {
          stopAnimation()
        } else if (hit.state === 'stopped' || hit.state === 'completed') {
          snapTo(hit.downloaded)
        }
      }

      if (event.payload.removed.includes(id)) {
        /* 当前弹窗对应的任务被删除/清空：直接关闭弹窗 */
        taskId.value = null
        task.value = null
        completed.value = false
        void takeoverClose()
      }
    })

    unlistenPending = await listen<PendingTakeover>('takeover:pending', (event) =>
      apply(event.payload),
    )

    /* 服务端在移除任务前推一次完成快照 */
    unlistenCompleted = await listen<TaskDto>('task:completed', (event) => {
      const id = taskId.value
      if (id === null || event.payload.id !== id) return
      setTaskSnapshot({ ...event.payload, state: 'completed', percent: 100, speed: 0 })
      snapTo(event.payload.downloaded)
    })
  } catch {
    /* 浏览器预览下没有事件系统 */
  }

  /* 先挂事件，再读取初始草稿，避免读取与监听之间的状态变化丢失。 */
  await load()
})

onBeforeUnmount(() => {
  stopAnimation()
  unlistenPending?.()
  unlistenProgress?.()
  unlistenChanged?.()
  unlistenCompleted?.()
})
</script>

<style scoped>
.takeover {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100vh;
  overflow: hidden;
  background: var(--bg);
  color: var(--text);
}

.takeover__bar {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 34px;
  flex: 0 0 34px;
  padding-left: 14px;
  padding-right: 5px;
}

.takeover__dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--accent);
  box-shadow: 0 0 8px var(--accent);
}

.takeover__title {
  font-size: 12px;
  font-weight: 700;
  color: var(--text-secondary);
}

.takeover__spacer {
  flex: 1;
  height: 100%;
}

.takeover__win-btn {
  display: grid;
  place-items: center;
  width: 26px;
  height: 22px;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: var(--text-muted);
  cursor: default;
  transition: background-color 120ms ease, color 120ms ease;
}

html.suppress-hover .takeover__win-btn:hover {
  background: transparent;
  color: var(--text-secondary);
}

html.suppress-hover .takeover__win-btn--close:hover {
  background: transparent;
  color: var(--text-secondary);
}

.takeover__win-btn:hover {
  background: var(--elevated);
  color: var(--text);
}

.takeover__win-btn--close:hover {
  background: #e81123;
  color: #fff;
}

.takeover__body {
  flex: 1;
  min-height: 0;
  padding: 10px 20px 0;
  display: flex;
  flex-direction: column;
}

.field {
  display: flex;
  align-items: center;
  /* 标签与控件之间 2 个字符宽 */
  gap: 2ch;
  margin-bottom: 10px;
}

.field__label {
  flex: none;
  font-size: var(--fs-row);
  color: var(--text-muted);
  white-space: nowrap;
}

.field__input {
  flex: 1;
  min-width: 0;
  height: 30px;
  padding: 0 10px;
  border: none;
  border-radius: 8px;
  background: var(--input);
  color: var(--text);
  font-size: var(--fs-row);
  outline: none;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.field__input:focus {
  box-shadow: inset 0 0 0 1px var(--accent);
}

.field__path {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  font-size: var(--fs-row);
  color: var(--text-secondary);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.field__browse {
  flex: none;
  height: 26px;
  padding: 0 10px;
  border: none;
  border-radius: 7px;
  background: var(--input);
  color: var(--text-secondary);
  font-size: var(--fs-note);
  cursor: default;
}

.field__browse:disabled {
  opacity: 0.45;
  cursor: default;
}

.field__browse:hover {
  background: var(--card-hover);
  color: var(--text);
}

.meta {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 4px 0 0;
}

.meta-row {
  display: flex;
  align-items: baseline;
  gap: 1ch;
  min-width: 0;
}

.meta__label {
  flex: none;
  font-size: var(--fs-row);
  color: var(--text-muted);
  white-space: nowrap;
}

.meta__label--gap {
  margin-left: 1ch;
}

.meta__value {
  min-width: 0;
  max-width: 100%;
  overflow: hidden;
  font-size: var(--fs-row);
  color: var(--text-secondary);
  font-variant-numeric: tabular-nums;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.meta__value--ellipsis {
  flex: 1;
}

.segments {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 10px;
  padding-top: 10px;
  border-top: 1px solid var(--hairline);
}

.segments__head {
  display: flex;
  align-items: baseline;
  justify-content: flex-start;
  gap: 0;
}

.segments__conn {
  margin-left: 4px;
  font-size: var(--fs-row);
  color: var(--text-muted);
  font-variant-numeric: tabular-nums;
}

.segments__meta {
  margin-left: auto;
}

.segments__title {
  font-size: var(--fs-row);
  color: var(--text-muted);
}

.segments__meta {
  flex: none;
  overflow: hidden;
  font-size: var(--fs-row);
  color: var(--text-secondary);
  font-variant-numeric: tabular-nums;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.segments__bar {
  display: flex;
  height: 7px;
  overflow: hidden;
  border-radius: 4px;
  background: var(--input);
}

.segments__fill {
  display: block;
  height: 100%;
  border-radius: 999px;
  background: var(--accent);
}

.segment {
  flex: 1;
  overflow: hidden;
}

.segment__fill {
  display: block;
  height: 100%;
  background: var(--accent);
  transition: width 200ms linear;
}

.takeover__footer {
  display: flex;
  align-items: center;
  gap: 10px;
  flex: 0 0 56px;
  padding: 0 20px;
}

.takeover__footer-group {
  display: flex;
  gap: 10px;
}

/* 报错文本：占据左侧剩余空间，与按钮同一基线，超长省略 */
.takeover__error {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  font-size: var(--fs-row);
  color: var(--danger);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.takeover__footer-group:last-child {
  margin-left: auto;
}

.btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  height: 32px;
  padding: 0 12px;
  border: 1px solid transparent;
  border-radius: 9px;
  background: transparent;
  color: var(--text-secondary);
  font-family: inherit;
  font-size: var(--fs-row);
  line-height: 1;
  cursor: default;
  transition: background-color 120ms ease, color 120ms ease, border-color 120ms ease;
}

.btn:hover {
  background: color-mix(in srgb, var(--text) 9%, transparent);
  color: var(--text);
}

.btn--primary {
  width: 96px;
  padding: 0;
  background: var(--accent);
  color: var(--on-accent);
  border-color: transparent;
  font-weight: 700;
}

.btn--primary:hover {
  background: var(--accent-hover);
  color: var(--on-accent);
}

.btn--primary:disabled {
  background: var(--input);
  color: var(--text-muted);
}
</style>
