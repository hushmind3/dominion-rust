import {useEffect,useRef,useState} from 'react'
import {AlertTriangle,Check,ChevronDown,ChevronUp,Clock3,FileDiff,Play,RefreshCw,ShieldCheck,Square,Workflow} from 'lucide-react'
import {get,post} from '../api'
import {Button,TextArea,TextInput} from './Primitives'
import {Notice,Panel,refresh,useRemote} from './UI'
import {useLocale} from '../i18n'
import {RunEvidenceAssets} from './RunEvidenceAssets'
import {RunDelegationPanel} from './RunDelegationPanel'

type RunStatus='queued'|'running'|'awaiting_approval'|'paused'|'completed'|'failed'|'cancelled'
type NativeEvent={seq:number;kind:string;message:string;at_ms:number;detail?:unknown}
type ToolCall={id:string;name:string;arguments:unknown}
type NativeProgram={
 id:string;cursor:number;pending_step?:string|null;halt_requested?:boolean;
 steps:{id:string;tool:string}[];
 observations?:Record<string,{status?:string}>;
}
type NativeRun={
 id:string;instruction:string;status:RunStatus;plan?:string|null;answer?:string|null;
 steps:number;max_steps:number;budget_krw:number;cost_krw:number;remote_cost_krw?:number;
 delegated_spent_krw?:number;input_tokens:number;cached_input_tokens?:number;
 output_tokens:number;events:NativeEvent[];team?:unknown;parent_run_id?:string|null;
 pending_approval?:ToolCall|null;approval_decision?:boolean|null;in_flight?:ToolCall|null;
 repository?:string|null;read_only?:boolean;reserved_budget_krw?:number;
 model_only?:boolean;program_session?:NativeProgram|null;
}
type TeamTask={id:string;run_id:string;role:'investigator'|'writer'|'reviewer';
 workspace:string;depends_on:string[];status:string;patch_review_ready?:boolean;unmet_acceptance?:string[];
 provider_id?:string;model_name?:string}
type TeamView={root_run_id:string;tasks:TeamTask[];completed:number;all_complete:boolean;failed:boolean}
type Patch={source_run_id:string;sha256:string;changed_paths:string[];patch_bytes:number;diff:string;can_apply:boolean;conflict?:string|null}
const money=(n:number|undefined)=>new Intl.NumberFormat('ko-KR',{maximumFractionDigits:2}).format(n||0)+'원'
const statusName=(s:string,ko:boolean)=>({
 queued:ko?'대기':'Queued',running:ko?'실행 중':'Running',
 awaiting_approval:ko?'승인 필요':'Needs approval',paused:ko?'일시 정지':'Paused',
 completed:ko?'완료':'Completed',failed:ko?'실패':'Failed',cancelled:ko?'취소':'Cancelled',
 ready:ko?'실행 준비':'Ready',pending:ko?'순서 대기':'Pending',
 active:ko?'진행 중':'Active',approval_required:ko?'패치 승인 필요':'Patch review',
 blocked:ko?'선행 작업 대기':'Blocked'
} as Record<string,string>)[s]||s
function eventTime(ms:number){return Number.isFinite(ms)&&ms>0?new Date(ms).toLocaleString():'—'}
function unresolvedAgentModelRequest(events:NativeEvent[]):number|undefined{
 let pending:number|undefined
 for(const event of events){
  if(event.kind==='model_request_intent')pending=event.seq
  if(event.kind==='model_request_settled'){
   const details=event.detail&&typeof event.detail==='object'
    ?event.detail as Record<string,unknown>:null
   if(details?.intent_seq===pending)pending=undefined
  }
 }
 return pending
}

export function NativeRunWorkspace({id,onSelect}:{id:string;onSelect:(id:string)=>void}){
 const loc=useLocale(),ko=loc==='ko'
 const current=useRemote<NativeRun|null>('/api/v1/runs/'+encodeURIComponent(id),null)
 const run=current.value
 const team=useRemote<TeamView|null>(run?.team?'/api/v1/task-teams/'+encodeURIComponent(id):null,null)
 const [busy,setBusy]=useState(false),[error,setError]=useState(''),[notice,setNotice]=useState('')
 const [effectEvidence,setEffectEvidence]=useState('')
 const [ownerNote,setOwnerNote]=useState('')
 const [patch,setPatch]=useState<{task:string;value:Patch}|null>(null)
 const [eventsOpen,setEventsOpen]=useState(false)
 const [eventQuery,setEventQuery]=useState('')
 const [eventPage,setEventPage]=useState(0)
 const [focusedEventSeq,setFocusedEventSeq]=useState<number|null>(null)
 const journalRef=useRef<HTMLOListElement>(null)
 useEffect(()=>{
  setEventQuery('');setEventPage(0);setFocusedEventSeq(null);setEventsOpen(false)
 },[id])
 useEffect(()=>{
  if(eventsOpen&&focusedEventSeq!==null){
   journalRef.current?.querySelector<HTMLElement>('[data-seq="'+focusedEventSeq+'"]')
    ?.scrollIntoView({block:'nearest'})
  }
 },[eventsOpen,focusedEventSeq,eventPage])
 const reload=async()=>{
  await Promise.all([current.reload(),team.reload()])
  refresh('/api/executions','/api/jobs')
 }
 async function action(url:string,body:unknown,success:string){
  if(busy)return
  setBusy(true);setError('');setNotice('')
  try{await post(url,body);setNotice(success);setPatch(null);await reload()}
  catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 async function directive(){
  if(busy||!ownerNote.trim()||ownerNote.length>10000)return
  setBusy(true);setError('');setNotice('')
  try{
   await post('/api/v1/runs/'+encodeURIComponent(id)+'/directive',{instruction:ownerNote.trim()})
   setOwnerNote('')
   setNotice(ko?'지시가 실행 저널에 기록됐습니다. 다음 안전한 판단 시점에 적용됩니다.':'Directive recorded in the durable Run journal.')
   await reload()
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const base='/api/v1/runs/'+encodeURIComponent(id)
 const teamBase='/api/v1/task-teams/'+encodeURIComponent(id)
 async function preview(task:string){
  if(busy)return
  setBusy(true);setError('');setPatch(null)
  try{const value=await get<Patch>(teamBase+'/patches/'+encodeURIComponent(task));setPatch({task,value})}
  catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 if(current.loading&&!run)return <div className="mt-4 p-4 text-xs text-[#7C7469]" role="status">{ko?'실행 기록을 불러오는 중…':'Loading execution…'}</div>
 if(current.error&&!run)return <Notice message={current.error}/>
 if(!run)return null
 const approval=run.pending_approval&&run.approval_decision==null
 const isTeam=!!run.team
 const uncertain=!isTeam&&run.status==='paused'&&!!run.in_flight
 const pendingModelIntent=unresolvedAgentModelRequest(run.events)

 // Each program step is the actual Run journal cursor, never a UI-only plan.
 const program=run.program_session
 const programDone=program?.steps.filter(step=>
  program.observations?.[step.id]?.status!=null).length||0
 const lastProgram=program?undefined:run.events.slice().reverse().find(event=>
  ['program_completed','program_completed_with_errors','program_stopped',
   'program_superseded','program_evidence_objective'].includes(event.kind))
 const lastProgramDetail=lastProgram?.detail&&typeof lastProgram.detail==='object'
  ?lastProgram.detail as Record<string,unknown>:null
 const lastProgramTotal=Number(lastProgramDetail?.successful||0)
  +Number(lastProgramDetail?.failed||0)+Number(lastProgramDetail?.skipped||0)
 const lastTestReceipt=run.events.slice().reverse().find(event=>
  event.kind==='test_evidence_checked'||event.kind==='test_evidence_missing')
 const newestEvents=run.events.slice().reverse()
 const matchingEvents=eventQuery.trim()
  ?newestEvents.filter(ev=>{
      const search=eventQuery.trim().toLowerCase()
      return ev.kind.toLowerCase().includes(search)
       ||ev.message.toLowerCase().includes(search)
       ||String(ev.seq).includes(search)
       ||(ev.detail!=null&&JSON.stringify(ev.detail).toLowerCase().includes(search))
    })
  :newestEvents
 const eventPageCount=Math.max(1,Math.ceil(matchingEvents.length/35))
 const visibleEventPage=Math.min(eventPage,eventPageCount-1)
 const visibleEvents=matchingEvents.slice(visibleEventPage*35,(visibleEventPage+1)*35)
 const showEvent=(seq:number)=>{
   setEventQuery('')
   setFocusedEventSeq(seq)
   const index=newestEvents.findIndex(event=>event.seq===seq)
   setEventPage(index>=0?Math.floor(index/35):0)
   setEventsOpen(true)
 }
 const testExecutionConfirmed=lastTestReceipt?.kind==='test_evidence_checked'
 const uncertainModelCall=!isTeam&&run.status==='paused'
  &&(pendingModelIntent!==undefined||(run.model_only===true
    &&run.events.some(event=>event.kind==='model_only_request_intent')))
 const teamPatchUncertain=isTeam&&run.status==='paused'&&run.in_flight?.name==='team_patch_integrate'
 const patchArgs=run.in_flight?.arguments&&typeof run.in_flight.arguments==='object'
  ?run.in_flight.arguments as Record<string,unknown>:{}
 const pendingTask=typeof patchArgs.task_id==='string'?patchArgs.task_id:''
 const pendingSha=typeof patchArgs.patch_sha256==='string'?patchArgs.patch_sha256:''
 const canReconcileTeamPatch=teamPatchUncertain&&/^[0-9a-fA-F]{64}$/.test(pendingSha)&&!!pendingTask
 const canStop=['queued','running','awaiting_approval','paused'].includes(run.status)
 const totalCost=(run.cost_krw||0)+(run.remote_cost_krw||0)+(run.delegated_spent_krw||0)
 const usedSteps=Math.min(100,run.max_steps>0?Math.round(run.steps/run.max_steps*100):0)
 // The coordinator is intentionally stored as Paused while its children run.
 // Do not display "paused" as if active work had stopped.
 const teamTasks=team.value?.tasks||[]
 const teamStarted=run.events.some(event=>event.kind==='team_started')
 const visualStatus=isTeam&&run.status==='paused'
  ?teamTasks.some(task=>task.status==='active')
   ?(ko?'팀 실행 중':'Team running')
   :teamTasks.some(task=>task.patch_review_ready)
    ?(ko?'패치 승인 대기':'Patch approval needed')
    :teamTasks.some(task=>task.status==='approval_required')
     ?(ko?'Worker 도구 승인 대기':'Worker tool approval needed')
    :!teamStarted?(ko?'팀 초안':'Team draft')
     :teamTasks.some(task=>task.status==='ready')
      ?(ko?'다음 작업 준비':'Next task ready')
      :(ko?'팀 작업 대기':'Team waiting')
  :statusName(run.status,ko)
 return <Panel className="mt-4 p-4 sm:p-5" aria-label={ko?'실행 상세 및 제어':'Execution details and controls'}>
  <div className="flex flex-wrap items-start justify-between gap-3">
   <div className="min-w-0 flex-1">
    <p className="text-[10px] font-semibold uppercase tracking-widest text-[#9C9388]">{isTeam?'TASK TEAM':'NATIVE RUN'}</p>
    <h3 className="mt-1 text-[15px] font-semibold">{ko?'실제 실행 상태':'Live execution state'}</h3>
    <p className="mt-1 whitespace-pre-wrap break-words text-xs text-[#5C544A]">{run.instruction}</p>
    <p className="mt-1 break-all font-mono text-[10px] text-[#9C9388]">{run.id}</p>
   </div>
   <div className="flex flex-wrap items-center gap-2">
    <span className="rounded-lg bg-[#F3EEE5] px-2 py-1 text-xs font-semibold">{visualStatus}</span>
    <Button size="sm" variant="ghost" disabled={busy} onClick={()=>void reload()}><RefreshCw size={13}/>{ko?'새로고침':'Refresh'}</Button>
   </div>
  </div>
  <div className="mt-4 grid gap-2 sm:grid-cols-3">
   <div className="rounded-lg bg-[#FAF7F1] p-3"><p className="text-[11px] text-[#7C7469]">{ko?'사용한 실행 단계':'Steps used'}</p><strong>{run.steps} / {run.max_steps}</strong><div role="progressbar" aria-label={ko?'단계 사용량':'Steps used'} aria-valuenow={run.steps} aria-valuemin={0} aria-valuemax={run.max_steps||1} className="mt-2 h-1.5 overflow-hidden rounded-full bg-[#E7E0D5]"><div className="h-full bg-[#C96442]" style={{width:usedSteps+'%'}}/></div></div>
   <div className="rounded-lg bg-[#FAF7F1] p-3"><p className="text-[11px] text-[#7C7469]">{ko?'지출 기록':'Observed spend'}</p><strong>{money(totalCost)}</strong><p className="text-[10px] text-[#8A8176]">{ko?'최대 예산':'Budget'} {money(run.budget_krw)}</p></div>
   <div className="rounded-lg bg-[#FAF7F1] p-3"><p className="text-[11px] text-[#7C7469]">{ko?'입력 / 출력 토큰':'Input / output tokens'}</p><strong>{run.input_tokens.toLocaleString()} / {run.output_tokens.toLocaleString()}</strong><p className="text-[10px] text-[#8A8176]">{ko?'입력 중 캐시 기록':'Cached input'} {(run.cached_input_tokens||0).toLocaleString()}</p></div>
  </div>
  {run.parent_run_id&&<Button size="sm" variant="ghost" onClick={()=>onSelect(run.parent_run_id!)} className="mt-2">{ko?'상위 팀 / 실행으로 돌아가기':'Back to parent run'}</Button>}
  {run.repository&&<p className="mt-3 break-all text-xs text-[#7C7469]">{ko?'작업 저장소':'Repository'}: {run.repository}</p>}

  {program&&<section aria-label={ko?'실행 프로그램 단계':'Native program progress'} className="mt-4 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3">
   <div className="flex flex-wrap items-center justify-between gap-2">
    <strong className="flex items-center gap-1 text-xs"><Workflow size={15}/>{ko?'작업 프로그램 진행':'Native program progress'}</strong>
    <span className="text-xs font-semibold">{programDone} / {program.steps.length}</span>
   </div>
   <ol className="mt-2 grid gap-1 sm:grid-cols-2">
    {program.steps.map((step,index)=>{
     const observed=program.observations?.[step.id]?.status
     const active=program.cursor===index&&!!program.pending_step
     const approvalNeeded=active&&run.status==='awaiting_approval'
      &&run.pending_approval?.id===program.pending_step
     const status=observed==='ok'?(ko?'성공':'Done')
      :observed==='error'?(ko?'실패':'Failed')
      :observed==='skipped'?(ko?'건너뜀':'Skipped')
      :approvalNeeded?(ko?'승인 대기':'Approval needed')
      :active?(ko?'실행 중':'Running'):(ko?'대기':'Pending')
     return <li key={step.id} className="flex min-w-0 items-center gap-2 rounded-lg bg-white px-2 py-1.5 text-xs">
      <span className="shrink-0 text-[#8A8176]">{index+1}.</span>
      <span className="min-w-0 flex-1 break-words">{step.id} · {step.tool}</span>
      <span className="shrink-0 font-semibold">{status}</span>
     </li>
    })}
   </ol>
   {program.halt_requested&&<p className="mt-2 text-xs text-amber-800">{ko?'새 지시가 들어와 이미 시작한 효과의 기록을 확정한 뒤 나머지 단계를 멈춥니다.':'An owner directive superseded future steps; started effects still require original receipts.'}</p>}
  </section>}
  {!program&&lastProgram&&<div role="status" className="mt-4 rounded-lg border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-xs">
   <strong className="flex items-center gap-1"><Workflow size={15}/>{ko?'마지막 작업 프로그램 기록':'Last native program receipt'}</strong>
   <p className="mt-1 text-[#5C544A]">{lastProgram.kind==='program_completed'
    ?(ko?'프로그램 도구 단계가 정상 종료됐습니다. 전체 사용자 작업의 완료 판정과는 별개입니다.':'Program tool steps settled successfully. This is not overall task acceptance.')
    :(ko?'프로그램이 중단되거나 일부 실패했습니다. 원본 도구 기록을 확인하세요.':'Program stopped or reported partial failure. Inspect durable tool receipts.')}
    {lastProgramTotal>0?' · '+lastProgramTotal+(ko?'단계':' steps'):''}
   </p>
  </div>}
  {approval&&<div className="mt-4 rounded-xl border border-amber-200 bg-amber-50 p-4">
   <div className="flex items-center gap-2 font-semibold text-amber-900"><ShieldCheck size={17}/>{ko?'도구 실행 승인 대기':'Tool approval required'}</div>
   <p className="mt-1 text-xs text-amber-800">{run.pending_approval?.name}</p>
   <pre className="mt-2 max-h-44 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-white p-2 text-[11px]">{JSON.stringify(run.pending_approval?.arguments,null,2)}</pre>
   <div className="mt-3 flex flex-wrap gap-2">
    <Button disabled={busy} variant="primary" onClick={()=>void action(base+'/approval',{allow:true},ko?'요청한 도구를 승인했습니다':'Tool approved')}><Check size={14}/>{ko?'이 작업 승인':'Approve tool'}</Button>
    <Button disabled={busy} variant="secondary" onClick={()=>void action(base+'/approval',{allow:false},ko?'도구 실행을 거절했습니다':'Tool denied')}>{ko?'거절':'Deny'}</Button>
   </div>
  </div>}
  {teamPatchUncertain&&<div className="mt-4 rounded-xl border border-rose-300 bg-rose-50 p-4">
   <strong className="flex items-center gap-2 text-sm text-rose-900"><AlertTriangle size={17}/>{ko?'Git 패치 적용 결과 미확정':'Git patch outcome uncertain'}</strong>
   <p className="mt-2 text-xs text-rose-800">{ko?'패치 적용 도중 중단되었습니다. 다른 도구처럼 성공/실패를 임의로 기록할 수 없으며, 원래 승인한 SHA-256 패치가 Git에 존재하는지를 Rust가 역검증해야 합니다.':'Patch adoption was interrupted. The Rust engine must prove the exact originally approved SHA-256 patch is present in Git before clearing the uncertainty.'}</p>
   <p className="mt-2 break-words text-[11px] text-rose-800">{ko?'작업':'Task'}: {pendingTask||'—'}</p>
   <p className="mt-1 break-all font-mono text-[10px] text-rose-800">SHA-256: {pendingSha||'—'}</p>
   <Button className="mt-3" variant="secondary" disabled={busy||!canReconcileTeamPatch}
    onClick={()=>void action(teamBase+'/patches/'+encodeURIComponent(pendingTask)+'/reconcile',
      {sha256:pendingSha},ko?'원본 Git 패치의 적용 결과를 검증했습니다':'Exact patch outcome verified')}>
    <ShieldCheck size={14}/>{ko?'원본 패치 상태 검증·복구':'Verify & reconcile original patch'}
   </Button>
   {!canReconcileTeamPatch&&<Notice message={ko?'복구에 필요한 원본 패치 정보가 불완전합니다. 실행 이벤트 기록을 확인하세요.':'Original patch intent lacks verifiable task/hash fields.'}/>}
  </div>}
  {uncertainModelCall&&<div className="mt-4 rounded-xl border border-amber-200 bg-amber-50 p-4" role="status">
   <div className="flex items-center gap-2 font-semibold text-amber-900"><AlertTriangle size={16}/>{ko?'모델 응답 확인 필요':'Model response requires reconciliation'}</div>
   <p className="mt-2 text-xs text-amber-800">{ko?
    '모델 요청을 시작한 기록은 있지만 응답 및 사용량 영수증을 확인하지 못했습니다. 계획·에이전트 판단·검증 호출도 같은 실행에서 자동 재호출하지 않습니다. 원본 실행 기록을 확인하고 새 요청을 작성하세요.':
    'A model request has an unresolved billing outcome. Planning, agent turns and verification cannot replay that paid request automatically. Inspect the original Run journal and submit a fresh request.'}</p>
  </div>}
  {uncertain&&<div className="mt-4 rounded-xl border border-rose-200 bg-rose-50 p-4">
   <div className="flex items-center gap-2 font-semibold text-rose-900"><AlertTriangle size={16}/>{ko?'외부 변경 결과 확인 필요':'Uncertain external effect'}</div>
   <p className="mt-2 text-xs text-rose-800">{run.in_flight?.name}: {ko?'이 작업은 자동 재실행되지 않습니다. 외부 시스템에서 실제 결과를 확인한 후 처리하세요.':'This operation will not auto-replay. Verify the real external outcome first.'}</p>
   <TextArea value={effectEvidence} onChange={e=>setEffectEvidence(e.target.value)} rows={2} placeholder={ko?'확인한 결과와 근거를 입력하세요':'Describe independently verified outcome'} />
   <div className="mt-2 flex flex-wrap gap-2">
    <Button disabled={busy||!effectEvidence.trim()} onClick={()=>{if(window.confirm(ko?'외부 작업이 실제 성공했습니까?':'Confirm actual success?'))void action(base+'/reconcile',{succeeded:true,result:effectEvidence.trim()},ko?'성공 결과 기록':'Success recorded')}}>{ko?'성공 확인':'Confirm success'}</Button>
    <Button variant="secondary" disabled={busy||!effectEvidence.trim()} onClick={()=>{if(window.confirm(ko?'외부 작업이 실제 실패했습니까?':'Confirm actual failure?'))void action(base+'/reconcile',{succeeded:false,result:effectEvidence.trim()},ko?'실패 결과 기록':'Failure recorded')}}>{ko?'실패 확인':'Confirm failure'}</Button>
   </div>
  </div>}
  <div className="mt-4 flex flex-wrap gap-2">
   {!isTeam&&run.status==='paused'&&!run.in_flight&&!approval&&!uncertainModelCall&&<Button disabled={busy} onClick={()=>void action(base+'/resume',{},ko?'실행을 다시 시작했습니다':'Run resumed')}><Play size={14}/>{ko?'이어 실행':'Resume'}</Button>}
   {isTeam&&run.status==='paused'&&!run.in_flight&&<Button disabled={busy} onClick={()=>void action(teamBase+'/advance',{},ko?'팀 작업을 진행했습니다':'Team advanced')}><Play size={14}/>{ko?'팀 시작 / 다음 단계':'Start / advance team'}</Button>}
   {canStop&&<Button variant="secondary" disabled={busy||Boolean(teamPatchUncertain)} onClick={()=>{if(window.confirm(ko?'이 실행을 취소하시겠습니까?':'Cancel this run?'))void action(isTeam?teamBase+'/cancel':base+'/cancel',{},ko?'취소 요청을 기록했습니다':'Cancellation requested')}}><Square size={13}/>{ko?'취소':'Cancel'}</Button>}
  </div>
  {!isTeam&&run.status==='running'&&<div className="mt-4 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3">
    <strong className="text-xs">{ko?'실행 중 지시 변경':'Give the running agent new instructions'}</strong>
    <p className="mt-1 text-[11px] text-[#7C7469]">{ko?'새 지시는 기존 실행 ID와 도구 영수증을 유지하면서 저널에 기록됩니다. 진행 중인 외부 변경은 영수증이 확정될 때까지 보존됩니다.':'The same Run ID and effects are preserved; started external writes settle before new instructions take effect.'}</p>
    <TextArea rows={2} maxLength={10000} className="mt-2" value={ownerNote} onChange={e=>setOwnerNote(e.target.value)}
     placeholder={ko?'변경할 목표 또는 추가 확인사항':'Add corrections or updated acceptance requirements'}/>
    <Button size="sm" disabled={busy||!ownerNote.trim()} onClick={()=>void directive()}>{ko?'실행에 지시 전달':'Send directive'}</Button>
   </div>}
   {!isTeam&&<RunDelegationPanel runId={id} parentStatus={run.status} isTeam={isTeam} onSelect={onSelect}/>}
   {team.value&&<div className="mt-5 border-t border-[#E7E0D5] pt-4">
   <div className="flex flex-wrap items-center justify-between gap-2"><h4 className="text-sm font-semibold"><Workflow size={15} className="mr-1 inline"/>{ko?'팀 작업 보드':'Team task board'}</h4><span className="text-xs text-[#7C7469]">{team.value.completed}/{team.value.tasks.length} {ko?'완료':'completed'}</span></div>
   <ol className="mt-3 grid gap-2">
    {team.value.tasks.map((task,index)=><li key={task.id} className="rounded-xl border border-[#E7E0D5] bg-white p-3">
     <div className="flex flex-wrap items-start gap-2"><span className="rounded bg-[#F3EEE5] px-1.5 py-0.5 text-[10px]">{index+1}</span><div className="min-w-0 flex-1"><strong className="text-xs">{task.id}</strong><p className="text-[11px] text-[#7C7469]">{task.role} · {task.workspace}{task.depends_on.length?' · '+(ko?'선행':'After')+': '+task.depends_on.join(', '):''}</p>{task.model_name&&
     <p className="mt-1 break-all text-[11px] text-[#7C7469]">{ko?'실제 모델':'Bound model'}: {task.provider_id||'dominion-native'} · {task.model_name}</p>}</div><span className="rounded-lg bg-[#F3EEE5] px-2 py-1 text-[11px]">{statusName(task.status,ko)}</span></div>
     {!!task.unmet_acceptance?.length&&<p className="mt-2 text-[11px] text-rose-700">{task.unmet_acceptance.join(', ')}</p>}
     <div className="mt-2 flex flex-wrap gap-2">
      <Button size="sm" variant="ghost" onClick={()=>onSelect(task.run_id)}>{ko?'Worker 기록':'Worker run'}</Button>
      {task.role==='writer'&&task.patch_review_ready&&run.repository&&
       <Button size="sm" variant="secondary" disabled={busy} onClick={()=>void preview(task.id)}><FileDiff size={13}/>{ko?'패치 검토':'Review patch'}</Button>}
     </div>
    </li>)}
   </ol>
  </div>}
  {patch&&<div className="mt-4 rounded-xl border border-[#C7B18F] bg-[#FAF7F1] p-4">
   <strong className="flex items-center gap-2 text-sm"><FileDiff size={15}/>{ko?'변경사항 검토':'Patch review'}: {patch.task}</strong>
   <p className="mt-2 break-all font-mono text-[10px] text-[#5C544A]">SHA-256: {patch.value.sha256}</p>
   <p className="mt-1 text-xs">{patch.value.changed_paths.length} {ko?'개 파일':'files'} · {patch.value.patch_bytes.toLocaleString()} bytes</p>
   <p className="mt-1 text-xs text-[#7C7469]">{patch.value.changed_paths.join(', ')}</p>
   <pre className="mt-3 max-h-72 overflow-auto whitespace-pre-wrap break-words rounded-lg border bg-white p-3 text-[11px]">{patch.value.diff}</pre>
   {patch.value.conflict&&<Notice message={patch.value.conflict}/>}
   <div className="mt-3 flex flex-wrap gap-2">
    <Button disabled={busy||!patch.value.can_apply} onClick={()=>{if(window.confirm(ko?'검토한 SHA-256 패치를 팀 저장소에 반영하시겠습니까?':'Apply the exact reviewed patch?'))void action(teamBase+'/patches/'+encodeURIComponent(patch.task)+'/approval',{sha256:patch.value.sha256},ko?'검토한 패치를 승인했습니다':'Reviewed patch approved')}}><Check size={14}/>{ko?'검토한 패치 승인':'Approve reviewed patch'}</Button>
    <Button variant="secondary" onClick={()=>setPatch(null)}>{ko?'닫기':'Close'}</Button>
   </div>
  </div>}
  {run.plan&&<details className="mt-4 border-t border-[#E7E0D5] pt-3"><summary className="cursor-pointer text-xs font-semibold">{ko?'실행 계획':'Execution plan'}</summary><p className="mt-2 whitespace-pre-wrap break-words text-xs text-[#5C544A]">{run.plan}</p></details>}
  {run.answer&&<div className="mt-4 rounded-xl border border-[#D5E6D5] bg-[#F5FAF4] p-3"><strong className="text-xs text-[#28663E]">{ko?'최종 결과':'Final result'}</strong><p className="mt-2 whitespace-pre-wrap break-words text-xs">{run.answer}</p></div>}
  {lastTestReceipt&&<div role="status" className="mt-4 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3">
  <strong className="flex items-center gap-2 text-xs">{testExecutionConfirmed?<ShieldCheck size={15}/>:<AlertTriangle size={15}/>}
   {ko?'요청한 테스트의 실제 실행 근거':'Requested test execution evidence'}</strong>
  <p className="mt-1 text-xs text-[#5C544A]">{testExecutionConfirmed?
   (ko?'마지막 변경 이후 승인된 테스트 명령의 성공 기록이 있습니다. 실제 요구사항 충족 여부는 별도 검증 대상입니다.':'An approved test command succeeded after the latest edit. Semantic acceptance still needs separate checks.'):
   (ko?'요청한 테스트의 최신 성공 기록이 없어 완료 판정을 보류했습니다. 테스트 명령을 승인·실행하고 실패를 수정해야 합니다.':'No fresh successful test receipt. Completion is blocked until approved tests run and failures are addressed.')}</p>
  <button type="button" onClick={()=>showEvent(lastTestReceipt.seq)} className="mt-2 text-xs font-semibold text-[#A94E30] underline underline-offset-2">
   {ko?'원본 실행 영수증 보기':'Show original execution receipts'} · #{lastTestReceipt.seq}</button>
 </div>}
 <RunEvidenceAssets runId={id}/>
  <div className="mt-4 border-t border-[#E7E0D5] pt-3">
   <button type="button" onClick={()=>setEventsOpen(v=>!v)} aria-expanded={eventsOpen} className="flex w-full items-center justify-between text-left text-xs font-semibold"><span><Clock3 size={14} className="mr-1 inline"/>{ko?'실행 증거 · 이벤트 기록':'Durable execution events'} ({run.events.length})</span>{eventsOpen?<ChevronUp size={15}/>:<ChevronDown size={15}/>}</button>
   {eventsOpen&&<section aria-label={ko?'실행 기록 탐색':'Browse execution events'} className="mt-3">
      <TextInput aria-label={ko?'실행 기록 검색':'Search execution events'} value={eventQuery}
       onChange={e=>{setEventQuery(e.target.value);setEventPage(0);setFocusedEventSeq(null)}}
       placeholder={ko?'이벤트 유형·내용·번호 검색':'Search type, message, sequence'} />
      <div className="mt-2 flex flex-wrap items-center justify-between gap-2 text-[11px]">
       <span role="status" className="text-[#8A8176]">{matchingEvents.length===0?'0':visibleEventPage*35+1}–{Math.min((visibleEventPage+1)*35,matchingEvents.length)} / {matchingEvents.length}</span>
       <div className="flex gap-1">
        <Button size="sm" variant="secondary" disabled={visibleEventPage===0}
         onClick={()=>{setEventPage(p=>Math.max(0,p-1));setFocusedEventSeq(null)}}>{ko?'새 기록':'Newer'}</Button>
        <Button size="sm" variant="secondary" disabled={visibleEventPage+1>=eventPageCount}
         onClick={()=>{setEventPage(p=>p+1);setFocusedEventSeq(null)}}>{ko?'이전 기록':'Older'}</Button>
       </div>
      </div>
      <ol ref={journalRef} className="mt-2 max-h-96 space-y-2 overflow-auto">
       {visibleEvents.map(ev=><li key={ev.seq} data-seq={ev.seq} id={'run-event-'+ev.seq}
        className={'rounded-lg border p-2 text-[11px] '+(focusedEventSeq===ev.seq?
          'border-[#A94E30] bg-[#FFF5EB]':'border-transparent bg-[#FAF7F1]')}>
         <div className="flex flex-wrap justify-between gap-1"><strong>{ev.kind}</strong><span className="text-[#9C9388]">#{ev.seq} · {eventTime(ev.at_ms)}</span></div>
         <p className="mt-1 whitespace-pre-wrap break-words">{ev.message}</p>
         {ev.detail!=null&&<details className="mt-1"><summary className="cursor-pointer text-[#7C7469]">{ko?'기록 세부정보':'Receipt detail'}</summary><pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-all">{JSON.stringify(ev.detail,null,2)}</pre></details>}
        </li>)}
      </ol>
      {matchingEvents.length===0&&<p className="mt-2 text-xs text-[#8A8176]">{ko?'일치하는 실행 기록이 없습니다.':'No matching events.'}</p>}
     </section>}
  </div>
  {notice&&<p role="status" className="mt-3 text-xs text-emerald-700">{notice}</p>}
  {error&&<Notice message={error}/>}
  {current.error&&<Notice message={current.error}/>}
  {team.error&&<Notice message={team.error}/>}
 </Panel>
}
