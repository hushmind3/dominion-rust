import type {ReactNode} from 'react'
import {t,useLocale,type LocaleKey} from '../i18n'

const labels:Record<string,LocaleKey>={queued:'st.queued',planning:'st.planning',running:'st.running',reviewing:'st.reviewing',repairing:'st.repairing',completed:'st.completed',failed:'st.failed',needs_review:'st.needs_review',budget_exceeded:'st.budget_exceeded',cancelled:'st.cancelled',pending:'st.pending',available:'st.available',ready:'st.ready',answered_unverified:'st.answered_unverified',answered_verified:'st.answered_verified'}
export function StatusPill({status,children}:{status:string;children?:ReactNode}){
 useLocale()
 const tone=['completed','ready','available'].includes(status)?'bg-emerald-50 text-emerald-700 ring-emerald-100':
  ['failed','budget_exceeded'].includes(status)?'bg-rose-50 text-rose-700 ring-rose-100':
  ['needs_review','repairing'].includes(status)?'bg-amber-50 text-amber-800 ring-amber-100':
  ['running','reviewing','planning'].includes(status)?'bg-[#F7E8E1] text-[#A94E30] ring-[#F1DCCF]':'bg-[#F3EEE5] text-[#5C544A] ring-[#E0D8CC]';
 return <span className={'inline-flex items-center gap-1.5 rounded-lg px-2.5 py-1 text-[11px] font-semibold ring-1 ring-inset '+tone}>
  {children??(labels[status]?t(labels[status]):status)}
 </span>
}

export function statusLabel(status?: string): string {
  if (!status) return ''
  return labels[status] ? t(labels[status]) : status
}
