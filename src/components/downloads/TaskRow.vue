<template>
  <article
    @wheel="onWheel"
    class="task-row"
    :class="{ 'task-row--expanded': expanded }"
    @contextmenu.prevent="emit('menu', $event, task)"
  >
    <div class="task-row__head">
      <div class="task-row__text">
        <span class="task-row__status" :style="{ color: stateColor }">{{ stateLabel }}</span>
        <span class="task-row__spacer" />
        <span class="task-row__name" :title="task.name">{{ task.name }}</span>
      </div>

      <div class="task-row__actions">
        <button class="task-row__detail-btn" data-detail-toggle @click="emit('toggle-detail', task.id)">
          {{ expanded ? t('common.collapse') : t('common.detail') }}
        </button>
        <button
          class="task-row__toggle"
          :class="{
            'task-row__toggle--hidden':
              task.state === 'completed' || task.state === 'stopped',
          }"
          :disabled="task.state === 'completed' || task.state === 'stopped'"
          :title="running ? t('common.pause') : t('common.start')"
          @click="emit('toggle-run', task.id)"
        >
          <AppIcon :name="running ? 'pause' : 'play'" :size="14" :stroke-width="1.6" />
        </button>
      </div>
    </div>

    <div class="task-row__stats">
      <span v-for="stat in stats" :key="stat.label" class="task-stat">
        <span class="task-stat__label">{{ stat.label }}</span>
        <span class="task-stat__value">{{ stat.value }}</span>
      </span>
    </div>

    <p v-if="task.error && task.state !== 'completed'" class="task-row__error">
      {{ localizeError(task.error) }}
    </p>
    <p
      v-else-if="task.state === 'completed' && task.completionWarning"
      class="task-row__error"
      :title="localizeError(task.completionWarning)"
    >
      {{ localizeError(task.completionWarning) }}
    </p>

    <div class="task-row__progress">
      <div class="task-row__bar">
        <div class="task-row__bar-fill" :style="barStyle" />
      </div>
      <span class="task-row__percent">{{ percentText }}%</span>
    </div>

    <Transition name="detail-fade">
      <div v-if="expanded" ref="extraRef" class="task-row__extra">
      <div class="extra-grid">
        <div v-for="row in detailRows" :key="row.label" class="extra-cell">
          <span class="extra-cell__label">{{ row.label }}</span>
          <span class="extra-cell__value">{{ row.value }}</span>
        </div>
      </div>

      <div class="extra-field">
        <span class="extra-field__label">{{ t('detail.url') }}</span>
        <span class="extra-field__value">{{ task.url }}</span>
      </div>

      <div class="extra-field">
        <span class="extra-field__label">{{ t('detail.dir') }}</span>
        <span class="extra-field__value">{{ task.saveDir }}</span>
      </div>


      <!-- 每个连接的真实速率：由下载核心采样，随每秒推送刷新 -->
      <div v-if="connectionCells.length" class="conn-grid">
        <div v-for="cell in connectionCells" :key="cell.id" class="conn-cell">
          <span class="conn-cell__label">{{ cell.label }}</span>
          <span
            class="conn-cell__value"
            :class="{ 'conn-cell__value--error': cell.state === 'error' }"
          >{{ cell.value }}</span>
        </div>
      </div>

      <!-- 已完成：提供打开入口（文件已从服务移除，只剩磁盘上的成品） -->
      <div v-if="task.state === 'completed'" class="done-actions">
        <button class="done-actions__btn" @click="openDone('file')">{{ t('detail.openFile') }}</button>
        <button class="done-actions__btn" @click="openDone('folder')">{{ t('detail.openFolder') }}</button>
      </div>

      <!-- 认证失败：在详情滚动区里补用户名密码，提交后重新排队 -->
      <div v-if="task.needsAuth || task.hasAuth" class="auth-block">
        <AppSegmented
          v-model="authType"
          class="auth-block__type"
          :options="[
            { label: t('feed.basic'), value: 'basic' },
            { label: t('feed.bearer'), value: 'bearer' },
          ]"
          width="180px"
        />
        <div class="auth-block__row">
          <template v-if="authType === 'basic'">
            <input v-model="authUser" class="auth-block__input" :placeholder="t('detail.authUser')" />
            <input
              v-model="authPassword"
              class="auth-block__input"
              type="password"
              :placeholder="t('detail.authPass')"
            />
          </template>
          <input
            v-else
            v-model="authToken"
            class="auth-block__input"
            type="password"
            :placeholder="t('feed.token')"
          />
          <button class="auth-block__submit" @click="submitAuth">{{ t('common.confirm') }}</button>
        </div>
      </div>
      </div>
    </Transition>
  </article>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'

import AppIcon from '@/components/AppIcon.vue'
import AppSegmented from '@/components/ui/AppSegmented.vue'
import { t } from '@/i18n'
import { formatSpeed } from '@/utils/format'
import { taskTargetPath } from '@/utils/taskPath'
import { openFile, openFolder } from '@/bridge/tasks'
import { showToast } from '@/composables/useToast'
import { localizeError } from '@/utils/taskError'
import type { TaskView } from '@/stores/tasks'

const props = defineProps<{ task: TaskView; expanded: boolean }>()
const emit = defineEmits<{
  menu: [event: MouseEvent, task: TaskView]
  'toggle-detail': [id: number]
  'toggle-run': [id: number]
  'set-auth': [
    id: number,
    authType: 'basic' | 'bearer',
    user: string,
    password: string,
    token: string,
  ]
}>()

async function openDone(kind: 'file' | 'folder') {
  const target = taskTargetPath(props.task)
  try {
    await (kind === 'file' ? openFile(target) : openFolder(target))
  } catch (reason) {
    const text = String(reason)
    const missing = text.includes('不存在') || text.includes('找不到')
    showToast(t(missing ? 'toast.fileMissing' : 'toast.openFailed'))
  }
}

const authUser = ref(props.task.authUser ?? '')
const authPassword = ref('')
const authToken = ref('')
const authType = ref<'basic' | 'bearer'>(props.task.authType ?? 'basic')

/* 切换任务时带出已保存的用户名 */
watch(
  () => props.task.authUser,
  (value) => {
    authUser.value = value ?? ''
    authPassword.value = ''
    authToken.value = ''
  },
)

watch(
  () => props.task.authType,
  (value) => {
    if (value) authType.value = value
  },
)

function submitAuth() {
  if (authType.value === 'basic') {
    if (!authUser.value.trim() || !authPassword.value) return
  } else if (!authToken.value.trim()) {
    return
  }
  emit(
    'set-auth',
    props.task.id,
    authType.value,
    authUser.value.trim(),
    authPassword.value,
    authToken.value.trim(),
  )
  authUser.value = ''
  authPassword.value = ''
  authToken.value = ''
}

const STATE_COLOR: Record<TaskView['state'], string> = {
  queued: 'var(--warning)',
  downloading: 'var(--accent)',
  paused: 'var(--text-secondary)',
  stopped: 'var(--danger)',
  completed: 'var(--success)',
}

const CONN_LABEL: Record<string, string> = {
  idle: 'detail.connIdle',
  connecting: 'detail.connConnecting',
  downloading: '',
  error: 'detail.connError',
}

const extraRef = ref<HTMLElement | null>(null)

/** 展开后，鼠标在这张卡的任意位置滚动，都只滚详情区 */
function onWheel(event: WheelEvent) {
  if (!props.expanded) return
  const element = extraRef.value
  if (!element) return
  event.preventDefault()
  element.scrollTop += event.deltaY
}

const connectionCells = computed(() =>
  (props.task.connectionSpeeds ?? []).map((bytes, index) => {
    const state = props.task.connectionStates?.[index] ?? 'idle'
    const key = CONN_LABEL[state]

    return {
      id: index,
      state,
      label: `${t('detail.connection')} ${String(index + 1).padStart(2, '0')}`,
      value: state === 'downloading' || !key ? formatSpeed(bytes) : t(key),
    }
  }),
)

const stateLabel = computed(() => t(`state.${props.task.state}`))
const stateColor = computed(() => STATE_COLOR[props.task.state])

const running = computed(
  () =>
    props.task.state === 'downloading' ||
    props.task.state === 'queued',
)

const stats = computed(() => [
  { label: t('stat.size'), value: props.task.sizeText },
  { label: t('stat.speed'), value: props.task.speedText },
  { label: t('stat.peakSpeed'), value: props.task.peakSpeedText ?? '0 KB/s' },
  { label: t('stat.elapsed'), value: props.task.elapsedText },
  { label: t('stat.remaining'), value: props.task.remainingText },
  {
    label: t('stat.connections'),
    value: `${props.task.activeConnections}/${props.task.connections}`,
  },
])

const detailRows = computed(() => [
  { label: t('stat.downloaded'), value: props.task.downloadedText },
  { label: t('stat.size'), value: props.task.sizeText },
  { label: t('stat.speed'), value: props.task.speedText },
  { label: t('stat.peakSpeed'), value: props.task.peakSpeedText ?? '0 KB/s' },
  { label: t('stat.elapsed'), value: props.task.elapsedText },
  { label: t('stat.remaining'), value: props.task.remainingText },
  {
    label: t('stat.connections'),
    value: `${props.task.activeConnections} / ${props.task.connections}`,
  },
])

const percentText = computed(() => Math.round(props.task.displayPercent))

const barStyle = computed(() => {
  const percent = Math.min(Math.max(props.task.displayPercent, 0), 100)
  const color =
    props.task.state === 'stopped'
      ? 'var(--danger)'
      : props.task.state === 'paused'
        ? 'var(--warning)'
        : props.task.state === 'queued'
          ? 'var(--text-muted)'
          : 'var(--accent)'

  return {
    width: `${percent}%`,
    background: color,
  }
})
</script>

<style scoped>
.task-row {
  position: relative;
  height: 92px;
  padding: 0 18px;
  overflow: hidden;
  border-radius: 18px;
  background: var(--card);
  transition: height 180ms ease;
  container-type: inline-size;
}

.task-row--expanded {
  height: 340px;
}

.task-row__head {
  position: absolute;
  top: 8px;
  right: 12px;
  left: 18px;
  display: flex;
  align-items: center;
  gap: 6ch;
  height: 24px;
}

.task-row__text {
  display: flex;
  flex: 1;
  align-items: baseline;
  min-width: 0;
  overflow: hidden;
  font-size: var(--fs-row);
  white-space: nowrap;
}

.task-row__status {
  flex: none;
}

.task-row__spacer {
  flex: none;
  width: 2ch;
}

.task-row__name {
  flex: 0 1 auto;
  min-width: 0;
  overflow: hidden;
  font-weight: 700;
  color: var(--text);
  text-overflow: ellipsis;
}

.task-row__actions {
  display: flex;
  align-items: center;
  flex: none;
  gap: 8px;
}

.task-row__detail-btn {
  height: 24px;
  padding: 0 8px;
  border: none;
  border-radius: 7px;
  background: transparent;
  font-size: var(--fs-row);
  color: var(--text-muted);
  cursor: default;
}

.task-row__detail-btn:hover {
  color: var(--text);
}

.task-row__toggle {
  display: grid;
  place-items: center;
  width: 28px;
  height: 26px;
  border: none;
  border-radius: 8px;
  background: transparent;
  color: var(--text-secondary);
  cursor: default;
}

.task-row__toggle--hidden {
  visibility: hidden;
}

.task-row__toggle:hover {
  background: var(--card-hover);
  color: var(--text);
}

.task-row__stats {
  position: absolute;
  top: 38px;
  right: 100px;
  left: 18px;
  display: grid;
  grid-template-columns: repeat(6, minmax(0, 1fr));
  gap: 6px;
}

.task-stat {
  display: flex;
  align-items: baseline;
  gap: 5px;
  min-width: 0;
}

.task-stat__label {
  flex: none;
  font-size: var(--fs-row);
  color: var(--text-muted);
}

.task-stat__value {
  overflow: hidden;
  font-size: var(--fs-row);
  color: var(--text-secondary);
  font-variant-numeric: tabular-nums;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.task-row__error {
  position: absolute;
  top: 56px;
  right: 100px;
  left: 18px;
  margin: 0;
  overflow: hidden;
  font-size: var(--fs-row);
  color: var(--danger);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.task-row__progress {
  position: absolute;
  top: 68px;
  right: 18px;
  left: 18px;
  display: flex;
  align-items: center;
  gap: 10px;
  height: 16px;
}

.task-row__bar {
  flex: 1;
  height: 6px;
  overflow: hidden;
  border-radius: 3px;
  background: var(--input);
}

/* 宽度由每帧插值驱动，这里不要再加 transition，否则会双重缓动 */
.task-row__bar-fill {
  height: 100%;
  border-radius: 3px;
}

.task-row__percent {
  flex: none;
  width: 42px;
  font-size: var(--fs-row);
  line-height: 16px;
  color: var(--text-muted);
  font-variant-numeric: tabular-nums;
  text-align: right;
}

.detail-fade-enter-active,
.detail-fade-leave-active {
  transition: opacity 160ms ease;
}

.detail-fade-enter-from,
.detail-fade-leave-to {
  opacity: 0;
}

.task-row__extra {
  position: absolute;
  top: 96px;
  right: 18px;
  bottom: 14px;
  left: 18px;
  display: flex;
  flex-direction: column;
  overflow-x: hidden;
  overflow-y: auto;
  padding-top: 12px;
  padding-right: 8px;
  border-top: 1px solid var(--hairline);
  scrollbar-gutter: stable;
}

.conn-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 4px 12px;
  margin-top: 10px;
  padding-top: 10px;
  border-top: 1px solid var(--hairline);
}

@container (max-width: 620px) {
  .conn-grid {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}

@container (min-width: 1000px) {
  .conn-grid {
    grid-template-columns: repeat(8, minmax(0, 1fr));
  }
}

.conn-cell {
  display: flex;
  align-items: center;
  gap: 6px;
  min-width: 0;
  overflow: hidden;
  font-size: var(--fs-row);
}

.conn-cell__label {
  color: var(--text-muted);
  white-space: nowrap;
}

.conn-cell__value--error {
  color: var(--danger);
}

.conn-cell__value {
  flex: 1;
  min-width: 0;
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.extra-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 4px 20px;
}

@container (max-width: 560px) {
  .extra-grid {
    grid-template-columns: minmax(0, 1fr);
  }
}

.extra-cell {
  display: flex;
  align-items: baseline;
  gap: 10px;
  min-width: 0;
}

.extra-cell__label {
  flex: none;
  width: 64px;
  font-size: var(--fs-row);
  color: var(--text-muted);
}

.extra-cell__value {
  min-width: 0;
  overflow: hidden;
  font-size: var(--fs-row);
  color: var(--text-secondary);
  font-variant-numeric: tabular-nums;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.extra-field {
  display: flex;
  gap: 10px;
  margin-top: 10px;
  min-width: 0;
}

.extra-field__label {
  flex: none;
  width: 64px;
  font-size: var(--fs-row);
  color: var(--text-muted);
}

.done-actions {
  display: flex;
  gap: 8px;
  margin-top: 12px;
  padding-top: 10px;
  border-top: 1px solid var(--hairline);
}

.done-actions__btn {
  height: 28px;
  padding: 0 12px;
  border: none;
  border-radius: 7px;
  background: var(--input);
  color: var(--text);
  font-family: inherit;
  font-size: var(--fs-row);
  cursor: default;
}

.auth-block {
  margin-top: 12px;
  padding-top: 10px;
  border-top: 1px solid var(--hairline);
}

.auth-block__row {
  display: flex;
  gap: 8px;
}

.auth-block__type {
  margin-bottom: 8px;
}

.auth-block__input {
  flex: 1;
  min-width: 0;
  height: 28px;
  padding: 0 9px;
  border: none;
  border-radius: 7px;
  background: var(--input);
  color: var(--text);
  font-family: inherit;
  font-size: var(--fs-row);
  outline: none;
}

.auth-block__submit {
  flex: none;
  height: 28px;
  padding: 0 14px;
  border: none;
  border-radius: 7px;
  background: var(--accent);
  color: var(--on-accent);
  font-family: inherit;
  font-size: var(--fs-row);
  cursor: default;
}

.extra-field__value {
  min-width: 0;
  font-size: var(--fs-row);
  color: var(--text);
  overflow-wrap: anywhere;
  word-break: break-all;
}
</style>
