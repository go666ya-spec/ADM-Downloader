<template>
  <div class="page-header">
    <div class="page-header__text">
      <h1 class="page-header__title">{{ title }}</h1>
      <p v-if="subtitle" class="page-header__subtitle">
        {{ subtitle }}
        <span v-if="note" class="page-header__note">{{ note }}</span>
      </p>
    </div>
    <div class="page-header__actions">
      <slot name="actions" />
    </div>
  </div>
</template>

<script setup lang="ts">
defineProps<{ title: string; subtitle?: string; note?: string }>()
</script>

<style scoped>
.page-header {
  display: flex;
  align-items: flex-end;
  justify-content: space-between;
  gap: 16px;
  padding: 14px 4px 10px;
}

.page-header__title {
  margin: 0;
  font-size: var(--fs-page);
  font-weight: 700;
  letter-spacing: -0.01em;
  color: var(--text);
}

.page-header__subtitle {
  margin: 6px 0 0;
  font-size: var(--fs-row);
  color: var(--text-muted);
}

/* 副标题后面的行内提示，与正文间隔 4 个字符 */
.page-header__note {
  margin-left: 4ch;
  color: var(--danger);
}

.page-header__actions {
  display: flex;
  align-items: center;
  gap: 8px;
}
</style>
