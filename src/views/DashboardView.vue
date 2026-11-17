<template>
  <div class="dashboard">
    <PageHeader :title="t('page.dashboard.title')" :subtitle="t('page.dashboard.sub')" />

    <div class="page-scroll">

    <div class="dashboard__grid dashboard__grid--summary">
      <SystemPanel :info="info" :route="route" />
      <SpeedPanel v-model="speedMode" />
      <RatePanel :label="t('rate.download')" :bytes-per-second="downloadBps" />
    </div>

    <div class="dashboard__grid dashboard__grid--paired">
      <PlanPanel :plan="plan" @toggle="togglePlan" />
      <ActivityPanel :counts="counts" />
    </div>

    <TaskFeedPanel @add="onAddTask" />
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { useRouter } from 'vue-router'

import PageHeader from '@/components/PageHeader.vue'
import ActivityPanel from '@/components/dashboard/ActivityPanel.vue'
import PlanPanel from '@/components/dashboard/PlanPanel.vue'
import RatePanel from '@/components/dashboard/RatePanel.vue'
import SpeedPanel from '@/components/dashboard/SpeedPanel.vue'
import SystemPanel from '@/components/dashboard/SystemPanel.vue'
import TaskFeedPanel, { type AddTaskPayload } from '@/components/dashboard/TaskFeedPanel.vue'
import { enqueueTask, planCancelTask, planEnable, planState, type PlanInfoDto } from '@/bridge/tasks'
import { useDefaultDir } from '@/composables/useDefaultDir'
import { t } from '@/i18n'
import { useSystemSnapshot } from '@/composables/useSystemSnapshot'
import { useBandwidthMode } from '@/composables/useBandwidthMode'
import { taskStore } from '@/stores/tasks'
import { isValidTaskLink } from '@/utils/linkKind'
import { localizeError } from '@/utils/taskError'
import { showToast } from '@/composables/useToast'
import type { PlanSnapshot, TaskCounts } from '@/types/dashboard'

const { info, route } = useSystemSnapshot()
const router = useRouter()


const { mode: speedMode, load: loadBandwidthMode, apply: applyBandwidthMode } = useBandwidthMode()

/* 切换限速模式：立即写回配置，后端按模式重算实际限速 */
watch(speedMode, (value, previous) => {
  if (value === previous) return
  void applyBandwidthMode(value)
})

let unlistenPlan: UnlistenFn | undefined

onMounted(() => {
  void loadBandwidthMode()

  void planState()
    .then((info) => {
      plan.active = info.active
    })
    .catch(() => undefined)

  /* 计划触发后后端会自动关闭，这里跟着同步 */
  void listen<PlanInfoDto>('plan:changed', (event) => {
    plan.active = event.payload.active
  })
    .then((dispose) => {
      unlistenPlan = dispose
    })
    .catch(() => undefined)
})

onBeforeUnmount(() => {
  unlistenPlan?.()
})

const downloadBps = ref(0)

/* 任务计数直接来自队列仓库，不再写死 */
const counts = computed<TaskCounts>(() => ({
  active: taskStore.activeCount,
  queued: taskStore.queuedCount,
}))

const plan = reactive<PlanSnapshot>({
  active: false,
  downloading: 0,
  queued: 0,
  canStart: false,
})

watch(
  counts,
  (value) => {
    plan.downloading = value.active
    plan.queued = value.queued
    plan.canStart = value.active + value.queued > 0
    if (value.active + value.queued === 0 && plan.active) {
      plan.active = false
    }
  },
  { immediate: true },
)

async function togglePlan() {
  if (plan.active) {
    const info = await planCancelTask()
    plan.active = info.active
    return
  }
  if (!plan.canStart) return

  const info = await planEnable()
  plan.active = info.active
}

/* 新建任务：保存目录取系统「下载」文件夹 */
const defaultDir = useDefaultDir()

async function onAddTask(payload: AddTaskPayload) {
  const url = payload.url.trim()
  if (!url) return

  if (!isValidTaskLink(url)) {
    showToast(t('feed.invalidUrl'))
    return
  }

  try {
    const basic = payload.authType === 'basic'
    const bearer = payload.authType === 'bearer'
    await enqueueTask(
      url,
      payload.fileName,
      payload.saveDir || defaultDir.value,
      payload.referer,
      payload.cookie,
      basic ? payload.username : '',
      basic ? payload.password : '',
      0,
      payload.userAgent,
      bearer ? `Bearer ${payload.token}` : '',
    )
    await router.push('/downloads')
  } catch (reason) {
    showToast(localizeError(String(reason)))
  }
}
</script>

<style scoped>
.dashboard {
  display: flex;
  flex-direction: column;
  gap: 16px;
  height: 100%;
  min-height: 0;
  padding-bottom: 0;
  container-type: inline-size;
}

.dashboard__grid {
  display: grid;
  gap: 16px;
  min-height: 0;
}

/* 卡片永远不允许撑破栅格列，内容超宽时由卡片内部自己省略 */
.dashboard__grid > * {
  min-width: 0;
  min-height: 0;
}

.dashboard__grid--summary {
  /* 以默认窗口宽度为基准，四列按相同比例共同伸缩。 */
  grid-template-columns:
    minmax(0, 1.68fr)
    minmax(0, 0.77fr)
    minmax(0, 1fr);
}

.dashboard__grid--paired {
  /* 复用顶部同一套四列边界：左侧跨两列，右侧跨两列。 */
  grid-template-columns:
    minmax(0, 1.68fr)
    minmax(0, 0.77fr)
    minmax(0, 1fr);
}

.dashboard__grid--paired > :first-child {
  grid-column: span 2;
}

.dashboard__grid--paired > :nth-child(2) {
  grid-column: span 1;
}

/*
 * 窗口宽度只决定卡片宽度；窗口高度由三行共同吸收。
 * 比例以最小窗口下的 122 / 142 / 286 为基准：
 * 顶部六卡和底部任务区一起随高度增长，URL 卡片在任务区内部保持固定高度。
 */
.dashboard .page-scroll {
  display: grid;
  grid-template-rows:
    minmax(122px, 1fr)
    minmax(142px, 1.16fr)
    minmax(286px, 2.34fr);
  overflow: hidden;
  padding-bottom: 0;
}
</style>
