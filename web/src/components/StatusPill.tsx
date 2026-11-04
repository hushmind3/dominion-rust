import type {ReactNode} from 'react'

const labels:Record<string,string>={queued:'대기',planning:'계획',running:'실행 중',reviewing:'검수',repairing:'재작업',completed:'완료',failed:'실패',
 needs_review:'검토 필요',budget_exceeded:'예산 초과',cancelled:'취소',pending:'대기',available:'사용 가능',ready:'준비'}
export function StatusPill({status,children}:{status:string;children?:ReactNode}){
 const tone=['completed','ready','available'].includes(status)?'bg-emerald-50 text-emerald-700 ring-emerald-100':
  ['failed','budget_exceeded'].includes(status)?'bg-rose-50 text-rose-700 ring-rose-100':
  ['needs_review','repairing'].includes(status)?'bg-amber-50 text-amber-800 ring-amber-100':
  ['running','reviewing','planning'].includes(status)?'bg-[#F7E8E1] text-[#A94E30] ring-[#F1DCCF]':'bg-[#F3EEE5] text-[#5C544A] ring-slate-200';
 return <span className={'inline-flex items-center gap-1.5 rounded-lg px-2.5 py-1 text-[11px] font-semibold ring-1 ring-inset '+tone}>
  {children??labels[status]??status}
 </span>
}
