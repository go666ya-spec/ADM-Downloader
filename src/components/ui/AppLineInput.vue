<template>
  <input
    ref="inputEl"
    :value="modelValue"
    class="line-input"
    :type="type ?? 'text'"
    :placeholder="placeholder"
    :readonly="readonly"
    spellcheck="false"
    @input="onInput"
    @focus="onFocus"
    @blur="onBlur"
    @keyup.enter="onEnter"
    @wheel="onWheel"
  />
</template>

<script setup lang="ts">
import { ref } from 'vue'

/**
 * 单行文本输入框，行为对齐浏览器地址栏：
 * - 聚焦时光标落到末尾并滚动到末尾（先看到尾部）
 * - 失焦或回车即确认（去首尾空白），并抛出 commit 事件
 * - 编辑时滚轮横向滚动，方向键交给输入框原生行为
 * - 失焦后内容超长自动省略号截断（由外部类的 text-overflow 控制）
 */
const props = defineProps<{
  modelValue: string
  placeholder?: string
  readonly?: boolean
  type?: string
}>()

const emit = defineEmits<{
  'update:modelValue': [value: string]
  commit: [value: string]
  submit: [value: string]
}>()

const inputEl = ref<HTMLInputElement | null>(null)

function onInput(event: Event) {
  emit('update:modelValue', (event.target as HTMLInputElement).value)
}

function onFocus() {
  const el = inputEl.value
  if (!el) return
  const end = el.value.length
  el.setSelectionRange(end, end)
  requestAnimationFrame(() => {
    el.scrollLeft = el.scrollWidth
  })
}

function onBlur() {
  const el = inputEl.value
  if (!el) return
  const trimmed = el.value.trim()
  if (trimmed !== el.value) {
    el.value = trimmed
    emit('update:modelValue', trimmed)
  }
  el.scrollLeft = 0
  emit('commit', trimmed)
}

function onEnter() {
  emit('submit', inputEl.value?.value ?? '')
  inputEl.value?.blur()
}

function onWheel(event: WheelEvent) {
  const el = inputEl.value
  if (!el || document.activeElement !== el) return
  el.scrollLeft += event.deltaY
  event.preventDefault()
}
</script>

<style scoped>
.line-input {
  width: 100%;
  min-width: 0;
  /* 失焦时超长内容省略号截断，聚焦时仍可左右滚动编辑 */
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
