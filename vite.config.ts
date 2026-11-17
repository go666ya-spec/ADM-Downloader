import { fileURLToPath, URL } from 'node:url'

import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import AutoImport from 'unplugin-auto-import/vite'
import Components from 'unplugin-vue-components/vite'
import { ElementPlusResolver } from 'unplugin-vue-components/resolvers'

// Tauri 约定端口 1420；浏览器预览阶段同样是 1420，方便后面无缝切到 tauri dev
export default defineConfig({
  plugins: [
    vue(),
    AutoImport({
      resolvers: [ElementPlusResolver()],
      dts: 'src/types/auto-imports.d.ts',
    }),
    Components({
      // 自有组件全部显式 import，这里只负责 Element Plus 的按需注册
      dirs: [],
      resolvers: [ElementPlusResolver({ importStyle: 'css' })],
      dts: 'src/types/components.d.ts',
    }),
  ],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  clearScreen: false,
  build: {
    rollupOptions: {
      // 两个独立入口：主界面 index.html、浏览器接管弹窗 popup.html
      input: {
        main: fileURLToPath(new URL('./index.html', import.meta.url)),
        popup: fileURLToPath(new URL('./popup.html', import.meta.url)),
      },
    },
  },
  server: {
    // 固定端口，Tauri 的 devUrl 也指向这里
    port: 5688,
    strictPort: true,
    watch: {
      // src-tauri 的改动交给 cargo 自己监听
      ignored: ['**/src-tauri/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
})
