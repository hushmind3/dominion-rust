import {useEffect,useState} from 'react'
import {Bot,Check,ClipboardList,ExternalLink,LoaderCircle,PauseCircle,RefreshCw,Send,ShieldCheck} from 'lucide-react'
import {get,post} from '../api'
import {Button,TextArea} from '../components/Primitives'
import {Headline,Notice,useRemote} from '../components/UI'
import {StatusPill} from '../components/StatusPill'

type Artifact={path:string;sha256:string;bytes:number;sourceTool:string}
type Goal={id:string;goal:string;status:string;phase?:string;plan?:string;
 planRevision?:number;appliedPlanRevision?:number;ownerPlan?:{revision:number;plan:string};
 planHistory?:{version:number;plan:string;reason:string}[];
 reviewHistory?:{iteration:number;complete:boolean;gaps:string[]}[];
 artifacts?:Artifact[];gaps?:string[];iterations?:number;maxIterations?:number;
 sessionId?:string;createdAt?:string}
type Job={id:string;kind:string;status:string;input?:{goalId?:string};
 stepIndex?:number;stepTotal?:number}

const active=(status:string)=>status==='queued'||status==='running'
export function AutonomousGoalsPage({openSession}:{openSession:(id:string)=>void}){
 const goals=useRemote<Goal[]>('/api/goals',[])
 const jobs=useRemote<Job[]>('/api/jobs',[])
 const [objective,setObjective]=useState('')
 const [drafts,setDrafts]=useState<Record<string,string>>({})
 const [busy,setBusy]=useState(''),[error,setError]=useState(''),[notice,setNotice]=useState('')
 const reload=async()=>{await Promise.all([goals.reload(),jobs.reload()])}
 // SSE refreshes the records; polling fills in the gap after a tab reconnect.
 useEffect(()=>{
  const timer=window.setInterval(()=>{
   if(document.visibilityState==='visible'&&goals.value.some(goal=>active(goal.status)))
    void reload()
  },6000)
  return ()=>window.clearInterval(timer)
 },[goals.value,goals.reload,jobs.reload])
 const create=async()=>{
  if(!objective.trim()||busy)return
  setBusy('new');setError('');setNotice('')
  try{
   const created=await post<Goal>('/api/goals',{goal:objective.trim()})
   setObjective('');setNotice('자율 작업을 등록했습니다. 계획부터 실행·검증까지 작업 기록에 남습니다.')
   await reload()
   if(created.sessionId)openSession(created.sessionId)
  }catch(e){setError((e as Error).message)}finally{setBusy('')}
 }
 const steer=async(goal:Goal)=>{
  if(busy)return
  const plan=(drafts[goal.id]??goal.plan??'').trim()
  if(!plan)return
  setBusy(goal.id);setError('');setNotice('')
  try{
   await post('/api/goals/'+encodeURIComponent(goal.id)+'/steer',{
    plan,expectedRevision:goal.planRevision||0
   })
   setNotice('계획 변경이 접수됐습니다. 다음 안전한 실행 단계부터 적용됩니다.')
   setDrafts(prev=>{const next={...prev};delete next[goal.id];return next})
   await reload()
  }catch(e){setError((e as Error).message);await reload()}finally{setBusy('')}
 }
 const cancel=async(goal:Goal)=>{
  if(busy)return
  setBusy(goal.id);setError('');setNotice('')
  try{await post('/api/goals/'+encodeURIComponent(goal.id)+'/cancel',{})
   setNotice('목표 중지를 요청했습니다.');await reload()
  }catch(e){setError((e as Error).message)}finally{setBusy('')}
 }
 const ordered=[...goals.value].sort((a,b)=>(b.createdAt||'').localeCompare(a.createdAt||''))
 return <div className="mx-auto max-w-[1320px] space-y-5 px-5 py-5 sm:px-7 lg:px-6">
  <Headline eyebrow="AUTONOMOUS AGENT" title="🤖 자율 목표 에이전트"
   description="목표를 입력하면 프로젝트 탐색 → 계획 → 도구 실행 → 독립 검토 → 실패 수정으로 진행합니다. 실행 중 계획을 직접 바꿀 수 있습니다."/>
  <section className="rounded-2xl border border-[#E7E0D5] bg-white p-5">
   <label htmlFor="dominion-objective" className="mb-2 block text-sm font-semibold">완수할 작업</label>
   <TextArea id="dominion-objective" rows={3} value={objective}
    onChange={event=>setObjective(event.target.value)}
    placeholder="예: 현재 프로젝트의 로그인 오류를 조사하고, 관련 소스를 수정한 뒤 검증 증거와 변경 파일을 정리해줘"/>
   <div className="mt-3 flex flex-wrap items-center justify-between gap-3">
    <p className="text-xs text-[#7C7469]">실제 파일 수정·명령·외부 서비스 작업은 기존 도구 권한과 승인 정책을 따릅니다.</p>
    <Button variant="primary" disabled={busy!==''||!objective.trim()} onClick={()=>void create()}>
     {busy==='new'?<LoaderCircle size={15} className="animate-spin"/>:<Send size={15}/>} 작업 시작
    </Button>
   </div>
  </section>
  {error&&<Notice message={error}/>}
  {notice&&<p role="status" className="rounded-xl bg-[#F7F3EC] px-4 py-3 text-sm text-[#5C544A]">{notice}</p>}
  <div className="flex items-center justify-between"><h2 className="text-base font-bold">실행 기록</h2>
   <Button variant="ghost" size="sm" onClick={()=>void reload()}><RefreshCw size={14}/> 새로고침</Button></div>
  {ordered.length===0&&<p className="rounded-2xl border border-[#E7E0D5] p-6 text-center text-sm text-[#7C7469]">등록된 자율 목표가 없습니다.</p>}
  <div className="grid gap-4">{ordered.slice(0,30).map(goal=>{
   const job=jobs.value.find(row=>row.kind==='goal'&&row.input?.goalId===goal.id)
   const revision=goal.planRevision||0,applied=goal.appliedPlanRevision||0
   const editor=drafts[goal.id]??goal.ownerPlan?.plan??goal.plan??''
   return <section key={goal.id} className="rounded-2xl border border-[#E7E0D5] bg-white p-5">
    <div className="flex flex-wrap items-start justify-between gap-3">
     <div className="min-w-0 flex-1"><h3 className="break-words text-[15px] font-bold">{goal.goal}</h3>
      <p className="mt-1 text-xs text-[#7C7469]">단계: {goal.phase||goal.status} · {goal.iterations||0}/{goal.maxIterations||16}회 · 계획 버전 {revision}</p>
     </div><StatusPill status={goal.status}>{goal.status}</StatusPill>
    </div>
    {active(goal.status)&&<div className="mt-3 h-1.5 overflow-hidden rounded-full bg-[#F1ECE4]">
     <div className="h-full bg-[#C96442]" style={{width:Math.min(100,100*(goal.iterations||0)/(goal.maxIterations||16))+'%'}}/>
    </div>}
    {job&&<p className="mt-2 text-[11px] text-[#7C7469]">작업 ID: {job.id} · 상태 {job.status}</p>}
    {goal.gaps&&goal.gaps.length>0&&<details className="mt-3 rounded-xl bg-[#FFF7F1] p-3 text-xs">
     <summary className="cursor-pointer font-semibold">검토·수정이 필요한 항목 ({goal.gaps.length})</summary>
     <ul className="mt-2 list-disc space-y-1 pl-5">{goal.gaps.slice(0,12).map((gap,index)=><li key={index}>{gap}</li>)}</ul>
    </details>}
    <details className="mt-3 rounded-xl border border-[#EFE9DF] p-3" open={active(goal.status)}>
     <summary className="cursor-pointer text-xs font-semibold"><ClipboardList size={14} className="mr-1 inline"/> 실행 계획 {revision>applied&&'· 사용자 변경 적용 대기'}</summary>
     <div className="mt-3">
      <TextArea rows={5} value={editor} disabled={!active(goal.status)||busy===goal.id}
       onChange={event=>setDrafts(prev=>({...prev,[goal.id]:event.target.value}))}
       placeholder="계획 수립 중입니다. 이곳에서 직접 계획을 지정할 수 있습니다."/>
      {active(goal.status)&&<div className="mt-2 flex flex-wrap items-center justify-end gap-2">
       <Button size="sm" variant="ghost" disabled={busy!==''}
        onClick={()=>void cancel(goal)}><PauseCircle size={14}/> 중지</Button>
       <Button size="sm" variant="primary" disabled={busy!==''||!editor.trim()||editor.trim()===(goal.ownerPlan?.plan||goal.plan||'').trim()}
        onClick={()=>void steer(goal)}><Check size={14}/> 수정 계획 적용</Button>
      </div>}
     </div>
    </details>
    {(goal.artifacts?.length||0)>0&&<details className="mt-3 rounded-xl bg-[#FAF7F1] p-3">
     <summary className="cursor-pointer text-xs font-semibold"><ShieldCheck size={13} className="mr-1 inline"/> 실제 수정 파일 ({goal.artifacts!.length})</summary>
     <div className="mt-2 space-y-2">{goal.artifacts!.map(file=><div key={file.path} className="text-xs">
      <strong className="break-all">{file.path}</strong><p className="break-all font-mono text-[10px] text-[#7C7469]" title={file.sha256}>SHA-256 {file.sha256.slice(0,20)}… · {file.bytes.toLocaleString()} bytes</p>
     </div>)}</div><p className="mt-2 text-[11px] text-[#7C7469]">파일 해시는 무결성 기록입니다. 테스트 통과를 의미하지 않습니다.</p>
    </details>}
    <div className="mt-3 flex flex-wrap items-center gap-2 text-xs">
     {goal.sessionId&&<Button size="sm" variant="ghost" onClick={()=>openSession(goal.sessionId!)}><Bot size={14}/> 작업 대화 열기</Button>}
     <a href="#/developer" className="inline-flex items-center gap-1 text-[#A94E30] hover:underline"><ExternalLink size={13}/> 개발자 diff 확인</a>
     <span className="ml-auto text-[#7C7469]">재계획·검증 기록 {goal.reviewHistory?.length||0}회</span>
    </div>
   </section>
  })}</div>
 </div>
}
