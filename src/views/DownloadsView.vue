<template>
  <div class="downloads">
    <div class="downloads__sticky">
      <PageHeader :title="t('page.downloads.title')" :subtitle="t('page.downloads.sub')" />

      <div class="downloads__toolbar">
        <div class="downloads__batch">
          <button
            class="downloads__batch-toggle"
            :class="{ 'is-open': batchMenuOpen }"
            :title="t('downloads.actions')"
            :aria-expanded="batchMenuOpen"
            aria-haspopup="menu"
            @click.stop="toggleBatchMenu"
          >
            <span class="downloads__batch-dots" aria-hidden="true">
              <i />
              <i />
              <i />
            </span>
          </button>

          <div v-if="batchMenuOpen" class="downloads__batch-menu" role="menu" @click.stop>
            <button
              class="downloads__batch-item"
              role="menuitem"
              :disabled="!hasTasks"
              @click="startAll"
            >
              {{ t('downloads.startAll') }}
            </button>
            <button
              class="downloads__batch-item"
              role="menuitem"
              :disabled="!hasTasks"
              @click="pauseAll"
            >
              {{ t('downloads.pauseAll') }}
            </button>
            <button
              class="downloads__batch-item is-danger"
              role="menuitem"
              :disabled="!hasTasks"
              @click="deleteAll"
            >
              {{ t('downloads.deleteAll') }}
            </button>
            <button class="downloads__batch-item" role="menuitem" @click="newTask">
              {{ t('downloads.newTask') }}
            </button>
          </div>
        </div>

        <span class="downloads__summary">{{ summary }}</span>
      </div>
    </div>

    <div class="page-scroll">
      <div class="downloads__list">
      <TaskRow
        v-for="task in taskStore.tasks"
        :key="task.id"
        :task="task"
        :expanded="detailId === task.id"
        @menu="openMenu"
        @toggle-detail="toggleDetail"
        @toggle-run="toggleRun"
        @set-auth="onSetAuth"
      />

        <div v-if="!taskStore.tasks.length" class="downloads__empty">{{ t('downloads.empty') }}</div>
      </div>
    </div>

    <div v-if="menu.visible" class="downloads__menu-backdrop" @click="menu.visible = false" />
    <TaskContextMenu v-if="menu.visible" :x="menu.x" :y="menu.y" @select="onMenuSelect" />
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref } from 'vue'
import { useRouter } from 'vue-router'

import PageHeader from '@/components/PageHeader.vue'
import AppIcon from '@/components/AppIcon.vue'
import TaskContextMenu from '@/components/downloads/TaskContextMenu.vue'
import TaskRow from '@/components/downloads/TaskRow.vue'
import {
  clearTasks,
  setTaskAuth,
  openFile,
  openFolder,
  pauseAllTasks,
  pauseTask,
  redownloadTask,
  removeTask,
  resumeTask,
  startAllTasks,
} from '@/bridge/tasks'
import { showToast } from '@/composables/useToast'
import { t } from '@/i18n'
import { taskTargetPath } from '@/utils/taskPath'
import { taskStore, type TaskView } from '@/stores/tasks'

const detailId = ref<number | null>(null)
const menu = reactive({ visible: false, x: 0, y: 0, taskId: 0 })
const batchMenuOpen = ref(false)
const hasTasks = computed(() => taskStore.tasks.length > 0)
const router = useRouter()

/* 工具栏统计：只统计列表里实际存在的任务 */
const summary = computed(() => {
  const total = taskStore.tasks.length
  if (total === 0) return t('downloads.none')

  const parts = [`${total} ${t('summary.tasks')}`]
  const downloading = taskStore.tasks.filter((task) => task.state === 'downloading').length
  const queued = taskStore.tasks.filter((task) => task.state === 'queued').length
  const paused = taskStore.tasks.filter((task) => task.state === 'paused').length

  if (downloading > 0) parts.push(`${downloading} ${t('summary.downloading')}`)
  if (queued > 0) parts.push(`${queued} ${t('summary.queued')}`)
  if (paused > 0) parts.push(`${paused} ${t('summary.paused')}`)

  return parts.join(' · ')
})

function toggleDetail(id: number) {
  detailId.value = detailId.value === id ? null : id
}

function toggleRun(id: number) {
  const task = taskStore.tasks.find((item) => item.id === id)
  if (!task || task.state === 'completed' || task.state === 'stopped') return

  if (task.state === 'downloading' || task.state === 'queued') {
    taskStore.setStateLocal(id, 'paused')
    void pauseTask(id).catch(() => taskStore.load())
  } else {
    taskStore.setStateLocal(id, 'queued')
    void resumeTask(id).catch(() => taskStore.load())
  }
}

async function onSetAuth(
  id: number,
  authType: 'basic' | 'bearer',
  user: string,
  password: string,
  token: string,
) {
  await setTaskAuth(id, authType, user, password, token)
}

async function startAll() {
  batchMenuOpen.value = false
  taskStore.setAllStateLocal(['paused'], 'queued')
  try {
    await startAllTasks()
  } catch {
    await taskStore.load()
  }
}

async function pauseAll() {
  batchMenuOpen.value = false
  taskStore.setAllStateLocal(['downloading', 'queued'], 'paused')
  try {
    await pauseAllTasks()
  } catch {
    await taskStore.load()
  }
}

async function deleteAll() {
  batchMenuOpen.value = false
  if (!hasTasks.value) return
  taskStore.clearLocal()
  detailId.value = null
  try {
    await clearTasks()
  } catch {
    await taskStore.load()
  }
}

async function newTask() {
  batchMenuOpen.value = false
  await router.push('/dashboard')
}

function toggleBatchMenu() {
  menu.visible = false
  batchMenuOpen.value = !batchMenuOpen.value
}

function openMenu(event: MouseEvent, task: TaskView) {
  batchMenuOpen.value = false
  menu.visible = true
  menu.x = event.clientX
  menu.y = event.clientY
  menu.taskId = task.id
}

async function onMenuSelect(action: string) {
  const task = taskStore.tasks.find((item) => item.id === menu.taskId)
  menu.visible = false
  if (!task) return

  /* 目标文件路径：保存目录 + 文件名 */
  const target = taskTargetPath(task)

  switch (action) {
    case 'open-file':
      await safeOpen(() => openFile(target))
      break
    case 'open-folder':
      await safeOpen(() => openFolder(target))
      break
    case 'redownload':
      /* 任何状态都可以重新下载：静态数据不变，动态数据归零 */
      await redownloadTask(task.id)
      break
    case 'copy-url':
      await copyText(task.url)
      break
    case 'copy-path':
      await copyText(target)
      break
    case 'remove':
      /* 主动操作：先移除本地卡片，再通知后端 */
      taskStore.removeLocal(task.id)
      await removeTask(task.id)
      if (detailId.value === task.id) detailId.value = null
      break
    default:
      break
  }
}

async function safeOpen(run: () => Promise<void>) {
  try {
    await run()
  } catch (reason) {
    const text = String(reason)
    console.warn('[open]', text)
    const missing = text.includes('不存在') || text.includes('找不到')
    /* 只提示，不动任务：缓存可能还在，任务仍可重新下载 */
    showToast(t(missing ? 'toast.fileMissing' : 'toast.openFailed'))
  }
}

async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text)
  } catch {
    /* 没有剪贴板权限时静默失败 */
  }
}

function onDocumentClick(event: MouseEvent) {
  const target = event.target as HTMLElement | null
  if (!target) return
  if (!target.closest('.downloads__batch')) batchMenuOpen.value = false
  if (detailId.value === null) return
  if (target.closest('.task-row--expanded')) return
  if (target.closest('[data-detail-toggle]')) return
  detailId.value = null
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    menu.visible = false
    batchMenuOpen.value = false
    detailId.value = null
  }
}

onMounted(() => {
  document.addEventListener('click', onDocumentClick)
  document.addEventListener('keydown', onKeydown)
})

onBeforeUnmount(() => {
  document.removeEventListener('click', onDocumentClick)
  document.removeEventListener('keydown', onKeydown)
})
</script>

<style scoped>
.downloads {
  position: relative;
  min-height: 100%;
  padding-bottom: 16px;
  container-type: inline-size;
}

.downloads__list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

/* 标题 + 工具栏固定在顶部，只有卡片列表参与滚动 */
.downloads__sticky {
  position: sticky;
  top: 0;
  z-index: 5;
  background: var(--bg);
  /* 间隔放在吸顶容器上：滚动时也始终和卡片保持同样距离 */
  padding-bottom: 12px;
}

.downloads__toolbar {
  display: flex;
  align-items: center;
  gap: 7px;
  height: 30px;
  margin-top: 2px;
  padding: 0 4px;
}

.downloads__summary {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  font-size: var(--fs-row);
  color: var(--text-muted);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.downloads__batch {
  position: relative;
  flex: none;
}

.downloads__batch-toggle {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  padding: 0;
  border: none;
  border-radius: 8px;
  background: transparent;
  color: var(--text-muted);
  cursor: default;
  transition: color 120ms ease;
}

.downloads__batch-toggle:hover,
.downloads__batch-toggle.is-open {
  background: transparent;
  color: var(--text);
}

.downloads__batch-dots {
  display: flex;
  align-items: center;
  gap: 2px;
}

.downloads__batch-dots i {
  width: 3px;
  height: 3px;
  border-radius: 50%;
  background: currentColor;
}

.downloads__batch-menu {
  position: absolute;
  top: calc(100% + 5px);
  left: 0;
  z-index: 20;
  width: 154px;
  padding: 4px;
  border-radius: 10px;
  background: var(--elevated);
  box-shadow: 0 8px 26px var(--shadow);
}

.downloads__batch-item {
  display: flex;
  align-items: center;
  width: 100%;
  height: 30px;
  padding: 0 10px;
  border: none;
  border-radius: 7px;
  background: transparent;
  color: var(--text-secondary);
  font-family: inherit;
  font-size: 11px;
  text-align: left;
  cursor: default;
}

.downloads__batch-item:hover:not(:disabled) {
  background: var(--card-hover);
  color: var(--text);
}

.downloads__batch-item.is-danger {
  color: var(--danger);
}

.downloads__batch-item:disabled {
  color: var(--text-muted);
  opacity: 0.45;
}

.downloads__empty {
  display: grid;
  place-items: center;
  height: 240px;
  font-size: 15px;
  color: var(--text-muted);
}

.downloads__menu-backdrop {
  position: fixed;
  inset: 0;
  z-index: 30;
}
</style>
