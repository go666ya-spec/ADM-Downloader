import { invoke } from '@tauri-apps/api/core'

export interface ExtensionEntry {
  browser: string
  id: string
}

/** 注册浏览器扩展（写 manifest + 注册表） */
export const registerExtensionHost = (browser: string, id: string) =>
  invoke<void>('extension_register', { browser, id })

export const unregisterExtensionHost = (browser: string) =>
  invoke<void>('extension_unregister', { browser })

export const listExtensions = () => invoke<ExtensionEntry[]>('extension_registered')
