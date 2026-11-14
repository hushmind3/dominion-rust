import {useEffect,useMemo,useState} from 'react'
import {ArrowRight,Check,FileCheck2,Play,RefreshCw,RotateCw,ShieldCheck,Square,Trash2,Wrench} from 'lucide-react'
import {get,post,del} from '../api'
import {Button,SelectInput,TextArea,TextInput} from '../components/Primitives'
import {StatusPill} from '../components/StatusPill'
import {FormField,Headline,Notice,Panel,useRemote} from '../components/UI'

type Agent={id:string;name:string;model?:string}
type CheckResult={type?:string;path?:string|null;command?:string|null;passed?:boolean;gaps?:string[]}
type Artifact={path:string;bytes:number;sha256?:string;hashOmitted?:boolean}
type GitChanges={baselineHead?:string;currentHead?:string;initialStatus?:string;currentStatus?:string;changedFiles?:{change:string;path:string}[];diffStat?:string;diffPreview?:string|null;diffPreviewTruncated?:boolean}
type IsolatedChanges={goalId:string;branch:string;baseCommit:string;status:string;diff:string;untracked?:string;untrackedPreviews?:{path:string;bytes:number;content?:string;previewOmitted?:boolean}[];reviewIncomplete?:boolean;truncated?:boolean;reviewDigest:string}
type GoalReport={gitChanges?:GitChanges|null;accepted:boolean;checks:CheckResult[];gaps:string[];artifacts:Artifact[];successfulToolEvidence:number;toolEvidenceRecorded:number;tokenBudget?:number;sessionUsage?:{calls?:number;totalTokens?:number;promptTokens?:number;completionTokens?:number;cachedPromptTokens?:number};operations:{tool:string;path?:string|null;exitCode?:number|null}[]}
type Goal={id:string;goal:string;status:string;phase?:string;createdAt?:string;jobId?:string;sessionId?:string;plan?:string;isolateWorktree?:boolean;gitWorktree?:{path:string;branch:string;baseCommit:string;rolledBack?:boolean;applied?:boolean;appliedCommit?:string};
 iterations?:number;maxIterations?:number;tokenBudget?:number;replans?:number;planRevision?:number;appliedPlanRevision?:number;ownerPlan?:{revision:number;plan:string};planHistory?:{version:number;plan:string}[];
 reviewHistory?:{iteration:number;complete:boolean;gaps:string[]}[];
 evidence?:unknown[];gaps?:string[];acceptanceChecks?:unknown[];finalReport?:GoalReport}
type Job={id:string;status:string;stepIndex?:number;stepTotal?:number;error?:string;result?:{status?:string;finalReport?:GoalReport}}
type GoalMetrics={source:string;proofLevel:string;totalGoals:number;settledGoals:number;internalAcceptedGoals:number;pendingOrRunningGoals:number;completedWithoutValidJobOrAcceptance:number;
 internalAcceptanceRateAmongSettled:number|null;totalIterations:number;totalReplans:number;
 usage:{calls:number;estimatedCalls:number;totalTokens:number;cachedPromptTokens:number;cacheEligiblePromptTokens:number;
  sessionsWithAudit:number;goalsWithoutSession:number;sessionsWithoutUsageAudit:number;sharedSessionLinks:number;
  completeAttribution:boolean;recordedTokensPerInternalAccepted:number|null;actualBilledKrw:number|null;actualBilledKrwPerSuccess:number|null}}
type VerifyKind='auto'|'file_exists'|'file_changed'|'command_succeeded'

export function GoalsPage({openSession}:{openSession:(id:string)=>void}){
 const goals=useRemote<Goal[]>('/api/goals',[])
 const metrics=useRemote<GoalMetrics|null>('/api/goals/metrics',null,false)
 const agents=useRemote<Agent[]>('/api/agents',[])
 const [goalText,setGoalText]=useState('')
 const [agentId,setAgentId]=useState('')
 const [kind,setKind]=useState<VerifyKind>('auto')
 const [tokenBudget,setTokenBudget]=useState(150000)
 const [isolateWorktree,setIsolateWorktree]=useState(true)
 const [changes,setChanges]=useState<IsolatedChanges|null>(null)
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
  const id=window.setInterval(()=>{if(document.visibilityState==='visible')void metrics.reload()},15000)
  return()=>window.clearInterval(id)
 },[metrics.reload])
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
    maxIterations:16,maxReplans:6,replanAfter:2,tokenBudget,isolateWorktree})
   setSelectedId(data.id);setGoalText('');setCriterion('')
   await goals.reload();setNotice('목표를 등록하고 자율 실행 대기열에 넣었습니다.')
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const savePlan=async()=>{
  if(!selected||busy)return
  setBusy(true);setError('')
  try{
   const live=selected.status==='running'
   await post('/api/goals/'+encodeURIComponent(selected.id)+(live?'/steer':'/plan'),
    live?{plan:editedPlan,expectedRevision:selected.planRevision||0}:{plan:editedPlan})
   await goals.reload();setNotice(live?'새 계획을 접수했습니다. 다음 안전한 실행 턴에서 적용됩니다.':'실행 전에 계획을 수정했습니다.')
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
 const reviewChanges=async()=>{
  if(!selected||busy)return
  setBusy(true);setError('');setChanges(null)
  try{setChanges(await get<IsolatedChanges>('/api/goals/'+encodeURIComponent(selected.id)+'/changes'))}
  catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const applyReviewed=async()=>{
  if(!selected||!changes||busy||changes.reviewIncomplete||changes.truncated)return
  if(!confirm('검토한 Git 변경을 원본 브랜치에 fast-forward 반영하시겠습니까? 원본 HEAD와 파일 상태가 목표 시작 시점과 같아야 합니다.'))return
  setBusy(true);setError('')
  try{await post('/api/goals/'+encodeURIComponent(selected.id)+'/apply',{reviewDigest:changes.reviewDigest,acknowledgeReview:true})
   setChanges(null);await goals.reload();setNotice('검토한 격리 브랜치 변경을 원본에 반영했습니다.')}
  catch(e){setError((e as Error).message);setChanges(null)}finally{setBusy(false)}
 }
 const discardReviewed=async()=>{
  if(!selected||busy||!confirm('격리 작업공간의 미반영 변경을 폐기하시겠습니까? 원본 저장소는 유지됩니다.'))return
  setBusy(true);setError('')
  try{await post('/api/goals/'+encodeURIComponent(selected.id)+'/discard',{})
   setChanges(null);await goals.reload();setNotice('격리 작업공간을 제거하고 원본 저장소를 유지했습니다.')}
  catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const report=selected?.finalReport||job?.result?.finalReport
 const active=selected?.status==='queued'||selected?.status==='running'
 useEffect(()=>{setChanges(null)},[selected?.id])
 return <div className="mx-auto w-full max-w-[1380px] px-5 py-5 sm:px-7 lg:pb-8">
  <Headline eyebrow="AUTONOMOUS AGENT" title="자율 목표 실행" description="목표 하나로 분석 → 계획 → 도구 실행 → 검증 → 실패 수정 → 결과물 전달. 완료 판정은 실제 도구와 최종 검증 조건으로 결정합니다." actions={<Button variant="ghost" onClick={()=>void goals.reload()}><RefreshCw size={15}/> 새로고침</Button>}/>
  {error&&<Notice message={error}/>}
  {notice&&<p role="status" className="mb-4 rounded-lg border border-[#E1D7C9] bg-[#FAF7F1] p-3 text-xs">{notice}</p>}
  <Panel className="mb-5 space-y-3 p-4">
   <div className="flex items-center justify-between gap-2"><h2 className="text-sm font-bold">📊 실행 성과 원장</h2><Button variant="ghost" onClick={()=>void metrics.reload()}><RefreshCw size={13}/> 기록 재집계</Button></div>
   {metrics.value?<><div className="grid grid-cols-2 gap-3 text-xs sm:grid-cols-4">
    <div><p className="text-[#8A8176]">내부 검증 통과</p><p className="text-lg font-bold">{metrics.value.internalAcceptedGoals}/{metrics.value.settledGoals}건</p></div>
    <div><p className="text-[#8A8176]">기록상 완료 비율</p><p className="text-lg font-bold">{metrics.value.internalAcceptanceRateAmongSettled===null?'표본 없음':(metrics.value.internalAcceptanceRateAmongSettled*100).toFixed(1)+'%'}</p></div>
    <div><p className="text-[#8A8176]">기록된 모델 토큰</p><p className="text-lg font-bold">{metrics.value.usage.totalTokens.toLocaleString('ko-KR')}</p></div>
    <div><p className="text-[#8A8176]">내부 통과 1건당 토큰</p><p className="text-lg font-bold">{metrics.value.usage.recordedTokensPerInternalAccepted===null?'계측 불충분':Math.round(metrics.value.usage.recordedTokensPerInternalAccepted).toLocaleString('ko-KR')}</p></div>
   </div>
   <p className="text-[11px] leading-5 text-[#8A8176]">총 {metrics.value.totalGoals}개 목표 · 진행 중 {metrics.value.pendingOrRunningGoals}개 · 재계획 {metrics.value.totalReplans}회 · 모델 호출 {metrics.value.usage.calls}회 (추정 사용량 {metrics.value.usage.estimatedCalls}회)</p>
   <p className="text-[11px] leading-5 text-[#8A8176]">캐시 보고 입력 {metrics.value.usage.cachedPromptTokens.toLocaleString('ko-KR')}/{metrics.value.usage.cacheEligiblePromptTokens.toLocaleString('ko-KR')}토큰. 사용량 로그 없는 세션 {metrics.value.usage.sessionsWithoutUsageAudit}개, 세션 누락 {metrics.value.usage.goalsWithoutSession}개. 공급자 실제 청구액: 계측되지 않음.</p>
   <p className="text-[11px] font-semibold leading-5 text-[#9A5947]">이 수치는 로컬 SQLite의 목표·실행 작업·모델 감사 로그에 기반한 내부 실행 관측입니다. 실모델 독립 벤치마크나 외부 작업 완수율 인증이 아닙니다.</p>
   </>:<p className="text-xs text-[#8A8176]">{metrics.error||'실행 기록을 집계하고 있습니다.'}</p>}
  </Panel>
  <div className="grid grid-cols-1 gap-5 lg:grid-cols-[330px_minmax(0,1fr)]">
   <div className="flex min-w-0 flex-col gap-4">
    <Panel className="flex flex-col gap-3 p-4">
     <h2 className="text-sm font-bold">새 목표</h2>
     <FormField label="무엇을 완성할까요?"><TextArea rows={5} value={goalText} onChange={e=>setGoalText(e.target.value)} placeholder="예: 이 프로젝트의 로그인 오류를 찾아 수정하고 검증 결과를 보고해 주세요."/></FormField>
     <FormField label="실행 담당 에이전트"><SelectInput value={agentId} onChange={e=>setAgentId(e.target.value)}><option value="">기본 실행 모델 사용</option>{agents.value.map(agent=><option key={agent.id} value={agent.id}>{agent.name}{agent.model?' · '+agent.model:''}</option>)}</SelectInput></FormField>
     <FormField label="완료 검증 조건"><SelectInput value={kind} onChange={e=>setKind(e.target.value as VerifyKind)}><option value="auto">요청에서 자동 추론</option><option value="file_exists">파일 존재</option><option value="file_changed">파일 변경</option><option value="command_succeeded">명령어 성공 (실제 실행 로그)</option></SelectInput></FormField>
     <label className="flex items-center gap-2 text-xs font-medium"><input type="checkbox" checked={isolateWorktree} onChange={e=>setIsolateWorktree(e.target.checked)}/> 🛡️ 코딩 목표 Git worktree 격리 (기본 사용)</label>
     <p className="text-[11px] text-[#8A8176]">Git 루트·깨끗한 원본 작업공간이 필요합니다. 에이전트는 별도 checkout에서 파일을 수정하고, 원본 반영은 변경 검토 후 직접 승인합니다. OS 보안 샌드박스는 아닙니다.</p>
     <FormField label="목표 실행 토큰 예산" hint={undefined}><TextInput type="number" min={0} max={2000000} step={10000} value={tokenBudget} onChange={e=>setTokenBudget(Math.min(2000000,Math.max(0,Number(e.target.value)||0)))}/></FormField>
     <p className="text-[11px] leading-5 text-[#8A8176]">기본 15만 토큰. 0은 제한 없음. Provider가 기록한 세션 실행 사용량을 기준으로 다음 턴을 제한하며, 별도 계획·검토 호출의 전체 청구액 상한은 아닙니다.</p>
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
      <div className="flex flex-wrap items-center gap-2 text-xs text-[#7C7469]"><span>단계: {selected.phase||'대기'}</span><span>반복 {selected.iterations||0}/{selected.maxIterations||16}</span><span>재계획 {selected.replans||0}회</span><span>토큰 예산 {selected.tokenBudget===0?'제한 없음':(selected.tokenBudget||150000).toLocaleString()}</span>{job?.status&&<span>작업: {job.status} ({job.stepIndex||0}/{job.stepTotal||16})</span>}</div>
      <div className="flex flex-wrap gap-2">{selected.sessionId&&<Button variant="ghost" onClick={()=>openSession(selected.sessionId!)}>실제 실행 대화 보기 <ArrowRight size={14}/></Button>}{active&&<Button variant="ghost" disabled={busy} onClick={()=>void stop()}><Square size={13}/> 중단</Button>}
       {(selected.status==='needs_review'||selected.status==='failed')&&<Button variant="ghost" disabled={busy} onClick={()=>void resume()}><RotateCw size={14}/> 외부 결과 확인 후 이어가기</Button>}
       {!active&&<Button variant="ghost" disabled={busy} onClick={()=>void remove()}><Trash2 size={14}/> 기록 삭제</Button>}</div>
      {job?.error&&<Notice message={job.error}/>}
     </Panel>
     {selected.isolateWorktree&&<Panel className="space-y-3 p-5">
      <h3 className="text-sm font-bold">🛡️ Git 격리 작업공간</h3>
      <p className="text-xs text-[#7C7469]">{selected.gitWorktree?.branch||'작업공간 준비 중'} · 기준 {selected.gitWorktree?.baseCommit?.slice(0,10)||'—'}</p>
      <p className="break-all text-[11px] text-[#8A8176]">{selected.gitWorktree?.path||'원본 checkout을 직접 수정하지 않습니다.'}</p>
      <div className="flex flex-wrap gap-2">
       {selected.gitWorktree&&!selected.gitWorktree.rolledBack&&<Button variant="secondary" disabled={busy} onClick={()=>void reviewChanges()}>변경 미리보기</Button>}
       {!active&&selected.gitWorktree&&!selected.gitWorktree.applied&&!selected.gitWorktree.rolledBack&&<Button variant="ghost" disabled={busy} onClick={()=>void discardReviewed()}>격리 변경 폐기</Button>}
      </div>
      {selected.gitWorktree?.applied&&<p className="text-xs text-emerald-700">원본 브랜치 반영 완료 · {selected.gitWorktree.appliedCommit?.slice(0,12)}</p>}
      {selected.gitWorktree?.rolledBack&&<p className="text-xs text-[#8A8176]">격리 작업공간 폐기됨 · 원본 유지</p>}
      {changes&&<div className="space-y-2 rounded-lg border border-[#EEE5D9] p-3 text-xs">
       <p className="font-semibold">검토할 Git 변경 · {changes.branch}</p>
       <p className="text-[#8A8176]">원본 기준 {changes.baseCommit.slice(0,10)} · 검토 영수증 {changes.reviewDigest.slice(0,16)}…</p>
       {changes.status&&<pre className="max-h-36 overflow-auto whitespace-pre-wrap rounded bg-[#FAF7F1] p-2">{changes.status}</pre>}
       {changes.diff&&<details open><summary className="cursor-pointer font-semibold">수정·삭제 파일 diff</summary><pre className="mt-2 max-h-80 overflow-auto whitespace-pre-wrap break-all rounded bg-[#FAF7F1] p-2">{changes.diff}</pre></details>}
       {!!changes.untrackedPreviews?.length&&<details open><summary className="cursor-pointer font-semibold">새 파일 내용 {changes.untrackedPreviews.length}개</summary>{changes.untrackedPreviews.map((file,i)=><div className="mt-2" key={i}><strong>{file.path} · {file.bytes}바이트</strong>{file.content!==undefined?<pre className="max-h-60 overflow-auto whitespace-pre-wrap break-all rounded bg-[#FAF7F1] p-2">{file.content}</pre>:<p className="text-amber-700">미리보기 생략 · 자동 반영 불가</p>}</div>)}</details>}
       {(changes.truncated||changes.reviewIncomplete)&&<p className="text-amber-700">일부 변경 내용이 생략돼 원본 반영을 차단했습니다. 변경 범위를 줄여 다시 검토하세요.</p>}
       {!active&&['completed','needs_review'].includes(selected.status)&&!changes.truncated&&!changes.reviewIncomplete&&!selected.gitWorktree?.applied&&<Button disabled={busy||!changes.reviewDigest} onClick={()=>void applyReviewed()}><Check size={14}/> 이 변경을 검토하고 원본에 반영</Button>}
      </div>}
     </Panel>}
     <Panel className="space-y-3 p-5"><h3 className="flex items-center gap-2 text-sm font-bold"><Wrench size={16}/> 작업 계획 · {selected.planHistory?.length||0}개 버전</h3>
      {(selected.status==='queued'||selected.status==='running')?<><TextArea rows={6} value={editedPlan} onChange={e=>setEditedPlan(e.target.value)} placeholder={selected.status==='running'?'실행 중 수정할 새 계획. 다음 실행 턴에서 적용됩니다.':'선택 사항: 실행 전에 계획을 직접 입력하거나 수정하세요.'}/><Button disabled={busy||!editedPlan.trim()} onClick={()=>void savePlan()}><Check size={14}/> {selected.status==='running'?'계획 개정 요청':'계획 저장'}</Button>
       {selected.status==='running'&&<p className="text-xs text-[#7C7469]">현재 반영 revision {selected.appliedPlanRevision||0} · 최신 제출 revision {selected.planRevision||0}. 실행 중 승인·완료 검증 조건은 바뀌지 않습니다.</p>}
       {(selected.planRevision||0)>(selected.appliedPlanRevision||0)&&<p className="text-xs text-amber-700">📌 새 계획 적용 대기 중 · 현재 도구 작업은 종료 후 다음 턴에서 적용</p>}
      </>:<pre className="max-h-64 overflow-auto whitespace-pre-wrap rounded-xl bg-[#FAF7F1] p-4 text-xs leading-6">{selected.plan||'계획 작성 대기 중'}</pre>}
      {!!selected.planHistory?.length&&<details className="text-xs text-[#7C7469]"><summary className="cursor-pointer">재계획 이력</summary>{selected.planHistory.map((item,i)=><div key={i} className="mt-2 border-l-2 border-[#E9D6C5] pl-3"><strong>계획 v{item.version}</strong><pre className="mt-1 whitespace-pre-wrap">{item.plan}</pre></div>)}</details>}
     </Panel>
     <Panel className="space-y-3 p-5"><h3 className="flex items-center gap-2 text-sm font-bold"><ShieldCheck size={16}/> 검증·증거</h3>
      {report?<><p className={'text-xs font-bold '+(report.accepted?'text-emerald-700':'text-amber-700')}>{report.accepted?'최종 검증 조건 충족':'미충족 검증 조건 존재'} · 성공 도구 {report.successfulToolEvidence}/{report.toolEvidenceRecorded}</p>
       {report.sessionUsage&&<p className="text-xs text-[#7C7469]">모델 실행 사용량 {Number(report.sessionUsage.totalTokens||0).toLocaleString()}토큰 · {Number(report.sessionUsage.calls||0)}회 · 캐시 적중 입력 {Number(report.sessionUsage.cachedPromptTokens||0).toLocaleString()}토큰</p>}
       {report.checks.map((check,i)=><div key={i} className="flex items-start gap-2 rounded-lg bg-[#FAF7F1] p-3 text-xs"><span>{check.passed?'✅':'⚠️'}</span><span><strong>{check.type}</strong> {check.path||check.command}{check.gaps?.map((g,j)=><small key={j} className="block text-[#9A5947]">{g}</small>)}</span></div>)}
       {!!report.artifacts.length&&<div><h4 className="mb-2 text-xs font-semibold">확인된 파일 산출물</h4>{report.artifacts.map(file=><div key={file.path} className="mb-2 rounded-lg border border-[#EEE5D9] p-3 text-xs"><FileCheck2 size={14} className="mr-2 inline"/><strong>{file.path}</strong><span className="ml-2 text-[#8A8176]">{file.bytes.toLocaleString()} bytes</span>{file.sha256&&<code className="mt-1 block break-all text-[10px] text-[#8A8176]">SHA-256 {file.sha256}</code>}</div>)}</div>}
       {report.gitChanges&&<details className="rounded-lg border border-[#EEE5D9] p-3 text-xs">
        <summary className="cursor-pointer font-semibold">Git 변경 기록 · {report.gitChanges.changedFiles?.length||0}개 추적 파일</summary>
        <p className="mt-2 text-[#8A8176]">기준 {report.gitChanges.baselineHead?.slice(0,10)||'초기 커밋 없음'} → 현재 {report.gitChanges.currentHead?.slice(0,10)||'정보 없음'}</p>
        <p className="my-1 text-[#8A8176]">Git 변경 기록은 저장소 관측 결과이며 특정 에이전트의 작업 증명과는 구별됩니다.</p>
        {!!report.gitChanges.changedFiles?.length&&<ul className="my-2 list-inside list-disc">{report.gitChanges.changedFiles.map((file,i)=><li key={i}>{file.change} · {file.path}</li>)}</ul>}
        {report.gitChanges.diffStat&&<pre className="overflow-auto whitespace-pre-wrap rounded-md bg-[#FAF7F1] p-3">{report.gitChanges.diffStat}</pre>}
        {report.gitChanges.currentStatus&&<pre className="mt-2 max-h-36 overflow-auto whitespace-pre-wrap text-[#8A8176]">{report.gitChanges.currentStatus}</pre>}
        {report.gitChanges.diffPreview&&<details className="mt-2"><summary className="cursor-pointer">변경 diff 미리 보기</summary><pre className="mt-2 max-h-96 overflow-auto whitespace-pre-wrap break-all rounded-md bg-[#FAF7F1] p-3">{report.gitChanges.diffPreview}</pre></details>}
        {report.gitChanges.diffPreviewTruncated&&<p className="text-amber-700">변경 diff가 커서 요약만 기록했습니다.</p>}
       </details>}
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
