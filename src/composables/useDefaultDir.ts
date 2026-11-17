import { onMounted, ref } from 'vue'

import { ensureSettings } from '@/stores/settings'

/** 新任务保存目录：以偏好设置为准，取不到时留空由后端兜底 */
const defaultDir = ref('')

export function useDefaultDir() {
  onMounted(async () => {
    try {
      const settings = await ensureSettings()
      defaultDir.value = settings.saveDir
    } catch {
      defaultDir.value = ''
    }
  })

  return defaultDir
}
