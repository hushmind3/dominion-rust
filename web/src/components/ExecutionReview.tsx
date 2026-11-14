import {useEffect,useState} from 'react'
import {get,post} from '../api'
import {Button} from './Primitives'
import {StatusPill} from './StatusPill'
import {useRemote} from './UI'
type Goal={id:string;goal:string;status:string;createdAt?:string;jobId?:string;sessionId?:string;finalReport?:{accepted?:boolean;gaps?:string[]};gitWorktree?:{applied?:boolean;rolledBack?:boolean;path?:string};isolateWorktree?:boolean}
type Changes={goalId:string;status:string;baseCommit:string;diff:string;untracked?:string;untrackedPreviews?:{path:string;bytes:number;content?:string;previewOmitted?:boolean}[];reviewDigest:string;reviewIncomplete?:boolean;truncated?:boolean}
const inProgress=(s:string)=>s==='queued'||s==='running'
/** An operations drawer, not an independent /goal product page. All external
 * effects still require the existing server-side digest and owner approval. */
export function ExecutionReview(){
 const goals=useRemote<Goal[]>('/api/goals',[])
 const [expanded,setExpanded]=useState<string|null>(null)
 const [diff,setDiff]=useState<Changes|null>(null)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 useEffect(()=>{
  const id=window.setInterval(()=>{if(document.visibilityState==='visible')void goals.reload()},15000)
  return()=>window.clearInterval(id)
 },[goals.reload])
 const selected=goals.value.find(g=>g.id===expanded)
 const pending=[...goals.value].filter(g=>inProgress(g.status)
   ||g.status==='needs_review'||g.status==='failed'
   ||g.gitWorktree&&!g.gitWorktree.applied&&!g.gitWorktree.rolledBack)
   .sort((a,b)=>String(b.createdAt||'').localeCompare(String(a.createdAt||'')))
 const mutate=async(action:'cancel'|'resume'|'discard'|'apply')=>{
  if(!selected||busy)return
  const prompt=action==='resume'?'실제 파일·원격 작업의 중복 여부를 직접 확인하셨습니까? 확인되지 않은 작업은 자동 재실행하지 않습니다.':
   action==='discard'?'격리 작업공간의 아직 반영하지 않은 변경을 폐기하시겠습니까?':
   action==='apply'?'표시된 전체 변경을 검토했고 원본 브랜치에 반영하시겠습니까?':'실행을 중단하시겠습니까?'
  if(!window.confirm(prompt))return
  if(action==='apply'&&(!diff||diff.goalId!==selected.id||diff.truncated||diff.reviewIncomplete)){
   setError('전체 변경 내용을 먼저 검토해야 합니다. 생략된 diff는 적용할 수 없습니다.');return
  }
  setBusy(true);setError('');setNotice('')
  try{
   const body=action==='resume'?{acknowledgeUnknownEffects:true}:
    action==='apply'?{reviewDigest:diff!.reviewDigest,acknowledgeReview:true}:{}
   await post('/api/goals/'+encodeURIComponent(selected.id)+'/'+action,body)
   setDiff(null);await goals.reload();setNotice('작업 상태가 저장됐습니다.')
  }catch(e){setError((e as Error).message);setDiff(null)}finally{setBusy(false)}
 }
 const inspect=async()=>{
  if(!selected||busy)return
  setBusy(true);setDiff(null);setError('')
  try{setDiff(await get<Changes>('/api/goals/'+encodeURIComponent(selected.id)+'/changes'))}
  catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 return <section className="mb-5 rounded-xl border border-[#E7E0D5] bg-white p-4">
  <div className="flex flex-wrap items-center justify-between gap-2">
   <div><h2 className="text-[13px] font-bold">장기 작업 · 중단·검토·복구</h2>
    <p className="mt-1 text-[11px] text-[#8A8176]">채팅 /goal 또는 에이전트/개발자 작업의 안전한 운영 기록입니다. 별도 Goal 메뉴는 없습니다.</p></div>
   <Button variant="secondary" onClick={()=>void goals.reload()}>기록 새로고침</Button>
  </div>
  {error&&<p role="alert" className="mt-3 text-xs text-rose-700">{error}</p>}
  {notice&&<p role="status" className="mt-3 text-xs text-emerald-700">{notice}</p>}
  {pending.length===0?<p className="mt-3 text-xs text-[#8A8176]">검토 또는 재개할 작업이 없습니다.</p>:
   <div className="mt-3 grid gap-2">{pending.slice(0,24).map(g=>
    <div key={g.id} className="rounded-lg border border-[#EEE5D9] p-3 text-xs">
     <button className="flex w-full items-start justify-between gap-3 text-left" onClick={()=>{setExpanded(expanded===g.id?null:g.id);setDiff(null);setError('')}} aria-expanded={expanded===g.id}>
      <span className="min-w-0"><strong className="block break-words">{g.goal}</strong><small className="text-[#8A8176]">{g.id.slice(0,15)}</small></span>
      <StatusPill status={g.status}/>
     </button>
     {expanded===g.id&&<div className="mt-3 space-y-3 border-t border-[#EEE5D9] pt-3">
      <p className="text-[#7C7469]">독립 검토: {g.finalReport?.accepted===true?'내부 통과':'통과 미확인'} · 원본 반영: {g.gitWorktree?.applied?'예':'아니오'}</p>
      {g.finalReport?.gaps?.map((gap,i)=><p key={i} className="text-amber-700">{gap}</p>)}
      <div className="flex flex-wrap gap-2">
       {inProgress(g.status)&&<Button variant="secondary" disabled={busy} onClick={()=>void mutate('cancel')}>작업 중단</Button>}
       {(g.status==='needs_review'||g.status==='failed')&&<Button variant="secondary" disabled={busy} onClick={()=>void mutate('resume')}>외부 영향 확인 후 재개</Button>}
       {g.gitWorktree&&!g.gitWorktree.applied&&!g.gitWorktree.rolledBack&&<>
        <Button variant="secondary" disabled={busy} onClick={()=>void inspect()}>Git 변경 전체 검토</Button>
        <Button variant="secondary" disabled={busy} onClick={()=>void mutate('discard')}>격리 변경 폐기</Button>
       </>}
       {g.sessionId&&<a href={'#/chat?session='+encodeURIComponent(g.sessionId)} className="self-center font-semibold text-[#C96442] hover:underline">실행 대화 보기</a>}
      </div>
      {diff&&diff.goalId===g.id&&<div className="space-y-2 rounded-lg bg-[#FAF7F1] p-3">
       <p className="text-[#8A8176]">Git 기준 {diff.baseCommit.slice(0,12)} · 영수증 {diff.reviewDigest.slice(0,16)}</p>
       <pre className="max-h-80 overflow-auto whitespace-pre-wrap break-all text-[11px]">{diff.diff}</pre>
       {diff.untracked&&<details><summary>추적되지 않은 파일</summary><pre className="whitespace-pre-wrap break-all">{diff.untracked}</pre></details>}
       {diff.untrackedPreviews?.map((x,i)=><details key={i}><summary>{x.path} · {x.bytes}바이트</summary><pre className="max-h-60 overflow-auto whitespace-pre-wrap break-all">{x.content||'[내용 미제공]'}</pre></details>)}
       {(diff.truncated||diff.reviewIncomplete)?<p className="text-amber-700">변경 내용이 일부 생략돼 승인이 차단됐습니다.</p>:
        !inProgress(g.status)&&['completed','needs_review'].includes(g.status)&&
        <Button disabled={busy} onClick={()=>void mutate('apply')}>검토한 원본 Git 변경 승인 적용</Button>}
      </div>}
     </div>}
    </div>)}</div>}
 </section>
}
