import {useEffect,useState} from 'react'
import {BrainCircuit,CheckCircle2,ClipboardCheck,Play,RefreshCw,Square,Terminal} from 'lucide-react'
import {Provider,get,post,resolveApproval} from '../api'
import {ModelPicker} from '../components/ModelPicker'
import {Button,SelectInput,TextArea} from '../components/Primitives'
import {FormField,Headline,Notice,Panel,StateTag,useRemote} from '../components/UI'

type DevStatus={
 config:{sourceDir:string};git:boolean;branch:string;dirty:number;exists:boolean
}
type RsiEvidence={
 candidateId?:string|null;baseCommit?:string;diffSha256?:string;diffBytes?:number;
 sourceClean?:boolean;checksPassed?:boolean;pairedComparisonComplete?:boolean;
 checks?:{label:string;ran:boolean;passed:boolean;stdout?:string;stderr?:string;error?:string}[];
 modelError?:string;captureError?:string
}
type Job={
 id:string;status:string;sessionId?:string;error?:string;
 result?:{sessionId?:string;messageId?:string;rsi?:RsiEvidence;repairAttempts?:number;reason?:string};promptState?:{inFlight?:boolean}
}
type RsiCandidate={id:string;status:string;target?:string;patch?:string;paths?:string[];patchSha256?:string;baseCommit?:string;reviewRequired?:boolean}
type PairedJob={id:string;status:string;error?:string;result?:{
 verdict?:string;candidateStatus?:string;
 metrics?:{pairedCases:number;gains:number;regressions:number;holdoutCases:number;eligible:boolean};
}}
type Message={id:string;role:string;content:string;created_at:string}
type Approval={id:string;sessionId:string;status:string;tool:string;args:unknown}
const SAVED_JOB='dominion.rsi.activeJob'
const terminal=(status:string)=>['completed','failed','cancelled','needs_review'].includes(status)
const operationLabel=(approval:Approval)=>{ const args=approval.args as Record<string,unknown>|null; const value=args&&typeof args==='object'?args:null; const detail=value?.path||value?.cwd||value?.command||''; return typeof detail==='string'&&detail.trim()?detail.slice(0,240):'선택한 도구의 실행을 승인할까요?' }
const defaultTask='예: 장기기억·도구 호출·토큰 비용 중 하나를 중점적으로 진단해 주세요. (비워두면 전체 소스를 스스로 진단합니다.)'

function CandidateReview({id,canApply,afterApply}:{id:string;canApply:boolean;afterApply:()=>void}){
 const candidate=useRemote<RsiCandidate>('/api/rsi/'+encodeURIComponent(id),{id,status:'proposed'})
 const [applying,setApplying]=useState(false)
 const [applyError,setApplyError]=useState('')
 const [applied,setApplied]=useState(false)
 const compareKey='dominion.rsi.compare.'+id
 const [comparisonId,setComparisonId]=useState(()=>{
  try{return localStorage.getItem(compareKey)||''}catch{return ''}
 })
 const [comparison,setComparison]=useState<PairedJob|null>(null)
 const [comparing,setComparing]=useState(false)
 const [compareError,setCompareError]=useState('')
 useEffect(()=>{
  if(!comparisonId)return
  let alive=true
  const refresh=async()=>{
   try{
    const job=await get<PairedJob>('/api/jobs/'+encodeURIComponent(comparisonId))
    if(!alive)return
    setComparison(job)
    if(['completed','failed','cancelled','needs_review'].includes(job.status))void candidate.reload()
   }catch(e){if(alive)setCompareError((e as Error).message)}
  }
  void refresh()
  const timer=window.setInterval(()=>void refresh(),2800)
  return()=>{alive=false;window.clearInterval(timer)}
 },[comparisonId])
 const compare=async()=>{
  if(comparing)return
  if(!window.confirm('고정된 8개 Rust 테스트를 원본·수정본 2개 Git 작업공간에서 실행합니다. 내 컴퓨터에서 모델이 수정한 Rust 코드를 테스트하므로 CPU·시간을 사용합니다. 실행하시겠습니까?'))return
  setComparing(true);setCompareError('')
  try{
   const job=await post<PairedJob>('/api/rsi/'+encodeURIComponent(id)+'/compare',{})
   setComparisonId(job.id);setComparison(job)
   try{localStorage.setItem(compareKey,job.id)}catch{}
  }catch(e){setCompareError((e as Error).message)}finally{setComparing(false)}
 }
 const apply=async()=>{
  if(!candidate.value.baseCommit||!candidate.value.patchSha256)return
  if(!window.confirm('성능 향상 비교가 완료되지 않았습니다. 컴파일 검사를 통과한 패치를 dominion-sub에 시험 반영하시겠습니까? 운영 서버는 자동 재시작되지 않습니다.'))return
  setApplying(true);setApplyError('')
  try{
   await post('/api/rsi/'+encodeURIComponent(id)+'/apply',{
    confirmation:'APPLY_UNVERIFIED_TO_SUB',baseCommit:candidate.value.baseCommit,
    patchSha256:candidate.value.patchSha256
   })
   setApplied(true);await candidate.reload();afterApply()
  }catch(e){setApplyError((e as Error).message)}finally{setApplying(false)}
 }
 return <div className="mt-3 rounded-lg border border-[#E7E0D5] bg-white p-3 text-[12px]">
  <strong>등록된 개선 후보 · {candidate.value.status}</strong>
  <p className="mt-2 break-all font-mono text-[10px] text-[#7C7469]">{id}</p>
  <p className="mt-1 text-[11px] text-[#7C7469]">대상 파일: {candidate.value.paths?.join(', ')||'불러오는 중'}</p>
  <details className="mt-3"><summary className="cursor-pointer font-semibold text-[#A94E30]">실제 수정 패치 확인</summary>
   <pre className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-[#F7F2EA] p-3 text-[10px]">{candidate.value.patch||'패치 정보 없음'}</pre>
  </details>
  <Notice message={applyError||compareError}/>
  {canApply&&['proposed','inconclusive'].includes(candidate.value.status)&&
   <Button variant="secondary" className="mt-3" disabled={comparing||comparison?.status==='running'||comparison?.status==='queued'} onClick={()=>void compare()}>
    <Terminal size={14}/> {comparing?'실험 등록 중…':'원본·수정본 8개 테스트 A/B 비교'}
   </Button>}
  {comparison&&<div className="mt-3 rounded-lg border border-[#E7E0D5] bg-[#FAF7F1] p-3">
   <strong>쌍대 실험: {comparison.status==='completed'?'종료':comparison.status==='failed'?'실패':comparison.status==='running'?'실행 중':comparison.status==='queued'?'대기 중':comparison.status}</strong>
   {comparison.error&&<Notice message={comparison.error}/>}
   {comparison.result?.metrics&&<p className="mt-2 text-[11px]">
    검사 {comparison.result.metrics.pairedCases}건 · 추가 성공 {comparison.result.metrics.gains}건 · 회귀 {comparison.result.metrics.regressions}건 · 보류 사례 {comparison.result.metrics.holdoutCases}건.
    판정: {comparison.result.verdict||'판정 대기'}.
   </p>}
   <p className="mt-1 text-[11px] text-[#8A8176]">기존 Rust 회귀 테스트의 원본/수정본 비교입니다. 실제 모델의 100턴 기억·토큰 비용 실험은 별도입니다.</p>
  </div>}
  {applied&&<p role="status" className="mt-3 text-[12px] text-emerald-700">작업 브랜치에 코드가 반영됐습니다. 서버 재빌드·재시작 전에는 실행 중인 엔진이 변경되지 않습니다.</p>}
  {canApply&&['proposed','inconclusive','verified'].includes(candidate.value.status)&&candidate.value.target!=='improver'&&
   <Button variant="secondary" disabled={applying} className="mt-3" onClick={()=>void apply()}>
    <ClipboardCheck size={14}/> {applying?'반영 중…':'컴파일 통과 패치 시험 반영 (수동 승인)'}
   </Button>}
  {candidate.value.status==='applied_to_sub'&&<p className="mt-2 font-semibold text-[#A94E30]">dominion-sub 시험 반영됨 · 독립 성능 개선 미검증</p>}
  <p className="mt-2 text-[11px] text-[#8A8176]">실제 성능 비교·최종 승격은 아직 수행되지 않았습니다. 코드는 자동 배포되지 않습니다.</p>
 </div>
}

export function SelfImprovementPage(){
 const providers=useRemote<Provider[]>('/api/providers',[])
 const dev=useRemote<DevStatus>('/api/dev/status',{
  config:{sourceDir:''},git:false,branch:'',dirty:0,exists:false,
 })
 const activeProviders=providers.value.filter(provider=>provider.enabled!==false)
 const [providerId,setProviderId]=useState('')
 const [model,setModel]=useState('')
 const [request,setRequest]=useState('')
 const [jobId,setJobId]=useState(()=>{
  try{return localStorage.getItem(SAVED_JOB)||''}catch{return ''}
 })
 const [job,setJob]=useState<Job|null>(null)
 const [approvals,setApprovals]=useState<Approval[]>([])
 const [answer,setAnswer]=useState('')
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 useEffect(()=>{
  if(!activeProviders.length)return
  if(!activeProviders.some(provider=>provider.id===providerId)){
   setProviderId(activeProviders[0].id)
   setModel(activeProviders[0].model||'')
  }
 },[providers.value,providerId])
 const sourceReady=dev.value.git&&dev.value.branch==='dominion-sub'&&dev.value.dirty===0
 useEffect(()=>{
  if(!jobId)return
  let alive=true
  const refresh=async()=>{
   try{
    const next=await get<Job>('/api/jobs/'+encodeURIComponent(jobId))
    if(!alive)return
    setJob(next)
    const session=next.sessionId||next.result?.sessionId
    if(session){
     const pending=await get<Approval[]>('/api/approvals')
     if(!alive)return
     setApprovals(pending.filter(a=>a.sessionId===session&&a.status==='pending'))
     if(terminal(next.status)){
      const messages=await get<Message[]>('/api/sessions/'+encodeURIComponent(session)+'/messages')
      if(!alive)return
      setAnswer([...messages].reverse().find(item=>item.role==='assistant'&&item.content.trim())?.content||'')
     }
    }
   }catch(e){if(alive)setError((e as Error).message)}
  }
  void refresh()
  const timer=window.setInterval(()=>void refresh(),1800)
  return()=>{alive=false;window.clearInterval(timer)}
 },[jobId])
 const start=async()=>{
  if(busy||!sourceReady||!providerId||!model.trim())return
  setBusy(true);setError('');setNotice('');setAnswer('');setApprovals([])
  try{
   const created=await post<Job>('/api/rsi/start',{providerId,model,request})
   setJob(created);setJobId(created.id)
   try{localStorage.setItem(SAVED_JOB,created.id)}catch{}
   setNotice(request.trim()?'지정한 분야를 중심으로 모델이 코드를 조사합니다.':'모델이 스스로 개선할 부분을 찾아 코드 진단을 시작했습니다.')
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const stop=async()=>{
  if(!jobId)return
  try{
   await post('/api/jobs/'+encodeURIComponent(jobId)+'/cancel',{})
   setNotice('중단 요청을 전달했습니다.')
  }catch(e){setError((e as Error).message)}
 }
 const approve=async(id:string,allow:boolean)=>{
  try{
   await resolveApproval(id,allow)
   setApprovals(previous=>previous.filter(item=>item.id!==id))
  }catch(e){setError((e as Error).message)}
 }
 const working=job&&!terminal(job.status)
 const statusText=job?.status==='completed'?'수정 작업 종료':job?.status==='running'?'모델이 코드 작업 중':job?.status==='queued'?'실행 대기':job?.status==='needs_review'?'추가 검토 필요':job?.status==='failed'?'실행 실패':job?.status==='cancelled'?'중단됨':''
 return <div className="mx-auto w-full max-w-[1080px] px-5 py-5 sm:px-7 lg:pb-10">
  <Headline eyebrow="DOMINION · RSI" title="재귀적 자기개선" description="모델만 고르면 DOMINION이 스스로 코드를 진단하고 더 나은 알고리즘과 구현을 찾아 개선 후보를 만듭니다." actions={<Button variant="secondary" onClick={()=>{void providers.reload();void dev.reload()}}><RefreshCw size={15}/> 새로고침</Button>}/>
  <Notice message={error||providers.error||dev.error}/>
  {notice&&<p role="status" className="mb-4 rounded-xl border border-[#E7E0D5] bg-white px-4 py-3 text-[12px] text-[#5C544A]">{notice}</p>}
  <Panel className="p-5">
   <div className="mb-5 flex items-center gap-3">
    <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#A94E30]"><BrainCircuit size={22}/></span>
    <div><strong className="text-[15px]">AI가 직접 진단하고 개선하기</strong>
     <p className="mt-1 text-[12px] text-[#7C7469]">모델이 전체 소스에서 개선할 부분을 직접 찾습니다. 선택적으로 중점 조사 분야를 지정할 수 있습니다.</p></div>
   </div>
   <div className="grid gap-3 md:grid-cols-2">
    <FormField label="제공자">
     <SelectInput value={providerId} onChange={e=>{const id=e.target.value;setProviderId(id);setModel(activeProviders.find(p=>p.id===id)?.model||'')}} disabled={busy||Boolean(working)}>
      {activeProviders.length===0&&<option value="">연결된 제공자 없음</option>}
      {activeProviders.map(provider=><option key={provider.id} value={provider.id}>{provider.name}</option>)}
     </SelectInput>
    </FormField>
    <FormField label="수정 작업을 수행할 모델">
     <ModelPicker providerId={providerId} value={model} onChange={setModel} disabled={busy||Boolean(working)} placeholder="모델 선택"/>
    </FormField>
   </div>
   <div className="mt-4"><FormField label="중점적으로 살펴볼 문제 (선택)">
    <TextArea rows={4} value={request} onChange={e=>setRequest(e.target.value)} disabled={busy||Boolean(working)} placeholder={defaultTask}/>
   </FormField></div>
   <div className="mt-4 flex flex-wrap items-center justify-between gap-3">
    <p className="text-[11px] text-[#7C7469]">소스: {dev.value.config.sourceDir||'미연결'} · 브랜치: {dev.value.branch||'미확인'}</p>
    <Button variant="primary" disabled={busy||Boolean(working)||!sourceReady||!providerId||!model.trim()} onClick={()=>void start()}>
     <Play size={16}/>{busy?'진단 시작 중…':'자율 진단·개선 시작'}
    </Button>
   </div>
   {!sourceReady&&<p role="status" className="mt-3 rounded-lg bg-[#FAF0E7] p-3 text-[12px] text-[#8F4E32]">작업 전 개발자 메뉴에서 DOMINION 소스 폴더를 연결하고 dominion-sub 브랜치와 미저장 변경 0개를 확인해야 합니다.</p>}
   <p className="mt-4 text-[11px] text-[#8A8176]">모델은 분리된 작업공간의 파일만 수정합니다. 검사는 엔진이 별도 수행하고 패치를 개선 후보로 저장합니다. 컴파일 성공과 실제 성능 향상은 서로 다른 판정입니다.</p>
  </Panel>
  {job&&<Panel className="mt-4 p-5">
   <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
    <div className="flex items-center gap-2"><Terminal size={17}/><strong className="text-[14px]">실행 현황</strong><StateTag online={job.status==='completed'}>{statusText||job.status}</StateTag></div>
    {working&&<Button variant="secondary" onClick={()=>void stop()}><Square size={13}/> 중단</Button>}
   </div>
   <p className="break-all text-[11px] text-[#8A8176]">작업 ID: {job.id}</p>
   {!!approvals.length&&<div className="mt-4 grid gap-2">
    {approvals.map(item=><div key={item.id} className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-[#E7D8C8] bg-[#FBF6EF] p-3">
     <div><p className="text-[12px] font-semibold"><ClipboardCheck size={14} className="mr-1 inline"/> 실행 승인 필요: {item.tool}</p>
      <p className="mt-1 max-w-[550px] break-all text-[11px] text-[#7C7469]">{operationLabel(item)}</p></div>
     <div className="flex gap-2"><Button variant="secondary" size="sm" onClick={()=>void approve(item.id,false)}>거절</Button><Button variant="primary" size="sm" onClick={()=>void approve(item.id,true)}>승인</Button></div>
    </div>)}
   </div>}
   {job.error&&<Notice message={job.error}/>}
   {job.result?.rsi&&<div className="mt-4 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-4 text-[12px]">
    <strong className="text-[13px]">엔진 직접 확인 결과</strong>
    <div className="mt-3 grid gap-2 sm:grid-cols-2">
     <p>원본 저장소: <strong>{job.result.rsi.sourceClean?'변경 없음':'변경 감지 · 점검 필요'}</strong></p>
     <p>수정 패치: <strong>{job.result.rsi.diffBytes?job.result.rsi.diffBytes+'바이트':'없음'}</strong></p>
     <p>컴파일·타입 검사: <strong>{job.result.rsi.checksPassed?'실행된 검사 통과':'불합격 또는 미실행'}</strong></p>
     <p>기준 대비 성능 비교: <strong>아직 수행하지 않음</strong></p>
     <p>검사 실패 후 재수정: <strong>{job.result.repairAttempts??0}회 (상한 2회)</strong></p>
     <p>코드 작업 판정: <strong>{job.result.reason==='candidate_compiles_not_performance_verified'?'컴파일 성공 · 성능 미검증':job.result.reason==='build_or_typecheck_failed'?'검사 실패 · 재검토 필요':job.result.reason||'검토 필요'}</strong></p>
    </div>
    {job.result.rsi.captureError&&<Notice message={job.result.rsi.captureError}/>}
    {job.result.rsi.modelError&&<Notice message={'모델 실행 오류: '+job.result.rsi.modelError}/>}
    <div className="mt-3 grid gap-2">{job.result.rsi.checks?.map((check,index)=><details key={index} className="rounded-lg border border-[#E7E0D5] bg-white p-3">
     <summary className="cursor-pointer font-semibold">{check.passed?'✓':'✕'} {check.label} — {check.ran?(check.passed?'통과':'실패'):'미실행'}</summary>
     <pre className="mt-2 max-h-44 overflow-auto whitespace-pre-wrap break-all text-[10px] text-[#7C7469]">{check.error||check.stderr||check.stdout||'상세 출력 없음'}</pre>
    </details>)}</div>
    {job.result.rsi.candidateId&&<CandidateReview id={job.result.rsi.candidateId} canApply={!!job.result.rsi.checksPassed&&!!job.result.rsi.sourceClean} afterApply={()=>void dev.reload()}/>}
   </div>}
   {answer&&<div className="mt-4 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-4">
    <div className="mb-2 flex items-center gap-2 text-[12px] font-bold"><CheckCircle2 size={15}/> 모델 설명 (엔진 검사와 구분)</div>
    <p className="whitespace-pre-wrap break-words text-[12px] leading-6 text-[#3A342D]">{answer}</p>
   </div>}
   {!answer&&!job.error&&<p className="mt-3 text-[12px] text-[#7C7469]">{working?'모델이 소스 조사·수정 작업을 진행하고 있습니다.':'완료된 답변이 있으면 이곳에 표시합니다.'}</p>}
   <p className="mt-3 text-[11px] text-[#8A8176]">작업이 종료됐어도 실제 수정과 테스트 성공 여부는 모델 보고와 실행 기록을 함께 확인해야 합니다. 자동 병합·배포는 하지 않습니다.</p>
  </Panel>}
 </div>
}
