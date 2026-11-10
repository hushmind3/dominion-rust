import type {ReactNode} from 'react'

type Props={
 title:string
 status:string
 statusTone:'connected'|'available'|'disconnected'
 accountId?:string
 model?:string
 hint?:string
 children:ReactNode
}

/** Shared card for OAuth, CLI and API-key connections: content never competes with action buttons for width. */
export function OAuthConnectionCard({title,status,statusTone,accountId,model,hint,children}:Props){
 return <article className="flex min-w-0 flex-col rounded-xl border border-[#EFE9DF] bg-[#FAF7F1] p-4">
  <div className="min-w-0">
   <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1.5">
    <h3 className="m-0 min-w-0 break-words text-[13px] font-bold leading-5 text-[#3A342D]">{title}</h3>
    <span role="status" className={'inline-flex shrink-0 items-center rounded-md px-2 py-1 text-[10px] font-semibold '+(statusTone==='connected'?'bg-emerald-100 text-emerald-800':statusTone==='available'?'bg-amber-100 text-amber-800':'bg-[#EDE8DE] text-[#7C7469]')}>{status}</span>
   </div>
   {accountId&&<p className="mt-2 break-all rounded-lg bg-white px-2.5 py-2 text-[10px] text-[#8A8176]">계정 · {accountId}</p>}
   {model&&<p className="mt-2 break-words text-[11px] text-[#8A8176]">기본 모델 · {model}</p>}
   {hint&&<p className="mt-2 text-[11px] leading-5 text-[#7C7469]">{hint}</p>}
  </div>
  <div className="mt-auto flex flex-wrap items-center gap-2 border-t border-[#E7E0D5] pt-3">{children}</div>
 </article>
}
