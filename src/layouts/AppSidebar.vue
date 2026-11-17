<template>
  <aside class="sidebar">
    <nav class="sidebar__nav">
      <RouterLink
        v-for="item in primaryNav"
        :key="item.name"
        :to="item.to"
        class="nav-link"
        active-class="nav-link--active"
      >
        <AppIcon :name="item.icon" class="nav-link__icon" />
        <span class="nav-link__label">{{ t(item.key) }}</span>
      </RouterLink>
    </nav>

    <div class="sidebar__footer">
      <RouterLink to="/appearance" class="nav-link" active-class="nav-link--active">
        <AppIcon name="settings" class="nav-link__icon" />
        <span class="nav-link__label">{{ t('nav.settings') }}</span>
      </RouterLink>
    </div>
  </aside>
</template>

<script setup lang="ts">
import AppIcon from '@/components/AppIcon.vue'
import { t } from '@/i18n'
import type { IconName } from '@/components/icons'

interface NavItem {
  name: string
  key: string
  to: string
  icon: IconName
}

const primaryNav: NavItem[] = [
  { name: 'dashboard', key: 'nav.dashboard', to: '/dashboard', icon: 'dashboard' },
  { name: 'downloads', key: 'nav.downloads', to: '/downloads', icon: 'download' },
  { name: 'preferences', key: 'nav.preferences', to: '/preferences', icon: 'tune' },
]
</script>

<style scoped>
.sidebar {
  display: flex;
  flex-direction: column;
  width: var(--sidebar-width);
  flex: 0 0 var(--sidebar-width);
  /* 顶部整体下移 30px，与标题栏留出呼吸感 */
  padding: 36px 12px 12px;
  background: var(--sidebar);
}

.sidebar__nav {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.sidebar__footer {
  margin-top: auto;
  padding-top: 8px;
}

.nav-link {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 40px;
  padding: 0 12px;
  border: none;
  border-radius: var(--control-radius);
  background: transparent;
  color: var(--text-secondary);
  text-decoration: none;
  font-family: inherit;
  font-size: 13.5px;
  line-height: 16px;
  text-align: left;
  cursor: default;
  transition: background-color 120ms ease, color 120ms ease;
}

.nav-link:hover {
  background: var(--card-hover);
  color: var(--text);
}

.nav-link--active {
  background: var(--sidebar-selected);
  color: var(--text);
  font-weight: 700;
}

.nav-link__icon {
  display: block;
  width: 16px;
  height: 16px;
}

.nav-link__label {
  line-height: 16px;
}
</style>
