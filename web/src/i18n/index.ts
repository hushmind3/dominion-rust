import {useSyncExternalStore} from 'react'
import ko, {type LocaleKey} from './ko'
import en from './en'

export type Locale = 'ko' | 'en'
const packs: Record<Locale, Record<LocaleKey, string>> = {ko, en}
const KEY = 'dominion.locale'

let locale: Locale = (() => {
  try {
    const saved = localStorage.getItem(KEY)
    if (saved === 'ko' || saved === 'en') return saved
  } catch {}
  return 'ko'
})()

const listeners = new Set<() => void>()
const subscribe = (fn: () => void) => { listeners.add(fn); return () => { listeners.delete(fn) } }

export function getLocale() { return locale }
export function setLocale(next: Locale) {
  locale = next
  try { localStorage.setItem(KEY, next) } catch {}
  for (const fn of listeners) fn()
}
export function useLocale(): Locale {
  return useSyncExternalStore(subscribe, getLocale, getLocale)
}
export function formatDateTime(value?: string | number | Date | null, options?: Intl.DateTimeFormatOptions): string {
  if (value === undefined || value === null || value === '') return ''
  const date = value instanceof Date ? value : new Date(value)
  if (!Number.isFinite(date.getTime())) return ''
  return date.toLocaleString(locale === 'en' ? 'en-US' : 'ko-KR', options)
}
export function t(key: LocaleKey, vars?: Record<string, string | number>): string {
  const pack = packs[locale] || ko
  let text = pack[key] ?? ko[key] ?? key
  if (vars) for (const [name, value] of Object.entries(vars)) text = text.replaceAll('{' + name + '}', String(value))
  return text
}
export type {LocaleKey}
