<template>
  <transition name="toast-fade">
    <div v-if="toast.visible" class="toast" role="status">{{ toast.message }}</div>
  </transition>
</template>

<script setup lang="ts">
import { onBeforeUnmount, onMounted } from 'vue'

import { hideToast, toast } from '@/composables/useToast'

/* 任意位置按下鼠标（含窗口内拖动、按钮、空白处）都关闭提示 */
function onPointerDown() {
  hideToast()
}

function onKeydown() {
  hideToast()
}

onMounted(() => {
  document.addEventListener('pointerdown', onPointerDown, true)
  document.addEventListener('keydown', onKeydown, true)
})

onBeforeUnmount(() => {
  document.removeEventListener('pointerdown', onPointerDown, true)
  document.removeEventListener('keydown', onKeydown, true)
})
</script>

<style scoped>
/* 单行小胶囊，贴底居中；不拦截鼠标，避免挡住下面的操作 */
.toast {
  position: fixed;
  /* 列表区域底部居中：内容区中心 = 50vw + 侧边栏一半 */
  bottom: 24px;
  left: calc(50vw + var(--sidebar-width) / 2);
  z-index: 60;
  max-width: min(520px, calc(100vw - 64px));
  padding: 7px 14px;
  overflow: hidden;
  border-radius: 999px;
  background: var(--text);
  color: var(--bg);
  font-size: var(--fs-row);
  line-height: 1.3;
  text-overflow: ellipsis;
  white-space: nowrap;
  pointer-events: none;
  transform: translateX(-50%);
  box-shadow: 0 8px 24px rgb(0 0 0 / 22%);
}

.toast-fade-enter-active,
.toast-fade-leave-active {
  transition: opacity 140ms ease, transform 140ms ease;
}

.toast-fade-enter-from,
.toast-fade-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(4px);
}
</style>
