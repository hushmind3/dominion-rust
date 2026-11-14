import {useEffect,useState} from 'react'
import {ArrowRight,Check,Play,RefreshCw,RotateCw,Square,Target} from 'lucide-react'
import {post} from '../../api'
import {GoalChanges} from './GoalChanges'
import {Button,SelectInput,TextArea,TextInput} from '../../components/Primitives'
import {StatusPill} from '../../components/StatusPill'
import {FormField,Notice,Panel,Subheading,useRemote} from '../../components/UI'

type Agent={id:string;name:string;model?:string;providerId?:string}
type Goal={
 id:string;jobId?:string;goal:string;status:string;phase?:string;plan?:string;
 isolateWorktree?:boolean;gitWorktree?:{path?:string;branch?:string;rolledBack?:boolean};
 iterations?:number;maxIterations?:number;replans?:number;resumeCount?:number;sessionId?:string;
 createdAt?:string;gaps?:string[];evidence?:unknown[];
 planHistory?:{version:number;plan:string;reason?:unknown}[];
 reviewHistory?:{iteration:number;complete:boolean;gaps?:string[];evidenceCount?:number}[];
 metrics?:{verifiedCompleted?:boolean;durationMs?:number;reviewCount?:number;
  successfulToolEvidenceCount?:number;acceptanceCheckCount?:number;unmetGapCount?:number;
  usage?:{calls?:number;totalTokens?:number;estimatedCalls?:number;cachedPromptTokens?:number}};
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
 const [plannerId,setPlannerId]=useState('')
 const [reviewerId,setReviewerId]=useState('')
 const [isolateWorktree,setIsolateWorktree]=useState(false)
 const [criteria,setCriteria]=useState('[]')
 const [iterations,setIterations]=useState(12)
 const [tokenBudget,setTokenBudget]=useState(150000)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [message,setMessage]=useState('')
 const [selected,setSelected]=useState('')
 const [planDrafts,setPlanDrafts]=useState<Record<string,string>>({})
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
    executor:{agentId:executor},
    ...(plannerId?{planner:{agentId:plannerId}}:{}),
    ...(reviewerId?{reviewer:{agentId:reviewerId}}:{}),
    successCriteria:checks,maxIterations:iterations,tokenBudget,
    isolateWorktree})
   setGoalText('');setSelected(created.id);setMessage('목표가 작업 큐에 등록됐습니다. 계획과 검증 결과를 아래에서 추적할 수 있습니다.')
   await Promise.all([goals.reload(),jobs.reload()])
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const cancel=async(goal:Goal)=>{
  if(busy)return
  setBusy(true);setError('');setMessage('')
  try{
   await post('/api/goals/'+encodeURIComponent(goal.id)+'/cancel',{})
   setMessage('목표와 영속 작업에 중단을 요청했습니다.')
   await Promise.all([goals.reload(),jobs.reload()])
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const resume=async(goal:Goal)=>{
  if(busy||!window.confirm('이전 실행의 파일·외부 작업 결과를 확인하셨습니까? 알 수 없는 외부 변경은 자동 반복하지 않습니다. 확인 후 새 실행으로 이어가시겠습니까?'))return
  setBusy(true);setError('');setMessage('')
  try{
   await post('/api/goals/'+encodeURIComponent(goal.id)+'/resume',{acknowledgeUnknownEffects:true})
   setMessage('이전 실행의 검증 기록을 보존하고 새 작업을 등록했습니다. 시작 전 외부 결과를 다시 확인합니다.')
   setSelected(goal.id)
   await Promise.all([goals.reload(),jobs.reload()])
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const submitPlan=async(goal:Goal)=>{
  const next=planDrafts[goal.id]?.trim()
  if(!next||busy)return
  setBusy(true);setError('');setMessage('')
  try{
   await post('/api/goals/'+goal.id+'/plan',{plan:next})
   setMessage('새 계획을 저장했습니다. 현재 실행 중인 도구가 끝나면 다음 실행 단계부터 적용됩니다.')
   setPlanDrafts(previous=>({...previous,[goal.id]:''}))
   await goals.reload()
  }catch(e){setError((e as Error).message)}
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
     <FormField label="목표 토큰 상한 (0 = 제한 없음)" hint="실행·계획·검토 호출의 관측 사용량을 합산합니다. 진행 중 호출은 잔여 한도를 넘을 수 있습니다.">
      <TextInput type="number" min={0} max={2000000} step={1000} value={tokenBudget}
       onChange={e=>setTokenBudget(Math.max(0,Math.min(2000000,Number(e.target.value)||0)))}/>
     </FormField>
    <label className="flex cursor-pointer items-start gap-3 rounded-xl border border-[#E7E0D5] p-3 text-[12px]">
     <TextInput type="checkbox" checked={isolateWorktree}
      onChange={e=>setIsolateWorktree(e.target.checked)} className="mt-0.5"/>
     <span><strong className="block">코딩 목표: Git 작업공간 격리</strong>
      <span className="mt-1 block leading-5 text-[#7C7469]">원본 체크아웃이 아닌 독립 worktree와 브랜치에서 수정하고, 변경 내역을 검토하거나 폐기합니다. Git 저장소 최상위·깨끗한 시작 상태가 필요합니다. 셸 명령에는 별도 승인이 필요합니다.</span>
     </span>
    </label>
    <details className="rounded-xl border border-[#E7E0D5] p-3 text-[12px] text-[#5C544A]">
     <summary className="cursor-pointer font-semibold">선택: 저비용 계획·검토 모델 분리</summary>
     <div className="mt-3 grid gap-3 sm:grid-cols-2">
      <FormField label="계획 담당" hint="지정하지 않으면 실행 에이전트 모델을 재사용합니다."><SelectInput value={plannerId} onChange={e=>setPlannerId(e.target.value)}><option value="">실행 모델과 동일</option>{available.map(a=><option key={a.id} value={a.id}>{a.name} · {a.model}</option>)}</SelectInput></FormField>
      <FormField label="독립 검토 담당" hint="도구 실행 없이 실제 증거와 완료 조건을 검토하는 모델입니다."><SelectInput value={reviewerId} onChange={e=>setReviewerId(e.target.value)}><option value="">실행 모델과 동일</option>{available.map(a=><option key={a.id} value={a.id}>{a.name} · {a.model}</option>)}</SelectInput></FormField>
     </div>
    </details>
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
      {running(goal.status)&&<Button size="sm" variant="danger" disabled={busy} onClick={()=>void cancel(goal)}><Square size={12}/> 중단</Button>}
      {['failed','needs_review'].includes(goal.status)&&<Button size="sm" variant="secondary" disabled={busy||((goal.resumeCount||0)>=12)} onClick={()=>void resume(goal)}><RotateCw size={12}/> 검토 후 이어가기</Button>}
      {goal.sessionId&&<Button size="sm" variant="ghost" onClick={()=>openSession(goal.sessionId!)}>대화 기록 <ArrowRight size={13}/></Button>}
     </div>
     {expanded&&<div className="mt-3 grid gap-3 border-t border-[#EFE9DF] pt-3">
      {goal.plan&&<section><strong className="text-xs">현재 실행 계획</strong><pre className="mt-2 max-h-52 overflow-y-auto whitespace-pre-wrap rounded-lg bg-[#FAF7F1] p-3 text-[11px] leading-5">{goal.plan}</pre></section>}
      {running(goal.status)&&<details className="rounded-lg border border-[#E7E0D5] p-3">
       <summary className="cursor-pointer text-xs font-semibold">실행 계획 직접 수정</summary>
       <p className="mt-2 text-[11px] text-[#7C7469]">현재 도구 실행은 중단하지 않고, 다음 안전한 실행 단계에 새 계획을 반영합니다. 목표 완료 조건과 실행 권한은 변경되지 않습니다.</p>
       <TextArea rows={4} value={planDrafts[goal.id]??''} onChange={e=>setPlanDrafts(previous=>({...previous,[goal.id]:e.target.value}))}
        placeholder={goal.plan||'실행할 새 계획을 구체적으로 입력하세요.'}/>
       <div className="mt-2 flex justify-end"><Button size="sm" variant="secondary" disabled={busy||!(planDrafts[goal.id]||'').trim()} onClick={()=>void submitPlan(goal)}>다음 단계 계획으로 적용</Button></div>
      </details>}
      {errors(goal.gaps).length>0&&<section><strong className="text-xs">미충족 검증 조건과 수정 과제</strong><ul className="mt-2 list-disc space-y-1 pl-5 text-[11px] text-amber-700">{errors(goal.gaps).map((gap,i)=><li key={i}>{gap}</li>)}</ul></section>}
      {(goal.planHistory||[]).length>1&&<details><summary className="cursor-pointer text-xs font-semibold">재계획 기록 {goal.planHistory!.length}개</summary>
       {goal.planHistory!.map((step,i)=><pre key={i} className="mt-2 whitespace-pre-wrap rounded-lg bg-[#FAF7F1] p-3 text-[11px]">{step.version}차: {step.plan}</pre>)}</details>}
      {(goal.reviewHistory||[]).length>0&&<details><summary className="cursor-pointer text-xs font-semibold">독립 검토 기록 {goal.reviewHistory!.length}개 · 성공 도구 증거 {(goal.evidence||[]).length}개</summary>
       {goal.reviewHistory!.map((review,i)=><div key={i} className="mt-2 rounded-lg bg-[#FAF7F1] p-3 text-[11px]">
        {review.iteration}회차 · {review.complete?'검토 통과':'수정 필요'} · 증거 {review.evidenceCount||0}개
        {errors(review.gaps).length>0&&<p className="mt-1 break-words">{errors(review.gaps).join(' / ')}</p>}
       </div>)}</details>}
      {goal.metrics&&<section className="rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3">
       <strong className="text-xs">실행 품질·비용 계측</strong>
       <div className="mt-2 grid grid-cols-2 gap-2 text-[11px] sm:grid-cols-4">
        <div><span className="block text-[#8A8176]">완료 검증</span><strong>{goal.metrics.verifiedCompleted?'통과':'미완료'}</strong></div>
        <div><span className="block text-[#8A8176]">도구 증거</span><strong>{goal.metrics.successfulToolEvidenceCount??0}건</strong></div>
        <div><span className="block text-[#8A8176]">검토 라운드</span><strong>{goal.metrics.reviewCount??0}회</strong></div>
        <div><span className="block text-[#8A8176]">소요 시간</span><strong>{typeof goal.metrics.durationMs==='number'?Math.round(goal.metrics.durationMs/1000)+'초':'기록 없음'}</strong></div>
       </div>
       {goal.metrics.usage&&<p className="mt-3 text-[11px] text-[#5C544A]">모델 호출 {goal.metrics.usage.calls??0}회 · 기록된 토큰 {(goal.metrics.usage.totalTokens??0).toLocaleString()}개 · 추정 사용량 호출 {goal.metrics.usage.estimatedCalls??0}회</p>}
       <p className="mt-1 text-[10px] text-[#8A8176]">기록된 사용량 기준이며 미보고 캐시·실제 결제액은 추정하지 않습니다.</p>
      </section>}
      <GoalChanges goalId={goal.id} finished={!running(goal.status)}
       isolated={goal.isolateWorktree===true}
       discarded={goal.gitWorktree?.rolledBack===true}/>
      {job&&<small className="text-[10px] text-[#8A8176]">작업 큐: {job.status} · 체크포인트 {job.stepIndex||0}/{job.stepTotal||goal.maxIterations||12}</small>}
     </div>}
    </Panel>
   })}
   {!ordered.length&&<Panel className="p-5 text-center text-[12px] text-[#7C7469]">아직 등록된 자율 목표가 없습니다.</Panel>}
  </div>
 </div>
}
