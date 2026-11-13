import {useEffect,useState} from 'react'
import {ArrowRight,Check,Play,RefreshCw,Square,Target} from 'lucide-react'
import {post} from '../../api'
import {Button,SelectInput,TextArea,TextInput} from '../../components/Primitives'
import {StatusPill} from '../../components/StatusPill'
import {FormField,Notice,Panel,Subheading,useRemote} from '../../components/UI'

type Agent={id:string;name:string;model?:string;providerId?:string}
type Goal={
 id:string;jobId?:string;goal:string;status:string;phase?:string;plan?:string;
 iterations?:number;maxIterations?:number;replans?:number;sessionId?:string;
 createdAt?:string;gaps?:string[];evidence?:unknown[];
 planHistory?:{version:number;plan:string;reason?:unknown}[];
 reviewHistory?:{iteration:number;complete:boolean;gaps?:string[];evidenceCount?:number}[];
}
type Job={id:string;kind:string;status:string;input?:{goalId?:string};stepIndex?:number;stepTotal?:number}
const running=(status:string)=>status==='queued'||status==='running'
const errors=(items?:string[])=>Array.isArray(items)?items:[]
/** A real goal API consumer, not another one-shot chat input. */
export function GoalConsole({openSession}:{openSession:(id:string)=>void}){
 const goals=useRemote<Goal[]>('/api/goals',[])
 const jobs=useRemote<Job[]>('/api/jobs',[])
 const agents=useRemote<Agent[]>('/api/agents',[])
 const [goalText,setGoalText]=useState('')
 const [agentId,setAgentId]=useState('')
 const [criteria,setCriteria]=useState('[]')
 const [iterations,setIterations]=useState(12)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [message,setMessage]=useState('')
 const [selected,setSelected]=useState('')
 const available=agents.value.filter(agent=>agent.providerId&&agent.model)
 const executor=available.some(agent=>agent.id===agentId)?agentId:(available[0]?.id||'')
 const hasActive=goals.value.some(goal=>running(goal.status))
 useEffect(()=>{
  if(!hasActive)return
  const timer=window.setInterval(()=>{void goals.reload();void jobs.reload()},3000)
  return()=>window.clearInterval(timer)
 },[hasActive,goals.reload,jobs.reload])
 const submit=async()=>{
  if(busy||!goalText.trim()||!executor)return
  let checks:unknown
  try{
   checks=JSON.parse(criteria)
   if(!Array.isArray(checks))throw Error('완료 조건은 JSON 배열이어야 합니다.')
  }catch(e){setError('완료 조건 JSON: '+(e as Error).message);return}
  setBusy(true);setError('');setMessage('')
  try{
   const created=await post<Goal>('/api/goals',{goal:goalText.trim(),
    executor:{agentId:executor},successCriteria:checks,maxIterations:iterations})
   setGoalText('');setSelected(created.id);setMessage('목표가 작업 큐에 등록됐습니다. 계획과 검증 결과를 아래에서 추적할 수 있습니다.')
   await Promise.all([goals.reload(),jobs.reload()])
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const cancel=async(goal:Goal,jobId?:string)=>{
  if(!jobId||busy)return
  setBusy(true);setError('')
  try{await post('/api/jobs/'+jobId+'/cancel',{});await Promise.all([goals.reload(),jobs.reload()])}
  catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const ordered=[...goals.value].sort((a,b)=>(b.createdAt||'').localeCompare(a.createdAt||''))
 return <div className="mb-8 grid gap-4">
  <Panel className="min-w-0 p-5">
   <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
    <div className="flex items-center gap-2"><Target size={20}/><strong className="text-[15px]">자율 목표 에이전트</strong></div>
    <Button variant="ghost" onClick={()=>{void goals.reload();void jobs.reload()}}><RefreshCw size={14}/> 새로고침</Button>
   </div>
   <p className="mb-4 text-[12px] leading-6 text-[#7C7469]">
    목표 하나를 등록하면 계획 → 도구 작업 → 독립 검토 → 부족한 부분 수정 과정을 실행합니다.
    작업 중 중단과 검토 재개는 영속 체크포인트로 관리하며, 위험한 도구는 별도 승인을 따릅니다.
   </p>
   <div className="grid gap-3">
    <FormField label="달성할 목표"><TextArea rows={3} value={goalText} onChange={e=>setGoalText(e.target.value)}
     placeholder="예: src/engine 아래 변경 요청을 분석하고, 필요한 파일을 수정한 뒤 테스트 결과까지 확인해 주세요."/></FormField>
    <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_150px]">
     <FormField label="실행 에이전트"><SelectInput value={executor} onChange={e=>setAgentId(e.target.value)}>
      {!available.length&&<option value="">설정된 모델이 있는 에이전트가 없습니다</option>}
      {available.map(a=><option key={a.id} value={a.id}>{a.name} · {a.model}</option>)}
     </SelectInput></FormField>
     <FormField label="최대 실행 단계"><TextInput type="number" min={1} max={64} value={iterations}
      onChange={e=>setIterations(Math.max(1,Math.min(64,Number(e.target.value)||1)))}/></FormField>
    </div>
    <details className="rounded-xl border border-[#E7E0D5] p-3 text-[12px] text-[#5C544A]">
     <summary className="cursor-pointer font-semibold">선택: 기계적으로 검사할 완료 조건</summary>
     <p className="mt-2 leading-5">조건을 생략하면 파일 경로가 명시된 목표에서 가능한 검사 조건을 자동 추론합니다. 예: 파일 존재 검사</p>
     <pre className="my-2 overflow-x-auto rounded bg-[#FAF7F1] p-2 text-[11px]">{'[{"type":"file_exists","path":"report.txt"}]'}</pre>
     <TextArea rows={3} value={criteria} onChange={e=>setCriteria(e.target.value)} aria-label="완료 조건 JSON"/>
    </details>
    <div className="flex justify-end"><Button disabled={busy||!goalText.trim()||!executor} onClick={()=>void submit()}>
     <Play size={14}/> {busy?'처리 중':'자율 목표 시작'}
    </Button></div>
   </div>
   {error&&<Notice message={error}/>}
   {message&&<p className="mt-3 flex items-center gap-2 text-xs text-emerald-700"><Check size={14}/>{message}</p>}
  </Panel>
  <Subheading title="자율 작업 진행 현황" extra={<span className="text-xs text-[#7C7469]">{ordered.length}개 목표 · {ordered.filter(g=>running(g.status)).length}개 진행 중</span>}/>
  <div className="grid gap-3">
   {ordered.slice(0,15).map(goal=>{
    const job=jobs.value.find(row=>row.id===goal.jobId)||
      jobs.value.find(row=>row.kind==='goal'&&row.input?.goalId===goal.id)
    const expanded=goal.id===selected
    return <Panel key={goal.id} className="min-w-0 p-4">
     <div className="flex flex-wrap items-start gap-3">
      <button type="button" onClick={()=>setSelected(expanded?'':goal.id)}
       aria-expanded={expanded} className="min-w-0 flex-1 text-left">
       <strong className="block break-words text-[13px]">{goal.goal}</strong>
       <small className="mt-1 block text-[11px] text-[#7C7469]">
        {goal.phase||'대기'} · {goal.iterations||0}/{goal.maxIterations||12}회 실행 · 계획 수정 {goal.replans||0}회
       </small>
      </button>
      <StatusPill status={goal.status}>{goal.status}</StatusPill>
      {running(goal.status)&&<Button size="sm" variant="danger" disabled={busy} onClick={()=>void cancel(goal,job?.id||goal.jobId)}><Square size={12}/> 중단</Button>}
      {goal.sessionId&&<Button size="sm" variant="ghost" onClick={()=>openSession(goal.sessionId!)}>대화 기록 <ArrowRight size={13}/></Button>}
     </div>
     {expanded&&<div className="mt-3 grid gap-3 border-t border-[#EFE9DF] pt-3">
      {goal.plan&&<section><strong className="text-xs">현재 실행 계획</strong><pre className="mt-2 max-h-52 overflow-y-auto whitespace-pre-wrap rounded-lg bg-[#FAF7F1] p-3 text-[11px] leading-5">{goal.plan}</pre></section>}
      {errors(goal.gaps).length>0&&<section><strong className="text-xs">미충족 검증 조건과 수정 과제</strong><ul className="mt-2 list-disc space-y-1 pl-5 text-[11px] text-amber-700">{errors(goal.gaps).map((gap,i)=><li key={i}>{gap}</li>)}</ul></section>}
      {(goal.planHistory||[]).length>1&&<details><summary className="cursor-pointer text-xs font-semibold">재계획 기록 {goal.planHistory!.length}개</summary>
       {goal.planHistory!.map((step,i)=><pre key={i} className="mt-2 whitespace-pre-wrap rounded-lg bg-[#FAF7F1] p-3 text-[11px]">{step.version}차: {step.plan}</pre>)}</details>}
      {(goal.reviewHistory||[]).length>0&&<details><summary className="cursor-pointer text-xs font-semibold">독립 검토 기록 {goal.reviewHistory!.length}개 · 성공 도구 증거 {(goal.evidence||[]).length}개</summary>
       {goal.reviewHistory!.map((review,i)=><div key={i} className="mt-2 rounded-lg bg-[#FAF7F1] p-3 text-[11px]">
        {review.iteration}회차 · {review.complete?'검토 통과':'수정 필요'} · 증거 {review.evidenceCount||0}개
        {errors(review.gaps).length>0&&<p className="mt-1 break-words">{errors(review.gaps).join(' / ')}</p>}
       </div>)}</details>}
      {job&&<small className="text-[10px] text-[#8A8176]">작업 큐: {job.status} · 체크포인트 {job.stepIndex||0}/{job.stepTotal||goal.maxIterations||12}</small>}
     </div>}
    </Panel>
   })}
   {!ordered.length&&<Panel className="p-5 text-center text-[12px] text-[#7C7469]">아직 등록된 자율 목표가 없습니다.</Panel>}
  </div>
 </div>
}
