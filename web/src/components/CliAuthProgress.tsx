import {useState} from 'react'
import {Button,TextInput} from './Primitives'
import {KeyRound,LogIn,RotateCcw,X} from 'lucide-react'

export type CliAuthStatus={
 source:string
 status:string
 message:string
 url?:string|null
 requiresInput?:boolean
}
type Props={
 session:CliAuthStatus
 onInput:(value:string)=>Promise<void>
 onCancel:()=>Promise<void>
}

/** Browser authorization progress stays inside DOMINION, even when CLI asks for a code/menu. */
export function CliAuthProgress({session,onInput,onCancel}:Props){
 const [code,setCode]=useState('')
 const active=!['completed','failed','cancelled'].includes(session.status)
 const title:Record<string,string>={claude:'Claude Code',gemini:'Gemini CLI',opencode:'OpenCode'}
 return <section className="mt-4 min-w-0 rounded-xl border border-[#D7D9E9] bg-[#F7F8FC] p-4" aria-live="polite">
  <div className="flex flex-wrap items-start justify-between gap-2">
   <div className="min-w-0">
    <strong className="block text-[13px] text-[#363650]">{title[session.source]||session.source} · 인증 진행</strong>
    <p className="mt-1 text-[12px] leading-5 text-[#626278]">{session.message}</p>
   </div>
   {active&&<Button size="sm" variant="ghost" onClick={()=>void onCancel()}><X size={13}/> 취소</Button>}
  </div>
  {session.url&&<a href={session.url} target="_blank" rel="noopener noreferrer"
    className="mt-3 inline-flex min-h-9 items-center gap-2 rounded-lg bg-[#C96442] px-3 text-[12px] font-semibold text-white hover:bg-[#A94E30]"><LogIn size={14}/> 공식 로그인 페이지 열기</a>}
  {active&&<div className="mt-3 flex flex-wrap items-center gap-2 border-t border-[#E2E2EE] pt-3">
   <Button size="sm" variant="secondary" onClick={()=>void onInput('enter')}><RotateCcw size={13}/> 선택 / 계속</Button>
   <Button size="sm" variant="ghost" onClick={()=>void onInput('down')}>다음 항목</Button>
   <div className="flex min-w-[180px] flex-1 items-center gap-2">
    <TextInput aria-label="인증 코드 입력" value={code} onChange={e=>setCode(e.target.value)} placeholder="인증 코드가 요청된 경우 입력" className="!py-1.5 !text-[12px]"/>
    <Button size="sm" variant="secondary" disabled={!code.trim()} onClick={()=>{void onInput(code.trim());setCode('')}}><KeyRound size={13}/> 제출</Button>
   </div>
  </div>}
  <p className="mt-2 text-[11px] leading-5 text-[#8D8D9B]">공식 CLI가 브라우저 인증을 처리합니다. 해당 계정의 OAuth 토큰은 CLI에 보관되며 DOMINION 모델 제공자에는 자동 복사되지 않습니다.</p>
 </section>
}
