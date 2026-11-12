import {useEffect,useState} from 'react'
import {ArrowLeft,ArrowRight,BrainCircuit,Check,LoaderCircle,Play,RefreshCw,ShieldCheck,Square,Download} from 'lucide-react'
import {Provider,get,post} from '../api'
import {ModelPicker} from '../components/ModelPicker'
import {Button,SelectInput,TextArea} from '../components/Primitives'
import {FormField,Headline,Notice,Panel,useRemote} from '../components/UI'

type Measurement={id:string;metrics:{key:string;baseline:number;candidate:number;improved:boolean;regressed:boolean}[]}
type Comparison={attempt:number;candidateId:string;winner:'baseline'|'candidate'|'inconclusive';
 canSelectCandidate:boolean;selected?:string;metrics:{pairedCases:number;regressions:number;measurements?:Measurement[]}}
type InstallJob={id:string;status:string;error?:string;result?:{installed?:string}}
type FocusCandidate={path:string;score?:number;sourceHash?:string;scoring?:string;policyError?:string;
 evidence?:{path?:number;symbols?:number;body?:number;domain?:number;priorProductive?:number}}
type FocusSelection={source?:string;domain?:string;policyRevision?:string;
 candidates?:FocusCandidate[]}
type Campaign={id:string;status:string;phase:string;attempts?:number;acceptedGenerations?:number;tokens?:number;
 systemEvolution?:boolean;systemSourceWrites?:number;evaluationEpisodes?:number;metaEvaluationProgress?:{arm?:string;task?:number};
 modelCalls?:number;verifiedSourceWrites?:number;candidateCount?:number;applicationError?:string;applyWhenFinished?:boolean;
 activeWorker?:{phase?:string;generatorCommit?:string;focusFiles?:string[]};diagnosis?:{failed?:number};
 objective?:string;providerId?:string;model?:string;headCommit?:string;baseCommit?:string;sourceSnapshot?:boolean;
 continuedFrom?:string;selectedVersionRunning?:boolean;installJobId?:string;
 limits?:{maxAttempts?:number;tokenBudget?:number;timeBudgetSeconds?:number};
 application?:{commit?:string;requiresRebuild?:boolean;restartRequested?:boolean};
 tokensEstimated?:boolean;
 error?:string;stopReason?:string;selectionPolicy?:string;comparison?:Comparison;
 history?:{attempt:number;accepted:boolean;selection?:string;error?:string;domain:string;newFindings?:number;generatorCommit?:string;acceptedCommit?:string;methodExecutions?:{result?:{followed?:boolean;unknown?:boolean}}[];domainSelection?:string;searchStrategy?:{id?:string;name?:string;implementation?:string;mode?:string};trialOutcome?:{status?:string;verdict?:string};focusFiles?:string[];focusSelection?:FocusSelection;metaEvaluation?:{tasks?:number;measurement?:{metrics?:{quality?:{baseline:number;candidate:number}}};episodes?:{arm:string;task:number;quality:number;usage?:{calls?:number;totalTokens?:number}}[]}}[]}
type VerifyRow={index:number;attempt?:number;accepted:boolean;domain?:string;generatorCommit?:string;acceptedCommit?:string;
 target?:string;independentReceipt:boolean;metaTransfer:boolean;regressions?:number;improvedHoldoutDomains?:string[];metaHoldoutGains?:number;reason?:string}
type VerifyReport={campaignId:string;capabilityDemonstrated:boolean;verifiedStreak:number;
 required:{verifiedStreak:number;holdoutDomains:number;metaTransfer:boolean};generations:VerifyRow[]}
const SAVED='dominion.rsi.evolutionJob'
const PREFERENCES='dominion.rsi.preferences'
const workloads={
 short:{label:'기본',maxAttempts:2,tokenBudget:600000,timeBudgetSeconds:3600},
 normal:{label:'확장',maxAttempts:4,tokenBudget:1200000,timeBudgetSeconds:7200},
 long:{label:'넉넉히',maxAttempts:12,tokenBudget:2000000,timeBudgetSeconds:7200},
} as const
type Workload=keyof typeof workloads
function preferences():{selection:'manual'|'automatic';workload:Workload;providerId:string;model:string}{
 try{const saved=JSON.parse(localStorage.getItem(PREFERENCES)||'null');return {
  selection:saved?.selection==='manual'?'manual':'automatic',
  workload:saved?.workload in workloads?saved.workload:'short',providerId:saved?.providerId||'',model:saved?.model||''}}
 catch{return {selection:'automatic',workload:'short',providerId:'',model:''}}
}
const stops:Record<string,string>={capability_demonstrated:'검증된 개선을 3세대 연속 채택하고 다른 분야로 이어가는 데 성공했습니다.',
 attempt_limit:'선택한 작업량을 마쳤습니다. 같은 목표로 이어갈 수 있습니다.',
 token_budget:'설정한 사용량에 도달했습니다. 연구와 선택 버전은 보존했습니다.',
 time_budget:'설정한 작업 시간이 끝났습니다. 연구와 선택 버전은 보존했습니다.',
 no_progress:'새로운 근거나 변경이 없어 반복 호출을 멈췄습니다.'}
const terminal=(status:string)=>['completed','failed','cancelled','needs_review'].includes(status)
const phases:Record<string,string>={preparing_lab:'개선 준비 중',building_generator:'선택한 버전 준비 중',
 diagnosing_and_editing:'문제를 찾고 개선하는 중',measuring:'기존 버전과 개선 버전 비교 중',
 waiting_for_choice:'어느 버전을 사용할지 선택해 주세요',selecting:'선택한 버전으로 이어가는 중',model_editing:'딥시크가 실제 소스를 수정하는 중',compiling_candidate:'수정한 코드 컴파일 검사 중',review:'수정 증거 수집 완료',paired_experiment:'독립 비교 실행 중',meta_evaluating:'이전 개선기와 수정된 개선기의 후속 코딩 성과 비교 중'}
const winners={candidate:'개선 버전이 우세합니다',baseline:'기존 버전이 우세합니다',inconclusive:'아직 우열을 확인하지 못했습니다'}
function Results({comparison,waiting,busy,onChoose}:{comparison:Comparison;waiting:boolean;busy:boolean;onChoose:(side:'baseline'|'candidate')=>void}){
 const rows=comparison.metrics.measurements?.flatMap(test=>test.metrics.map(metric=>({
  label:(test.id==='meta-downstream-editing'?'개선기의 실제 후속 코딩 성과':test.id)+' · '+({quality:'품질',elapsedMs:'실행 시간',tokens:'토큰',contextChars:'문맥 글자 수'}[metric.key]||metric.key),
  values:{baseline:metric.baseline,candidate:metric.candidate},
  format:(v:number)=>metric.key==='quality'?(v*100).toFixed(1)+'%':metric.key==='elapsedMs'?v.toFixed(2)+' ms':v.toLocaleString()
 })))||[]
 return <Panel className="mt-4 overflow-hidden p-5 sm:p-7">
  <p className="text-[11px] font-semibold uppercase tracking-widest text-[#8A8176]">{comparison.attempt}번째 비교</p>
  <h2 className="mt-2 text-xl font-bold text-[#3A342D]">{winners[comparison.winner]}</h2>
  <div className="mt-5 grid grid-cols-2 gap-3">
   {(['baseline','candidate'] as const).map(side=><div key={side} className={'rounded-xl border p-4 '+(comparison.winner===side?'border-[#B7C9B0] bg-[#F0F5ED]':'border-[#E7E0D5] bg-[#FAF7F1]')}>
    <div className="flex flex-wrap items-center justify-between gap-2"><strong>{side==='baseline'?'기존 버전':'개선 버전'}</strong>
     {comparison.winner===side&&<span className="rounded-full bg-[#DDEBD5] px-2 py-1 text-[11px] font-semibold text-[#3D6032]">우세</span>}</div>
    {rows.map(row=><div key={row.label} className="mt-4"><p className="text-[11px] text-[#7C7469]">{row.label}</p><p className="mt-1 text-lg font-semibold">{row.values?row.format(row.values[side]):'—'}</p></div>)}
    <Button variant={side==='candidate'&&comparison.canSelectCandidate?'primary':'secondary'} className="mt-5 w-full"
     disabled={!waiting||busy||(side==='candidate'&&!comparison.canSelectCandidate)} onClick={()=>onChoose(side)}>
     {busy?<LoaderCircle size={14} className="animate-spin"/>:<Check size={14}/>}
     {side==='baseline'?'기존 유지':'개선 버전 선택'}
    </Button>
   </div>)}
  </div>
  <p className="mt-4 text-[12px] leading-5 text-[#7C7469]">{comparison.selected?
   (comparison.selected==='candidate'?'개선 버전을 선택했습니다. 그 버전이 다음 개선을 이어갑니다.':'기존 버전을 유지하고 다음 개선안을 찾습니다.'):
   comparison.canSelectCandidate?'선택하면 해당 버전으로 다음 개선을 이어갑니다.':'우세함이 확인되지 않은 개선본은 채택하지 않습니다. 기존 버전을 유지하고 다음 후보를 찾을 수 있습니다.'}</p>
  <p className="mt-2 text-[11px] text-[#8A8176]">측정하지 않은 값은 —로 표시합니다. 빠르기만 하고 정확도나 기존 기능이 나빠진 버전은 우세로 판정하지 않습니다.</p>
 </Panel>
}

export function SelfImprovementPage(){
 const providers=useRemote<Provider[]>('/api/providers',[])
 const catalogue=useRemote<{campaigns:Campaign[]}>('/api/rsi/campaigns',{campaigns:[]})
 const algorithms=useRemote<{kinds:{kind:string;active?:{id:string;name:string;implementation:string;revision:string}}[]}>('/api/algorithms',{kinds:[]})
 const activeSearch=algorithms.value.kinds?.find(item=>item.kind==='selfimprove.search')?.active
 const enabledAll=providers.value.filter(provider=>provider.enabled!==false)
 const enabled=enabledAll
 const [providerId,setProviderId]=useState(()=>preferences().providerId),[model,setModel]=useState(()=>preferences().model)
 const [workload,setWorkload]=useState<Workload>(()=>preferences().workload)
 const budget=workloads[workload]
 const [request,setRequest]=useState(''),[selection,setSelection]=useState<'manual'|'automatic'>('automatic')
 const [busy,setBusy]=useState(false),[error,setError]=useState('')
 const [settingsOpen,setSettingsOpen]=useState(false)
 const [jobId,setJobId]=useState(()=>{try{return localStorage.getItem(SAVED)||''}catch{return ''}})
 const [campaign,setCampaign]=useState<Campaign|null>(null)
 const [install,setInstall]=useState<InstallJob|null>(null)
 const [verify,setVerify]=useState<VerifyReport|null>(null)
 useEffect(()=>{try{localStorage.setItem(PREFERENCES,JSON.stringify({selection,workload,providerId,model}))}catch{}},[selection,workload,providerId,model])
 useEffect(()=>{
  if(enabled.length&&!enabled.some(provider=>provider.id===providerId)){
   const preferred=enabled.find(p=>p.model==='deepseek-v4.1-flash')||enabled[0]
   setProviderId(preferred.id);setModel(preferred.model||'')
  }
 },[providers.value,providerId])
 useEffect(()=>{
  if(!jobId&&catalogue.value.campaigns.length){
   const active=catalogue.value.campaigns.find(item=>!terminal(item.status))
   setJobId((active||catalogue.value.campaigns[0]).id)
  }
 },[jobId,catalogue.value])
 useEffect(()=>{
  if(!jobId)return
  let alive=true
  const refresh=async()=>{try{
   const next=await get<Campaign>('/api/rsi/campaigns/'+encodeURIComponent(jobId))
   if(alive)setCampaign(next)
  }catch(e){if(alive)setError((e as Error).message)}}
  void refresh();const timer=window.setInterval(()=>void refresh(),1500)
  return()=>{alive=false;window.clearInterval(timer)}
 },[jobId])
 const installId=campaign?.installJobId||install?.id
 useEffect(()=>{
  if(!installId)return
  let alive=true
  const refresh=async()=>{try{const next=await get<InstallJob>('/api/dev/jobs/'+encodeURIComponent(installId));if(alive)setInstall(next)}catch{/* The service can restart after a completed installation. */}}
  void refresh();const timer=window.setInterval(()=>void refresh(),1500)
  return()=>{alive=false;window.clearInterval(timer)}
 },[installId])
 const installing=!!install&&!terminal(install.status)
 const working=!!campaign&&!terminal(campaign.status)
 const adopted=campaign?.acceptedGenerations||0
 const hasSelected=!!campaign?.headCommit&&!!campaign?.baseCommit&&campaign.headCommit!==campaign.baseCommit
 const research=campaign?.history?.reduce((n,row)=>n+(row.newFindings||0),0)||0
 const inherited=campaign?.history?.filter((row,index,rows)=>rows.slice(0,index).some(prior=>prior.acceptedCommit&&prior.acceptedCommit===row.generatorCommit)).length||0
 const backToSetup=()=>{
  setSettingsOpen(true);setError('')
  window.scrollTo({top:0,behavior:'smooth'})
 }
 const start=async(continuing=false)=>{
  if(busy||working||installing||!providerId||!model.trim())return
  setBusy(true);setError('')
  try{
   const job=await post<{id:string;status:string}>('/api/rsi/start',{
    providerId,model,...(continuing&&campaign?{continueFrom:campaign.id}:{request}),mode:'continuous',confirmation:'START_RSI_EVOLUTION',
    selectionPolicy:selection,evolutionTarget:'system',applyWhenFinished:true,expandAfterGain:true,nativeInduction:false,reasoningEffort:'low',maxAttempts:budget.maxAttempts,tokenBudget:budget.tokenBudget,timeBudgetSeconds:budget.timeBudgetSeconds
   })
   setJobId(job.id);setCampaign({id:job.id,status:job.status,phase:'preparing_lab'});setInstall(null);setSettingsOpen(false)
   try{localStorage.setItem(SAVED,job.id)}catch{}
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const choose=async(choice:'baseline'|'candidate')=>{
  if(!campaign?.comparison||busy)return
  setBusy(true);setError('')
  try{
   await post('/api/rsi/campaigns/'+encodeURIComponent(campaign.id)+'/choose',{choice,attempt:campaign.comparison.attempt})
   setCampaign({...campaign,phase:'selecting',comparison:{...campaign.comparison,selected:choice}})
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const stop=async()=>{
  if(!jobId||busy)return
  setBusy(true);setError('')
  try{await post('/api/jobs/'+encodeURIComponent(jobId)+'/cancel',{});setCampaign(old=>old?{...old,status:'cancelled'}:old)}
  catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const apply=async()=>{
  if(!campaign?.headCommit||busy||working||installing)return
  setBusy(true);setError('')
  try{
   const next=await post<InstallJob>('/api/rsi/campaigns/'+encodeURIComponent(campaign.id)+'/apply',{
    commit:campaign.headCommit,confirmation:'APPLY_VERIFIED_CAMPAIGN_TO_SUB'})
   setInstall(next);setCampaign({...campaign,installJobId:next.id})
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const runVerify=async()=>{
  if(!campaign||busy)return
  setBusy(true);setError('')
  try{setVerify(await get<VerifyReport>('/api/rsi/campaigns/'+encodeURIComponent(campaign.id)+'/verify'))}
  catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const status=campaign?campaign.status==='completed'?(adopted>0?'검증된 개선본을 선택했습니다':research>0?'연구를 보존했습니다 · 채택한 코드 없음':'개선 근거가 없어 종료했습니다'):campaign.status==='cancelled'?'중단했습니다':
  campaign.status==='needs_review'||campaign.status==='failed'?'이번 작업을 계속하지 못했습니다':phases[campaign.phase]||'개선을 준비하고 있습니다':''
 return <div className="mx-auto w-full max-w-[920px] px-5 py-6 sm:px-7">
  <Headline eyebrow="DOMINION" title="재귀적 자기개선" description="딥시크가 DOMINION의 알고리즘과 실행 하네스를 수정합니다. 이전 개선기와 수정된 개선기를 실제 후속 코딩 과제로 비교하고, 검증된 버전이 다음 개선을 이어갑니다."
   actions={<Button variant="secondary" onClick={()=>{void providers.reload();void catalogue.reload()}} aria-label="새로고침"><RefreshCw size={15}/></Button>}/>
  <Notice message={error||providers.error||install?.error||campaign?.applicationError||campaign?.error||''}/>
  <Panel className="p-5 sm:p-7">
   <div className="flex flex-wrap items-center justify-between gap-4"><div><h2 className="text-lg font-bold">버튼 한 번으로 시스템 자기개선</h2>
    <p className="mt-2 text-[13px] text-[#5C544A]">{enabled.find(p=>p.id===providerId)?.name||'서비스 확인 중'} · <strong>{model||'모델 확인 중'}</strong></p>
    <p className="mt-2 text-[12px] text-[#7C7469]">전체 코드 탐색 → 개선기 수정 → 후속 작업 A/B 평가 → {selection==='automatic'?'자동 채택':'직접 선택'} → 새 개선기로 재실행</p></div>
    <Button variant="primary" onClick={()=>void start()} disabled={busy||working||installing||!providerId||!model.trim()}>
     {busy||working||installing?<LoaderCircle size={16} className="animate-spin"/>:<Play size={16}/>}{installing?'프로그램에 반영 중':working?'자기개선 실행 중':'자기개선 시작'}</Button></div>
   <p className="mt-3 text-[12px] text-[#5C544A]">수정 범위: 알고리즘 · 자기개선기 · 도구/문맥 하네스 · 기억/학습 · 백엔드 · 프론트 전체. 외부 API 모델은 딥시크 플래시로 고정합니다.</p>
   <div className="mt-4 flex flex-wrap items-center justify-between gap-3"><p className="text-[11px] text-[#7C7469]">최대 {budget.maxAttempts}회 · {(budget.tokenBudget/10000).toLocaleString()}만 토큰 · {budget.timeBudgetSeconds/60}분. 개선을 확인하지 못하면 기존 버전을 유지합니다.</p>
    <Button variant="ghost" disabled={working||installing} onClick={()=>setSettingsOpen(v=>!v)}>{settingsOpen?'설정 닫기':'고급 설정'}</Button></div>
  </Panel>
  {settingsOpen&&<Panel className="mt-4 p-5 sm:p-7">
   <div className="mb-5 flex items-center gap-3"><BrainCircuit size={25} className="text-[#A94E30]"/><div><h2 className="font-bold">더 나은 버전이 다음 개선을 이어갑니다</h2><p className="mt-1 text-[12px] text-[#7C7469]">모델, 예산, 목표를 선택할 수 있습니다.</p></div></div>
   <div className="grid gap-3 sm:grid-cols-2"><FormField label="연결된 서비스"><SelectInput value={providerId} disabled={working||busy} onChange={e=>{setProviderId(e.target.value);setModel(enabled.find(p=>p.id===e.target.value)?.model||'')}}>
    {!enabled.length&&<option value="">먼저 모델을 연결해 주세요</option>}{enabled.map(provider=><option key={provider.id} value={provider.id}>{provider.name}</option>)}
   </SelectInput></FormField><FormField label="개선을 맡길 모델"><ModelPicker providerId={providerId} value={model} onChange={value=>{setModel(value)}} disabled={working||busy}/></FormField></div>
   <div className="mt-5"><p className="mb-2 text-[12px] font-semibold">비교 후 누가 선택할까요?</p><div className="grid grid-cols-2 gap-2">
    {([{id:'manual',label:'내가 선택',detail:'결과를 보고 버튼으로 고릅니다'},{id:'automatic',label:'우세한 쪽 자동 선택',detail:'확인된 개선만 이어서 사용합니다'}] as const).map(option=><Button variant="bare" type="button" key={option.id} aria-pressed={selection===option.id}
     disabled={working||busy} onClick={()=>setSelection(option.id)} className={'rounded-xl border p-3 text-left disabled:opacity-60 '+(selection===option.id?'border-[#C96442] bg-[#F9EDE6]':'border-[#E7E0D5] bg-white')}>
     <strong className="text-[13px]">{option.label}</strong><span className="mt-1 block text-[11px] text-[#7C7469]">{option.detail}</span></Button>)}
   </div></div>
   <div className="mt-5"><FormField label="중점적으로 나아졌으면 하는 점 (선택)"><TextArea rows={2} value={request} onChange={e=>setRequest(e.target.value)} disabled={working||busy} placeholder="예: 기억을 더 잘하고, 같은 일을 적은 비용으로 해 줘. 비워 두어도 됩니다."/></FormField></div>
   <div className="mt-5"><FormField label="한 번에 진행할 작업량"><SelectInput value={workload} disabled={working||busy||installing} onChange={e=>setWorkload(e.target.value as Workload)}>{Object.entries(workloads).map(([key,value])=><option key={key} value={key}>{value.label} · 최대 {value.maxAttempts}단계 · {(value.tokenBudget/10000).toLocaleString()}만 토큰 · {value.timeBudgetSeconds/60}분</option>)}</SelectInput></FormField></div>
   <p className="mt-4 text-[11px] leading-5 text-[#8A8176]">시작하면 선택한 API 모델로 실제 소스를 수정합니다. 검증된 변경은 현재 작업 브랜치와 4317 프로그램에 자동 반영하고 재시작합니다. 실행 중에는 중단할 수 있습니다.</p>
  </Panel>}
  {campaign&&<Panel className="mt-4 p-5"><div className="flex flex-wrap items-center justify-between gap-3"><div className="flex items-center gap-2">{working?<LoaderCircle size={16} className="animate-spin text-[#A94E30]"/>:<Check size={16}/>}<strong className="text-[14px]">{status}</strong></div>
   {working?<Button variant="secondary" disabled={busy} onClick={()=>void stop()}><Square size={13}/>중단</Button>:
    <div className="flex flex-wrap gap-2"><Button variant="ghost" onClick={backToSetup}><ArrowLeft size={14}/>돌아가기</Button><Button variant="primary" disabled={busy||installing||!providerId||!model.trim()} onClick={()=>void start(true)}><Play size={14}/>같은 목표로 이어가기</Button></div>}</div>
   {campaign.objective&&<p className="mt-3 text-[13px] leading-6 text-[#5C544A]">목표: {campaign.objective}</p>}
   <p className="mt-3 text-[12px] text-[#7C7469]">개선 시도 {campaign.attempts||0}회 <ArrowRight size={12} className="mx-1 inline"/>채택 {campaign.acceptedGenerations||0}회 · 생성에 사용한 토큰 {campaign.tokensEstimated?'약 ':''}{(campaign.tokens||0).toLocaleString()}</p>
   <div className="mt-3 grid grid-cols-2 gap-2 text-[12px] sm:grid-cols-4">{[{label:'실제 모델 호출',value:campaign.modelCalls||0},{label:'개선기 소스 수정',value:campaign.systemSourceWrites||0},{label:'개선기 코드 후보',value:campaign.candidateCount||0},{label:'독립 검증 채택',value:adopted}].map(item=><div key={item.label} className="rounded-lg bg-[#FAF7F1] p-3"><span className="block text-[#7C7469]">{item.label}</span><strong className="mt-1 block text-lg">{item.value}</strong></div>)}</div>
   <p className="mt-3 text-[12px] text-[#5C544A]">후속 코딩 평가 {campaign.evaluationEpisodes||0}회{campaign.metaEvaluationProgress?.arm?' · '+(campaign.metaEvaluationProgress.arm==='baseline'?'이전 개선기':'수정된 개선기')+' · 과제 '+((campaign.metaEvaluationProgress.task||0)+1):''}</p>
   <p className="mt-3 text-[11px] text-[#7C7469]">실행 모델: {campaign.model||model} · 채택 버전으로 후속 실행 {inherited}회{campaign.activeWorker?.generatorCommit?' · 현재 실행기 '+campaign.activeWorker.generatorCommit.slice(0,10):''}</p>
   {campaign.activeWorker?.focusFiles?.length?<p className="mt-1 break-all text-[11px] text-[#7C7469]">수정 대상: {campaign.activeWorker.focusFiles.join(', ')}</p>:null}
   <details className="mt-3 rounded-xl border border-[#E7E0D5] bg-white p-4">
    <summary className="cursor-pointer text-[12px] font-semibold">저장된 검증 기록</summary><div className="mt-3 flex flex-wrap items-start justify-between gap-2"><div><p className="text-[13px] font-semibold">독립 평가와 계보 기록</p><p className="mt-1 text-[11px] text-[#7C7469]">저장된 세대별 독립 평가 영수증과 계보를 다시 대조합니다. 새 모델 호출은 없습니다.</p></div>
     <Button variant="secondary" disabled={busy} onClick={()=>void runVerify()}>{busy?<LoaderCircle size={13} className="animate-spin"/>:<ShieldCheck size={13}/>}기록 조회</Button></div>
    {verify&&verify.campaignId===campaign.id&&<div className="mt-3">
     <p className={verify.capabilityDemonstrated?'rounded-lg bg-[#F2F6EF] px-3 py-2 text-[12px] font-semibold text-[#4E6B45]':'rounded-lg bg-[#FAF7F1] px-3 py-2 text-[12px] font-semibold text-[#7C4A2E]'}>{verify.capabilityDemonstrated?'기준 충족 · 검증된 3세대 연속 채택과 전이 근거를 확인했습니다':`기준 미달 · 독립 검증 채택 ${verify.verifiedStreak}/3세대`}</p>
     {verify.generations.length>0?<div className="mt-2 grid gap-1">{verify.generations.map(row=><div key={row.index} className="rounded-md bg-[#FAF7F1] px-2 py-1.5 text-[11px]">
      <div className="flex flex-wrap justify-between gap-2"><span className="font-semibold">{row.index}. {row.attempt?row.attempt+'번째 시도':'세대'}{row.domain?' · '+row.domain:''}</span><span className={row.independentReceipt?'text-[#4E6B45]':'text-[#7C7469]'}>{row.accepted?(row.independentReceipt?'독립 검증 일치':'영수증 불일치'):'채택 안 됨'}</span></div>
      <p className="mt-0.5 text-[#81786B]">{row.improvedHoldoutDomains?.length?'개선 확인 분야: '+row.improvedHoldoutDomains.join(', '):'개선 확인 분야 없음'}{row.regressions!=null?' · 회귀 '+row.regressions:''}{row.metaTransfer?' · 전이 확인':''}{row.reason?' · '+row.reason:''}</p>
     </div>)}</div>:<p className="mt-2 text-[11px] text-[#7C7469]">기록된 세대가 없습니다.</p>}
    </div>}
   </details>
   {!working&&campaign.stopReason&&<p className="mt-3 text-[12px] text-[#7C7469]">{stops[campaign.stopReason]||campaign.stopReason}</p>}
   {!working&&hasSelected&&<div className="mt-4 rounded-xl border border-[#D9E3D3] bg-[#F2F6EF] p-4"><p className="text-[13px] font-semibold">{campaign.selectedVersionRunning?'검증한 버전이 현재 4317에서 실행 중입니다':installing?'검증본을 빌드하고 있습니다':install?.status==='completed'?'빌드 완료 · 서버 재연결 확인 중':'선택한 개선본을 실제 사용하는 프로그램에 반영할 수 있습니다'}</p><p className="mt-1 text-[12px] text-[#7C7469]">{campaign.sourceSnapshot?'작업 시작 시 미저장 코드가 있었습니다. 파일을 보존하기 위해 자동 반영은 제한합니다.':'작업 브랜치만 반영하며 메인과 다른 서비스의 설정은 유지합니다.'}</p>{!campaign.selectedVersionRunning&&!campaign.sourceSnapshot&&<Button className="mt-3" variant="primary" disabled={busy||installing} onClick={()=>void apply()}>{installing?<LoaderCircle size={14} className="animate-spin"/>:<Download size={14}/>}검증본을 4317에 반영</Button>}</div>}
   {activeSearch&&<div className="mt-2 flex flex-wrap items-center gap-2 rounded-lg bg-[#FAF7F1] px-3 py-2 text-[11px] text-[#7C7469]"><span className="font-semibold text-[#5C544A]">다음 연구를 고르는 방식</span><span>{activeSearch.name}</span></div>}
   {campaign.history&&campaign.history.length>0&&<details className="mt-3 text-[12px]"><summary className="cursor-pointer text-[#7C7469]">세대별 실행·탐색 근거</summary><div className="mt-2 grid gap-2">{campaign.history.map(item=><div key={item.attempt} className="rounded-lg bg-[#FAF7F1] p-3">
    <strong>{item.attempt}번째 · {item.accepted?'검증 버전 선택':(item.newFindings||0)>0?'연구 보존':item.error?'개선 효과 확인 못함':'새로운 진전 없음'}</strong>
    <p className="mt-1 text-[#7C7469]">탐색 분야: {item.domain||'미측정'}{item.domainSelection==='algorithm_trial'?' · 신규 탐색식 시험':item.domainSelection==='research_continuation'?' · 이전 연구 계승':''}{item.trialOutcome?.verdict?` · 평가: ${item.trialOutcome.verdict}`:''}</p>
    {item.focusSelection?.candidates?.length?<div className="mt-2 space-y-1">
     <p className="font-semibold text-[#665D50]">선택한 코드와 판단 근거 · {item.focusSelection.source==='indexed_evidence'?'인덱스 기반':'사용자 지정'}{item.focusSelection.policyRevision?` · 정책 ${item.focusSelection.policyRevision.slice(0,10)}`:''}</p>
     {item.focusSelection.candidates.map(file=><div key={file.path} className="rounded-md border border-[#E7E0D5] bg-white px-2 py-1.5">
      <div className="flex flex-wrap justify-between gap-2"><code className="break-all">{file.path}</code><span>{Number.isFinite(file.score)?`점수 ${file.score?.toFixed(2)}`:'점수 미측정'}</span></div>
      <p className="mt-0.5 text-[#81786B]">{file.scoring==='expression'?'모델 생성 탐색식':'기본 근거 탐색'}{file.policyError?' · 탐색식 오류로 기본 방식 사용':''}{file.sourceHash?` · 원본 ${file.sourceHash.slice(0,10)}`:''}</p>
     </div>)}
    </div>:item.focusFiles?.length?<p className="mt-2 text-[#81786B]">대상 파일: {item.focusFiles.join(', ')}</p>:null}
    {item.metaEvaluation?.episodes?.length?<div className="mt-3 rounded-lg border border-[#D9E3D3] bg-white p-3"><p className="font-semibold">개선기 자체의 후속 코딩 평가 · {item.metaEvaluation.tasks}개 과제</p>
     <div className="mt-2 grid grid-cols-2 gap-2">{(['baseline','candidate'] as const).map(arm=><div key={arm}><strong>{arm==='baseline'?'이전 개선기':'수정된 개선기'}</strong>
      {item.metaEvaluation?.episodes?.filter(row=>row.arm===arm).map(row=><p key={row.task} className="mt-1 text-[11px] text-[#7C7469]">과제 {row.task+1}: {(row.quality*100).toFixed(1)}% · {row.usage?.calls||0}회 호출 · {(row.usage?.totalTokens||0).toLocaleString()}토큰</p>)}</div>)}</div>
     <p className="mt-2 text-[11px] text-[#7C7469]">같은 모델·원본 소스·과제별 예산으로 실제 패치를 생성하고 동결된 검사로 평가했습니다. 두 과제의 결과이며, 모든 분야의 향상을 증명하지는 않습니다.</p></div>:null}
    {item.generatorCommit&&<p className="mt-2 text-[#81786B]">실행기 {item.generatorCommit.slice(0,10)}{item.acceptedCommit?` · 채택 ${item.acceptedCommit.slice(0,10)}`:''}</p>}
   </div>)}</div></details>}
  </Panel>}
  {campaign?.comparison&&<Results comparison={campaign.comparison} waiting={working&&campaign.phase==='waiting_for_choice'} busy={busy} onChoose={side=>void choose(side)}/>}
 </div>
}
