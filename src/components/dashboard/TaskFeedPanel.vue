<template>
  <div class="feed-stack">
    <article
      ref="composeRef"
      class="panel feed-compose"
      :class="{ 'feed-compose--animating': animating }"
      :style="composeStyle"
    >
      <div class="feed-panel__input-row">
        <AppLineInput
          v-model="draft"
          class="feed-panel__input"
          :placeholder="t('feed.placeholder')"
          @submit="submit"
        />
        <button class="feed-panel__add" :title="t('feed.add')" @click="submit()">
          <AppIcon name="check" :size="15" :stroke-width="2.1" />
        </button>
        <button
          class="feed-panel__advanced-toggle"
          :class="{ 'feed-panel__advanced-toggle--active': advanced }"
          :title="t('feed.advanced')"
          @click="toggleAdvanced"
        >
          {{ advanced ? t('common.collapse') : t('feed.advanced') }}
        </button>
      </div>

      <Transition name="advanced-fade">
        <div v-if="advanced" class="feed-panel__advanced">
        <div class="advanced-row">
          <span class="advanced-row__label">{{ t('feed.saveDir') }}</span>
          <div class="advanced-row__control advanced-row__control--path">
            <AppLineInput v-model="saveDir" class="advanced-input" />
            <button class="advanced-browse" @click="browse">{{ t('common.browse') }}</button>
          </div>
        </div>

        <div class="advanced-row">
          <span class="advanced-row__label">{{ t('feed.fileName') }}</span>
          <div class="advanced-row__control">
            <AppLineInput v-model="fileName" class="advanced-input" :placeholder="t('feed.auto')" />
          </div>
        </div>

        <div class="advanced-row">
          <span class="advanced-row__label">{{ t('feed.userAgent') }}</span>
          <div class="advanced-row__control">
            <AppLineInput v-model="userAgent" class="advanced-input" :placeholder="t('feed.auto')" />
          </div>
        </div>

        <div class="advanced-row">
          <span class="advanced-row__label">{{ t('feed.referer') }}</span>
          <div class="advanced-row__control">
            <AppLineInput v-model="referer" class="advanced-input" :placeholder="t('feed.optional')" />
          </div>
        </div>

        <div class="advanced-row advanced-row--wide">
          <span class="advanced-row__label">{{ t('feed.cookie') }}</span>
          <div class="advanced-row__control">
            <textarea
              v-model="cookie"
              class="advanced-textarea"
              spellcheck="false"
              :placeholder="t('feed.optional')"
            />
          </div>
        </div>

        <div class="advanced-row">
          <span class="advanced-row__label">{{ t('feed.authorization') }}</span>
          <div class="advanced-row__control">
            <AppSegmented v-model="authType" :options="authOptions" width="260px" />
          </div>
        </div>

        <div v-if="authType === 'basic'" class="advanced-row">
          <span class="advanced-row__label">{{ t('feed.username') }}</span>
          <div class="advanced-row__control advanced-row__control--auth">
            <AppLineInput v-model="username" class="advanced-input" />
            <span class="advanced-row__inline-label">{{ t('feed.password') }}</span>
            <AppLineInput v-model="password" class="advanced-input" type="password" />
          </div>
        </div>

        <div v-if="authType === 'bearer'" class="advanced-row">
          <span class="advanced-row__label">{{ t('feed.token') }}</span>
          <div class="advanced-row__control">
            <AppLineInput v-model="token" class="advanced-input" type="password" />
          </div>
        </div>

        </div>
      </Transition>
    </article>

    <article class="panel feed-list-card">
      <header class="feed-list-card__head">
        <h3 class="panel__title">{{ t('feed.title') }}</h3>
        <span class="feed-panel__summary">{{ summary }}</span>
      </header>

      <div class="feed-panel__list">
        <div v-for="task in visibleTasks" :key="task.id" class="feed-row">
          <span class="feed-row__dot" :style="{ background: stateColor(task.state) }" />
          <span class="feed-row__name" :title="task.name">{{ task.name }}</span>
          <span class="feed-row__percent">{{ Math.round(task.displayPercent) }}%</span>
        </div>
      </div>
    </article>
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'

import { t } from '@/i18n'

import AppIcon from '@/components/AppIcon.vue'
import AppLineInput from '@/components/ui/AppLineInput.vue'
import AppSegmented from '@/components/ui/AppSegmented.vue'
import { useDefaultDir } from '@/composables/useDefaultDir'
import { taskStore, type TaskView } from '@/stores/tasks'
import { pickFolder } from '@/bridge/settings'

export interface AddTaskPayload {
  url: string
  saveDir: string
  fileName: string
  userAgent: string
  referer: string
  cookie: string
  authType: 'basic' | 'bearer'
  username: string
  password: string
  token: string
}

const emit = defineEmits<{
  add: [payload: AddTaskPayload]
}>()

const draft = ref('')
const advanced = ref(false)
const saveDir = useDefaultDir()
const fileName = ref('')
const userAgent = ref('')
const referer = ref('')
const cookie = ref('')
const authType = ref<'basic' | 'bearer'>('basic')
const username = ref('')
const password = ref('')
const token = ref('')
const composeRef = ref<HTMLElement | null>(null)
const composeStyle = ref<Record<string, string>>({})
const animating = ref(false)
let collapsedHeight = 0
let animationTimer = 0

const authOptions = computed(() => [
  { label: t('feed.basic'), value: 'basic' },
  { label: t('feed.bearer'), value: 'bearer' },
])

const VISIBLE = new Set<TaskView['state']>(['downloading', 'queued', 'paused'])

const GROUP_ORDER: Record<TaskView['state'], number> = {
  downloading: 0,
  queued: 1,
  paused: 2,
  stopped: 3,
  completed: 4,
}

const visibleTasks = computed(() =>
  taskStore.tasks
    .filter((task) => VISIBLE.has(task.state))
    .slice()
    .sort((left, right) => {
      const group = GROUP_ORDER[left.state] - GROUP_ORDER[right.state]
      return group !== 0 ? group : left.id - right.id
    }),
)

const downloadingCount = computed(
  () => visibleTasks.value.filter((task) => task.state === 'downloading').length,
)

const summary = computed(
  () => `${visibleTasks.value.length} ${t('feed.tasks')} · ${downloadingCount.value} ${t('feed.downloading')}`,
)

function stateColor(state: TaskView['state']) {
  if (state === 'downloading') return 'var(--accent)'
  if (state === 'queued') return 'var(--warning)'
  return 'var(--text-secondary)'
}

function layoutCompose() {
  const card = composeRef.value
  const scroll = document.querySelector('.dashboard .page-scroll') as HTMLElement | null
  if (!card || !scroll || !advanced.value) return

  const top = card.getBoundingClientRect().top
  const bottom = scroll.getBoundingClientRect().bottom
  const height = Math.max(180, Math.round(bottom - top))

  composeStyle.value = { height: `${height}px` }
}

function stopAnimationTimer() {
  if (animationTimer) {
    window.clearTimeout(animationTimer)
    animationTimer = 0
  }
}

async function toggleAdvanced() {
  const card = composeRef.value
  stopAnimationTimer()

  if (!advanced.value) {
    collapsedHeight = card?.offsetHeight ?? 0
    /* 先固定折叠高度，下一帧再设目标高度，CSS 才有起止值可动画 */
    composeStyle.value = { height: `${collapsedHeight}px` }
    animating.value = true
    advanced.value = true

    await nextTick()
    layoutCompose()
    animationTimer = window.setTimeout(() => {
      animating.value = false
    }, 280)
  } else {
    const current = card?.offsetHeight ?? 0
    composeStyle.value = { height: `${current}px` }
    animating.value = true
    advanced.value = false

    await nextTick()
    composeStyle.value = { height: `${collapsedHeight}px` }

    animationTimer = window.setTimeout(() => {
      composeStyle.value = {}
      animating.value = false
    }, 280)
  }
}

function onWindowResize() {
  if (advanced.value) layoutCompose()
}

onMounted(() => window.addEventListener('resize', onWindowResize))
onBeforeUnmount(() => {
  window.removeEventListener('resize', onWindowResize)
  stopAnimationTimer()
})

async function browse() {
  try {
    const folder = await pickFolder(t('feed.saveDir'))
    if (folder) saveDir.value = folder
  } catch {
    /* 取消选择 */
  }
}

function submit(value?: string) {
  const url = (value ?? draft.value).trim()
  if (!url) return

  const basic = authType.value === 'basic' && username.value.trim() !== '' && password.value !== ''
  const bearer = authType.value === 'bearer' && token.value.trim() !== ''

  emit('add', {
    url,
    saveDir: saveDir.value.trim(),
    fileName: fileName.value.trim(),
    userAgent: userAgent.value.trim(),
    referer: referer.value.trim(),
    cookie: cookie.value.trim(),
    authType: authType.value,
    username: basic ? username.value.trim() : '',
    password: basic ? password.value : '',
    token: bearer ? token.value.trim() : '',
  })
}
</script>

<style scoped>
.feed-stack {
  position: relative;
  display: flex;
  flex: 1 1 auto;
  flex-direction: column;
  gap: 12px;
  min-height: 0;
}

.feed-compose,
.feed-list-card {
  padding: 16px 20px;
}

.feed-compose {
  display: flex;
  flex: none;
  flex-direction: column;
  overflow: hidden;
}

.feed-compose--animating {
  transition: height 240ms ease;
}

.feed-list-card {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
}

.feed-list-card__head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 16px;
}

.feed-panel__summary {
  font-size: var(--fs-row);
  color: var(--text-muted);
}

.feed-panel__input-row {
  display: flex;
  gap: 8px;
}

.feed-panel__advanced {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 7px;
  min-height: 0;
  margin-top: 10px;
  padding: 10px 12px;
  overflow-y: auto;
  border-radius: 12px;
  background: color-mix(in srgb, var(--input) 55%, transparent);
  scrollbar-gutter: stable;
}

.advanced-row {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
}

.advanced-row__label {
  flex: none;
  width: 88px;
  font-size: var(--fs-row);
  color: var(--text-muted);
}

.advanced-row__control {
  display: flex;
  flex: 1;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.advanced-row__control--path .advanced-input,
.advanced-row__control--auth .advanced-input {
  flex: 1;
}

.advanced-row__inline-label {
  flex: none;
  font-size: var(--fs-row);
  color: var(--text-muted);
}

.advanced-input {
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

.advanced-textarea {
  width: 100%;
  height: 72px;
  padding: 7px 9px;
  resize: vertical;
  border: none;
  border-radius: 7px;
  background: var(--input);
  color: var(--text);
  font-family: inherit;
  font-size: var(--fs-row);
  outline: none;
}

.advanced-browse {
  flex: none;
  height: 28px;
  padding: 0 10px;
  border: none;
  border-radius: 7px;
  background: var(--input);
  color: var(--text-secondary);
  font-family: inherit;
  font-size: var(--fs-row);
  cursor: default;
}

.advanced-browse:hover {
  color: var(--text);
  background: var(--card-hover);
}

.advanced-error {
  margin: 2px 0 0 100px;
  font-size: var(--fs-row);
  color: var(--danger);
}

.advanced-fade-enter-active,
.advanced-fade-leave-active {
  transition: opacity 160ms ease;
}

.advanced-fade-enter-from,
.advanced-fade-leave-to {
  opacity: 0;
}

.feed-panel__advanced-toggle {
  flex: none;
  height: 30px;
  padding: 0 10px;
  border: none;
  border-radius: 8px;
  background: var(--input);
  color: var(--text-muted);
  font-family: inherit;
  font-size: var(--fs-row);
  cursor: default;
  transition: background-color 120ms ease, color 120ms ease;
}

.feed-panel__advanced-toggle:hover {
  background: var(--card-hover);
  color: var(--text);
}

.feed-panel__advanced-toggle--active {
  background: color-mix(in srgb, var(--accent) 16%, transparent);
  color: var(--accent);
}

.feed-panel__input {
  flex: 1;
  min-width: 0;
  height: 30px;
  padding: 0 10px;
  border: none;
  border-radius: 9px;
  background: var(--input);
  color: var(--text);
  font-size: var(--fs-row);
  outline: none;
}

.feed-panel__input::placeholder {
  color: color-mix(in srgb, var(--text-muted) 78%, transparent);
}

.feed-panel__add {
  display: grid;
  place-items: center;
  width: 30px;
  height: 30px;
  border: none;
  border-radius: 8px;
  background: var(--accent);
  color: var(--on-accent);
  cursor: default;
  transition: background-color 120ms ease;
}

.feed-panel__add:hover {
  background: var(--accent-hover);
}

.feed-panel__list {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 2px;
  min-height: 0;
  margin-top: 10px;
  overflow-y: auto;
  scrollbar-gutter: stable;
}

.feed-row {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 30px;
  min-width: 0;
  padding: 0 8px 0 10px;
  border-radius: 7px;
}

.feed-row:hover {
  background: var(--card-hover);
}

.feed-row__dot {
  flex: none;
  width: 7px;
  height: 7px;
  border-radius: 50%;
}

.feed-row__name {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  font-size: var(--fs-row);
  color: var(--text);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.feed-row__percent {
  flex: none;
  font-size: var(--fs-row);
  color: var(--text-muted);
  font-variant-numeric: tabular-nums;
}
</style>
