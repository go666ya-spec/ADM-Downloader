import { createRouter, createWebHashHistory } from 'vue-router'

/**
 * hash 模式：浏览器预览与打包后的本地资源协议都能直接工作，
 * 不需要服务端 history fallback。
 */
const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: '/', redirect: '/dashboard' },
    {
      path: '/dashboard',
      name: 'dashboard',
      component: () => import('@/views/DashboardView.vue'),
      meta: { title: '仪表盘' },
    },
    {
      path: '/downloads',
      name: 'downloads',
      component: () => import('@/views/DownloadsView.vue'),
      meta: { title: '下载' },
    },
    {
      path: '/preferences',
      name: 'preferences',
      component: () => import('@/views/PreferencesView.vue'),
      meta: { title: '偏好设置' },
    },
    {
      path: '/appearance',
      name: 'appearance',
      component: () => import('@/views/AppearanceView.vue'),
      meta: { title: '外观' },
    },
  ],
})

export default router
