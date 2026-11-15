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

const jobKinds = new Set(['learn','room','goal','workflow','reflect','resume','video','build','setup','test','commit','package','connect','upload'])
export function jobKindLabel(kind?: string): string {
  if (!kind) return ''
  return jobKinds.has(kind) ? t(('jobk.' + kind) as LocaleKey) : kind
}

const auditEvents = new Set(['model.usage','job.completed','job.queued','job.failed','learning.completed','reflection.history_trimmed','provider.saved','room.owner_message','room.exchange_finished','room.knowledge_learned','model.downloaded','algorithm.activated','algorithm.statistics_rebuilt','mcp.list_failed','migration.algorithm_stat_rebuild','backup.created','backup.encrypted','workflow.completed','goal.finished','execution.started','execution.finished'])
export function auditEventLabel(event?: string): string {
  if (!event) return ''
  return auditEvents.has(event) ? t(('evt.' + event) as LocaleKey) : event
}
