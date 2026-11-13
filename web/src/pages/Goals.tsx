import {useEffect,useMemo,useState} from 'react'
import {ArrowRight,Check,FileCheck2,Play,RefreshCw,RotateCw,ShieldCheck,Square,Trash2,Wrench} from 'lucide-react'
import {get,post,del} from '../api'
import {Button,SelectInput,TextArea,TextInput} from '../components/Primitives'
import {StatusPill} from '../components/StatusPill'
import {FormField,Headline,Notice,Panel,useRemote} from '../components/UI'

type Agent={id:string;name:string;model?:string}
type CheckResult={type?:string;path?:string|null;command?:string|null;passed?:boolean;gaps?:string[]}
type Artifact={path:string;bytes:number;sha256?:string;hashOmitted?:boolean}
type GoalReport={accepted:boolean;checks:CheckResult[];gaps:string[];artifacts:Artifact[];successfulToolEvidence:number;toolEvidenceRecorded:number;operations:{tool:string;path?:string|null;exitCode?:number|null}[]}
type Goal={id:string;goal:string;status:string;phase?:string;createdAt?:string;jobId?:string;sessionId?:string;plan?:string;
 iterations?:number;maxIterations?:number;replans?:number;planHistory?:{version:number;plan:string}[];
 reviewHistory?:{iteration:number;complete:boolean;gaps:string[]}[];
 evidence?:unknown[];gaps?:string[];acceptanceChecks?:unknown[];finalReport?:GoalReport}
type Job={id:string;status:string;stepIndex?:number;stepTotal?:number;error?:string;result?:{status?:string;finalReport?:GoalReport}}
type VerifyKind='auto'|'file_exists'|'file_changed'|'command_succeeded'

export function GoalsPage({openSession}:{openSession:(id:string)=>void}){
 const goals=useRemote<Goal[]>('/api/goals',[])
 const agents=useRemote<Agent[]>('/api/agents',[])
 const [goalText,setGoalText]=useState('')
 const [agentId,setAgentId]=useState('')
 const [kind,setKind]=useState<VerifyKind>('auto')
 const [criterion,setCriterion]=useState('')
 const [selectedId,setSelectedId]=useState('')
 const [editedPlan,setEditedPlan]=useState('')
 const [job,setJob]=useState<Job|null>(null)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 const ordered=useMemo(()=>[...goals.value].sort((a,b)=>(b.createdAt||'').localeCompare(a.createdAt||'')),[goals.value])
 const selected=ordered.find(item=>item.id===selectedId)||ordered[0]
 const hasJob=selected?.jobId
 useEffect(()=>{
  const id=window.setInterval(()=>{if(document.visibilityState==='visible')void goals.reload()},3500)
  return()=>window.clearInterval(id)
 },[goals.reload])
 useEffect(()=>{setEditedPlan(selected?.plan||'')},[selected?.id,selected?.plan])
 useEffect(()=>{
  if(!hasJob){setJob(null);return}
  let alive=true
  const load=async()=>{try{const value=await get<Job>('/api/jobs/'+encodeURIComponent(hasJob));if(alive)setJob(value)}catch{if(alive)setJob(null)}}
  void load()
  const id=window.setInterval(()=>{if(document.visibilityState==='visible')void load()},3500)
  return()=>{alive=false;window.clearInterval(id)}
 },[hasJob])
 const start=async()=>{
  if(!goalText.trim()||busy)return
  setError('');setNotice('');setBusy(true)
  try{
   const criteria=kind==='auto'?[]:kind==='command_succeeded'
    ?[{type:kind,command:criterion.trim()}]:[{type:kind,path:criterion.trim()}]
   if(kind!=='auto'&&!criterion.trim())throw new Error('검증 조건을 입력하세요.')
   const data=await post<Goal>('/api/goals',{goal:goalText.trim(),
    executor:agentId?{agentId}:{},successCriteria:criteria,
    maxIterations:16,maxReplans:6,replanAfter:2})
   setSelectedId(data.id);setGoalText('');setCriterion('')
   await goals.reload();setNotice('목표를 등록하고 자율 실행 대기열에 넣었습니다.')
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const savePlan=async()=>{
  if(!selected||busy)return
  setBusy(true);setError('')
  try{await post('/api/goals/'+encodeURIComponent(selected.id)+'/plan',{plan:editedPlan})
   await goals.reload();setNotice('실행 전에 계획을 수정했습니다.')
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const stop=async()=>{
  if(!selected||busy)return
  setBusy(true);setError('')
  try{await post('/api/goals/'+encodeURIComponent(selected.id)+'/cancel',{})
   await goals.reload();setNotice('목표와 연결된 작업에 중단을 요청했습니다.')
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const resume=async()=>{
  if(!selected||busy||!confirm('이전 실행의 파일·원격 작업 결과를 직접 확인하셨습니까? 확인하지 않은 외부 작업은 자동으로 반복하지 않으며, 읽기 전용 재확인부터 시작합니다.'))return
  setBusy(true);setError('')
  try{await post('/api/goals/'+encodeURIComponent(selected.id)+'/resume',{acknowledgeUnknownEffects:true})
   await goals.reload();setNotice('기존 증거·세션을 보존한 새 실행 작업을 등록했습니다.')
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const remove=async()=>{
  if(!selected||busy||!confirm('종료된 목표와 실행 요약을 목록에서 지우시겠습니까?'))return
  setBusy(true);setError('')
  try{await del('/api/goals/'+encodeURIComponent(selected.id));setSelectedId('');setJob(null);await goals.reload()}
  catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const report=selected?.finalReport||job?.result?.finalReport
 const active=selected?.status==='queued'||selected?.status==='running'
 return <div className="mx-auto w-full max-w-[1380px] px-5 py-5 sm:px-7 lg:pb-8">
  <Headline eyebrow="AUTONOMOUS AGENT" title="자율 목표 실행" description="목표 하나로 분석 → 계획 → 도구 실행 → 검증 → 실패 수정 → 결과물 전달. 완료 판정은 실제 도구와 최종 검증 조건으로 결정합니다." actions={<Button variant="ghost" onClick={()=>void goals.reload()}><RefreshCw size={15}/> 새로고침</Button>}/>
  {error&&<Notice message={error}/>}
  {notice&&<p role="status" className="mb-4 rounded-lg border border-[#E1D7C9] bg-[#FAF7F1] p-3 text-xs">{notice}</p>}
  <div className="grid grid-cols-1 gap-5 lg:grid-cols-[330px_minmax(0,1fr)]">
   <div className="flex min-w-0 flex-col gap-4">
    <Panel className="flex flex-col gap-3 p-4">
     <h2 className="text-sm font-bold">새 목표</h2>
     <FormField label="무엇을 완성할까요?"><TextArea rows={5} value={goalText} onChange={e=>setGoalText(e.target.value)} placeholder="예: 이 프로젝트의 로그인 오류를 찾아 수정하고 검증 결과를 보고해 주세요."/></FormField>
     <FormField label="실행 담당 에이전트"><SelectInput value={agentId} onChange={e=>setAgentId(e.target.value)}><option value="">기본 실행 모델 사용</option>{agents.value.map(agent=><option key={agent.id} value={agent.id}>{agent.name}{agent.model?' · '+agent.model:''}</option>)}</SelectInput></FormField>
     <FormField label="완료 검증 조건"><SelectInput value={kind} onChange={e=>setKind(e.target.value as VerifyKind)}><option value="auto">요청에서 자동 추론</option><option value="file_exists">파일 존재</option><option value="file_changed">파일 변경</option><option value="command_succeeded">명령어 성공 (실제 실행 로그)</option></SelectInput></FormField>
     {kind!=='auto'&&<FormField label={kind==='command_succeeded'?'검증 명령어':'워크스페이스 파일 경로'}><TextInput value={criterion} onChange={e=>setCriterion(e.target.value)} placeholder={kind==='command_succeeded'?'cargo test': 'src/main.rs'}/></FormField>}
     <Button disabled={busy||!goalText.trim()} onClick={()=>void start()}><Play size={15}/> 목표 실행</Button>
     <p className="text-[11px] leading-5 text-[#8A8176]">검증 명령어를 선택하면 실제 성공 기록이 있어야 완료됩니다. 실행 중 도구 승인 정책은 기존 엔진 설정을 따릅니다.</p>
    </Panel>
    <Panel className="overflow-hidden">
     <h2 className="border-b border-[#EFE9DF] px-4 py-3 text-sm font-bold">목표 목록 · {ordered.length}</h2>
     <div className="max-h-[440px] overflow-y-auto">{ordered.map(goal=><button type="button" key={goal.id} onClick={()=>{setSelectedId(goal.id);setError('')}} className={'flex w-full flex-col gap-1 border-b border-[#F0EBE3] px-4 py-3 text-left hover:bg-[#FAF7F1] '+(selected?.id===goal.id?'bg-[#FAF2E8]':'')}>
      <span className="line-clamp-2 text-[12px] font-semibold">{goal.goal}</span>
      <span className="text-[11px] text-[#8A8176]">{goal.status} · {goal.iterations||0}/{goal.maxIterations||16}회</span>
     </button>)}{!ordered.length&&<p className="p-4 text-xs text-[#8A8176]">등록된 자율 목표가 없습니다.</p>}</div>
    </Panel>
   </div>
   <div className="min-w-0 space-y-4">
    {!selected?<Panel className="p-8 text-center text-sm text-[#8A8176]">목표를 만들면 계획·도구 실행·검증 결과가 이곳에 표시됩니다.</Panel>:<>
     <Panel className="space-y-4 p-5">
      <div className="flex flex-wrap items-center justify-between gap-2"><div><p className="text-[11px] font-semibold text-[#8A8176]">실행 작업 · {selected.id.slice(0,10)}</p><h2 className="mt-1 text-lg font-bold">{selected.goal}</h2></div><StatusPill status={selected.status}>{selected.status}</StatusPill></div>
      <div className="flex flex-wrap items-center gap-2 text-xs text-[#7C7469]"><span>단계: {selected.phase||'대기'}</span><span>반복 {selected.iterations||0}/{selected.maxIterations||16}</span><span>재계획 {selected.replans||0}회</span>{job?.status&&<span>작업: {job.status} ({job.stepIndex||0}/{job.stepTotal||16})</span>}</div>
      <div className="flex flex-wrap gap-2">{selected.sessionId&&<Button variant="ghost" onClick={()=>openSession(selected.sessionId!)}>실제 실행 대화 보기 <ArrowRight size={14}/></Button>}{active&&<Button variant="ghost" disabled={busy} onClick={()=>void stop()}><Square size={13}/> 중단</Button>}
       {(selected.status==='needs_review'||selected.status==='failed')&&<Button variant="ghost" disabled={busy} onClick={()=>void resume()}><RotateCw size={14}/> 외부 결과 확인 후 이어가기</Button>}
       {!active&&<Button variant="ghost" disabled={busy} onClick={()=>void remove()}><Trash2 size={14}/> 기록 삭제</Button>}</div>
      {job?.error&&<Notice message={job.error}/>}
     </Panel>
     <Panel className="space-y-3 p-5"><h3 className="flex items-center gap-2 text-sm font-bold"><Wrench size={16}/> 작업 계획 · {selected.planHistory?.length||0}개 버전</h3>
      {selected.status==='queued'?<><TextArea rows={6} value={editedPlan} onChange={e=>setEditedPlan(e.target.value)} placeholder="선택 사항: 실행 전에 계획을 직접 입력하거나 수정하세요. 비우면 계획 모델이 작성합니다."/><Button disabled={busy||!editedPlan.trim()} onClick={()=>void savePlan()}><Check size={14}/> 계획 저장</Button></>:<pre className="max-h-64 overflow-auto whitespace-pre-wrap rounded-xl bg-[#FAF7F1] p-4 text-xs leading-6">{selected.plan||'계획 작성 대기 중'}</pre>}
      {!!selected.planHistory?.length&&<details className="text-xs text-[#7C7469]"><summary className="cursor-pointer">재계획 이력</summary>{selected.planHistory.map((item,i)=><div key={i} className="mt-2 border-l-2 border-[#E9D6C5] pl-3"><strong>계획 v{item.version}</strong><pre className="mt-1 whitespace-pre-wrap">{item.plan}</pre></div>)}</details>}
     </Panel>
     <Panel className="space-y-3 p-5"><h3 className="flex items-center gap-2 text-sm font-bold"><ShieldCheck size={16}/> 검증·증거</h3>
      {report?<><p className={'text-xs font-bold '+(report.accepted?'text-emerald-700':'text-amber-700')}>{report.accepted?'최종 검증 조건 충족':'미충족 검증 조건 존재'} · 성공 도구 {report.successfulToolEvidence}/{report.toolEvidenceRecorded}</p>
       {report.checks.map((check,i)=><div key={i} className="flex items-start gap-2 rounded-lg bg-[#FAF7F1] p-3 text-xs"><span>{check.passed?'✅':'⚠️'}</span><span><strong>{check.type}</strong> {check.path||check.command}{check.gaps?.map((g,j)=><small key={j} className="block text-[#9A5947]">{g}</small>)}</span></div>)}
       {!!report.artifacts.length&&<div><h4 className="mb-2 text-xs font-semibold">확인된 파일 산출물</h4>{report.artifacts.map(file=><div key={file.path} className="mb-2 rounded-lg border border-[#EEE5D9] p-3 text-xs"><FileCheck2 size={14} className="mr-2 inline"/><strong>{file.path}</strong><span className="ml-2 text-[#8A8176]">{file.bytes.toLocaleString()} bytes</span>{file.sha256&&<code className="mt-1 block break-all text-[10px] text-[#8A8176]">SHA-256 {file.sha256}</code>}</div>)}</div>}
       {!!report.operations.length&&<details className="text-xs"><summary className="cursor-pointer">성공한 실행 도구 {report.operations.length}개</summary>{report.operations.map((op,i)=><p key={i} className="mt-2 rounded-md bg-[#FAF7F1] p-2">{op.tool}{op.path?' · '+op.path:''}{op.exitCode!==undefined?' · 종료 코드 '+op.exitCode:''}</p>)}</details>}
      </>:<p className="text-xs text-[#8A8176]">최종 검증 보고서는 실행 종료 시 생성됩니다. 중간 단계 증거 {selected.evidence?.length||0}건을 기록 중입니다.</p>}
      {!!selected.gaps?.length&&<div className="rounded-xl border border-amber-200 bg-amber-50 p-3 text-xs"><strong>남은 문제</strong>{selected.gaps.map((gap,i)=><p className="mt-1" key={i}>{gap}</p>)}</div>}
      {!!selected.reviewHistory?.length&&<details className="text-xs"><summary className="cursor-pointer">독립 검토 이력 {selected.reviewHistory.length}건</summary>{selected.reviewHistory.map((x,i)=><div className="mt-2 border-l-2 border-[#E9D6C5] pl-3" key={i}>검토 #{x.iteration} · {x.complete?'모델 통과 제안':'추가 수정 요청'}{x.gaps?.map((g,j)=><p key={j}>{g}</p>)}</div>)}</details>}
     </Panel>
    </>}
   </div>
  </div>
 </div>
}
