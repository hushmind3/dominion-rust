import {useEffect,useState} from 'react'
import {Wrench,X} from 'lucide-react'
import {get,post} from '../api'
import {Button} from './Primitives'

type Signal={id:string;evidenceFingerprint:string;kind:string;title:string;detail:string;evidence?:{status?:string}[]}
export async function openImprovementIssue(body:{signalId:string}|{sessionId:string;description:string}){
 const issue=await post<{id:string}>('/api/rsi/issues',body)
 window.location.hash='#/rsi?issue='+encodeURIComponent(issue.id)
}

export function ImprovementNotice(){
 const [signal,setSignal]=useState<Signal|null>(null),[busy,setBusy]=useState(false),[error,setError]=useState('')
 useEffect(()=>{
  let alive=true
  const check=async()=>{
   try{
    const {signals,recommendationReady}=await get<{signals:Signal[];recommendationReady:boolean}>('/api/rsi/signals')
    if(!alive)return
    const quietUntil=Number(localStorage.getItem('dominion.improvement.quietUntil')||0)
    const focused=document.activeElement as HTMLInputElement|null
    const editing=focused?.matches('input,textarea,[contenteditable=true]')&&!!(focused.value||focused.textContent||'').trim()
    if(!recommendationReady||Date.now()<quietUntil||editing){setSignal(null);return}
    const dismissed: string[]=JSON.parse(localStorage.getItem('dominion.improvement.dismissed')||'[]')
    setSignal(signals.find(s=>(['rsi_no_edit','rsi_no_candidate','rsi_token_waste'].includes(s.kind)||(['execution_failures','failed_jobs'].includes(s.kind)&&(s.evidence?.length||0)>=2))&&(s.kind!=='failed_jobs'||s.evidence?.some(row=>row.status==='failed'))&&!dismissed.includes(s.evidenceFingerprint))||null)
   }catch{/* An unavailable server should not add a second error alert. */}
  }
  const timer=window.setInterval(()=>{if(document.visibilityState==='visible')void check()},15000)
  return()=>{alive=false;clearInterval(timer)}
 },[])
 const dismiss=()=>{
  if(signal)try{const old=JSON.parse(localStorage.getItem('dominion.improvement.dismissed')||'[]');localStorage.setItem('dominion.improvement.dismissed',JSON.stringify([...old,signal.evidenceFingerprint].slice(-100)));localStorage.setItem('dominion.improvement.quietUntil',String(Date.now()+24*60*60*1000))}catch{}
  setSignal(null)
 }
 if(!signal||window.location.hash.startsWith('#/rsi'))return null
 return <aside role="status" aria-live="polite" className="fixed bottom-5 right-5 z-50 w-[min(360px,calc(100vw-40px))] rounded-2xl border border-[#E7DDCC] bg-white p-4 shadow-lg">
  <div className="flex items-start gap-3"><Wrench size={18} className="mt-0.5 shrink-0 text-[#A94E30]"/><div className="min-w-0 flex-1"><p className="text-[13px] font-semibold">자기개선 한 번 해볼까요?</p><p className="mt-1 text-[12px] leading-5 text-[#7C7469]">최근 작업에서 개선할 점을 발견했습니다. 쌓인 경험을 바탕으로 더 나은 작업 방식을 찾아볼 수 있습니다.</p></div><button type="button" aria-label="나중에 확인" onClick={dismiss} className="rounded p-1 text-[#7C7469] hover:bg-[#FAF7F1]"><X size={15}/></button></div>
  <div className="mt-3 flex justify-end gap-2"><Button variant="ghost" onClick={dismiss}>나중에</Button><Button variant="primary" disabled={busy} onClick={async()=>{setBusy(true);setError('');try{await openImprovementIssue({signalId:signal.id});dismiss()}catch(e){setError((e as Error).message)}finally{setBusy(false)}}}>{busy?'기록 준비 중…':'개선 제안 보기'}</Button></div>
  {error&&<p role="alert" className="mt-2 text-xs text-rose-700">{error}</p>}
 </aside>
}
