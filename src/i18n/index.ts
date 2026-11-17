import { reactive } from 'vue'

import { ensureSettings, patchSettings } from '@/stores/settings'

export type LocaleCode = 'zh-cn' | 'zh-tw' | 'en' | 'ko' | 'ja'

type Pack = Record<string, string>

/** 各语言包都是独立 chunk，按需 import */
const loaders: Record<LocaleCode, () => Promise<{ default: Pack }>> = {
  'zh-cn': () => import('./locales/zh-cn'),
  'zh-tw': () => import('./locales/zh-tw'),
  en: () => import('./locales/en'),
  ko: () => import('./locales/ko'),
  ja: () => import('./locales/ja'),
}

export const AVAILABLE_LOCALES: LocaleCode[] = ['zh-cn', 'zh-tw', 'en', 'ko', 'ja']

const state = reactive({
  /** 设置里保存的值，可能是 system */
  preference: 'system',
  /** 当前实际使用的语言 */
  locale: 'zh-cn' as LocaleCode,
  pack: {} as Pack,
  fallback: {} as Pack,
})

/** 取文案；缺失时退回简体中文，再缺就返回 key 本身 */
export function t(key: string): string {
  return state.pack[key] ?? state.fallback[key] ?? key
}

export function currentLocale(): LocaleCode {
  return state.locale
}

export function localePreference(): string {
  return state.preference
}

/** 把浏览器语言映射到支持的语言 */
function detect(): LocaleCode {
  const raw = (navigator.language || 'zh-CN').toLowerCase()
  if (raw.startsWith('zh-tw') || raw.startsWith('zh-hk')) return 'zh-tw'
  if (raw.startsWith('zh')) return 'zh-cn'
  if (raw.startsWith('ja')) return 'ja'
  if (raw.startsWith('ko')) return 'ko'
  if (raw.startsWith('en')) return 'en'
  return 'zh-cn'
}

function normalize(preference: string): LocaleCode {
  if (preference === 'system') return detect()
  return (AVAILABLE_LOCALES as string[]).includes(preference)
    ? (preference as LocaleCode)
    : 'zh-cn'
}

async function loadPack(code: LocaleCode): Promise<Pack> {
  try {
    return (await loaders[code]()).default
  } catch {
    return {}
  }
}

/** 切换语言：只加载目标语言包，不预加载其他语言 */
export async function applyLocale(preference: string) {
  const code = normalize(preference)
  if (state.locale === code && Object.keys(state.pack).length > 0) {
    state.preference = preference
    return
  }

  if (Object.keys(state.fallback).length === 0) {
    state.fallback = await loadPack('zh-cn')
  }

  state.pack = code === 'zh-cn' ? state.fallback : await loadPack(code)
  state.locale = code
  state.preference = preference
  document.documentElement.lang = code
}

/** 启动时初始化：只读设置里的语言 */
export async function initLocale() {
  let preference = 'system'
  try {
    preference = (await ensureSettings()).language || 'system'
  } catch {
    /* 浏览器预览下没有 Tauri 命令 */
  }
  await applyLocale(preference)
}

/** 用户切换语言：加载并写回配置 */
export async function setLanguage(preference: string) {
  await applyLocale(preference)
  try {
    await ensureSettings()
    patchSettings({ language: preference })
  } catch {
    /* 浏览器预览下没有 Tauri 命令 */
  }
}
