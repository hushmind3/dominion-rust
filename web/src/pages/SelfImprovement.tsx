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
type Campaign={id:string;status:string;phase:string;controlRun?:boolean;controlOf?:string;controlParentCampaignId?:string;attempts?:number;acceptedGenerations?:number;tokens?:number;
 objective?:string;providerId?:string;model?:string;headCommit?:string;baseCommit?:string;sourceSnapshot?:boolean;
 continuedFrom?:string;selectedVersionRunning?:boolean;installJobId?:string;
 limits?:{maxAttempts?:number;tokenBudget?:number;timeBudgetSeconds?:number};
 application?:{commit?:string;requiresRebuild?:boolean;restartRequested?:boolean};
 tokensEstimated?:boolean;
 error?:string;stopReason?:string;selectionPolicy?:string;comparison?:Comparison;
 history?:{attempt:number;accepted:boolean;selection?:string;error?:string;domain:string;newFindings?:number;methodControlQueued?:{controlJobId?:string;status?:string};generatorCommit?:string;acceptedCommit?:string;methodExecutions?:{result?:{followed?:boolean;unknown?:boolean}}[];domainSelection?:string;searchStrategy?:{id?:string;name?:string;implementation?:string;mode?:string};trialOutcome?:{status?:string;verdict?:string};focusFiles?:string[];focusSelection?:FocusSelection}[]}
type VerifyRow={index:number;attempt?:number;accepted:boolean;domain?:string;generatorCommit?:string;acceptedCommit?:string;
 target?:string;independentReceipt:boolean;metaTransfer:boolean;regressions?:number;improvedHoldoutDomains?:string[];metaHoldoutGains?:number;reason?:string}
type VerifyReport={campaignId:string;capabilityDemonstrated:boolean;verifiedStreak:number;
 recursiveClosure?:{proven?:boolean;initialMethodControl?:{
  matchedControlFound?:boolean;measuredAdvantage?:boolean;
  treatedGenerationId?:string;controlCount?:number};
 revisedMethodControl?:{
  matchedControlFound?:boolean;measuredAdvantage?:boolean;
  treatedGenerationId?:string;controlCount?:number};
 methodControl?:{
  matchedControlFound?:boolean;measuredAdvantage?:boolean;
  treatedGenerationId?:string;methodOffGeneratorCommit?:string;
  matchedTaskSha256?:string;taskSha256?:string}};
 required:{verifiedStreak:number;holdoutDomains:number;metaTransfer:boolean};generations:VerifyRow[]}
const SAVED='dominion.rsi.evolutionJob'
const PREFERENCES='dominion.rsi.preferences'
const workloads={
 short:{label:'짧게',maxAttempts:4,tokenBudget:60000,timeBudgetSeconds:900},
 normal:{label:'보통',maxAttempts:8,tokenBudget:180000,timeBudgetSeconds:1800},
 long:{label:'넉넉히',maxAttempts:12,tokenBudget:600000,timeBudgetSeconds:3600},
} as const
type Workload=keyof typeof workloads
function preferences():{selection:'manual'|'automatic';workload:Workload;providerId:string;model:string}{
 try{const saved=JSON.parse(localStorage.getItem(PREFERENCES)||'null');return {
  selection:saved?.selection==='automatic'?'automatic':'manual',
  workload:saved?.workload in workloads?saved.workload:'short',providerId:saved?.providerId||'',model:saved?.model||''}}
 catch{return {selection:'manual',workload:'short',providerId:'',model:''}}
}
const stops:Record<string,string>={capability_demonstrated:'4세대 A→B→A2→B2와 두 번의 독립 개선법 대조에서 모두 성과를 확인했습니다.',
 attempt_limit:'선택한 작업량을 마쳤습니다. 같은 목표로 이어갈 수 있습니다.',
 token_budget:'설정한 사용량에 도달했습니다. 연구와 선택 버전은 보존했습니다.',
 time_budget:'설정한 작업 시간이 끝났습니다. 연구와 선택 버전은 보존했습니다.',
 no_progress:'새로운 근거나 변경이 없어 반복 호출을 멈췄습니다.'}
const terminal=(status:string)=>['completed','failed','cancelled','needs_review'].includes(status)
const phases:Record<string,string>={preparing_lab:'개선 준비 중',building_generator:'선택한 버전 준비 중',
 diagnosing_and_editing:'문제를 찾고 개선하는 중',measuring:'기존 버전과 개선 버전 비교 중',
 waiting_for_choice:'어느 버전을 사용할지 선택해 주세요',selecting:'선택한 버전으로 이어가는 중'}
const winners={candidate:'개선 버전이 우세합니다',baseline:'기존 버전이 우세합니다',inconclusive:'아직 우열을 확인하지 못했습니다'}
function pair(comparison:Comparison,key:string){
 const rows=comparison.metrics.measurements?.flatMap(row=>row.metrics).filter(row=>row.key===key)||[]
 if(!rows.length)return null
 const sum=(side:'baseline'|'candidate')=>rows.reduce((total,row)=>total+row[side],0)
 return {baseline:sum('baseline')/(key==='quality'?rows.length:1),candidate:sum('candidate')/(key==='quality'?rows.length:1)}
}
function Results({comparison,waiting,busy,onChoose}:{comparison:Comparison;waiting:boolean;busy:boolean;onChoose:(side:'baseline'|'candidate')=>void}){
 const accuracy=pair(comparison,'quality'),time=pair(comparison,'elapsedMs'),tokens=pair(comparison,'tokens')
 const rows=[{label:'기능 검사 정확도',values:accuracy,format:(v:number)=>(v*100).toFixed(1)+'%'},
  {label:'같은 검사에 걸린 시간',values:time,format:(v:number)=>v.toFixed(1)+' ms'},
  {label:'같은 문제의 사용량',values:tokens,format:(v:number)=>v.toLocaleString()+' 토큰'}]
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
 const [modelChanged,setModelChanged]=useState(false)
 const budget=workloads[workload]
 const [request,setRequest]=useState(''),[selection,setSelection]=useState<'manual'|'automatic'>(()=>preferences().selection)
 const [busy,setBusy]=useState(false),[error,setError]=useState('')
 const [settingsOpen,setSettingsOpen]=useState(()=>{try{return !localStorage.getItem(SAVED)}catch{return true}})
 const [jobId,setJobId]=useState(()=>{try{return localStorage.getItem(SAVED)||''}catch{return ''}})
 const [campaign,setCampaign]=useState<Campaign|null>(null)
 const [install,setInstall]=useState<InstallJob|null>(null)
 const [verify,setVerify]=useState<VerifyReport|null>(null)
 useEffect(()=>{try{localStorage.setItem(PREFERENCES,JSON.stringify({selection,workload,providerId,model}))}catch{}},[selection,workload,providerId,model])
 useEffect(()=>{
  if(enabled.length&&!enabled.some(provider=>provider.id===providerId)){
   setProviderId(enabled[0].id);setModel(enabled[0].model||'')
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
 useEffect(()=>{
  if(!modelChanged&&campaign?.providerId&&campaign.model&&enabled.some(p=>p.id===campaign.providerId)){
   setProviderId(campaign.providerId);setModel(campaign.model)
  }
 },[campaign?.id,campaign?.providerId,campaign?.model,providers.value,modelChanged])
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
 const hasSelected=!campaign?.controlRun&&!!campaign?.headCommit&&!!campaign?.baseCommit&&campaign.headCommit!==campaign.baseCommit
 const research=campaign?.history?.reduce((n,row)=>n+(row.newFindings||0),0)||0
 const inherited=campaign?.history?.filter((row,index,rows)=>rows.slice(0,index).some(prior=>prior.acceptedCommit&&prior.acceptedCommit===row.generatorCommit)).length||0
 const executedMethods=campaign?.history?.reduce((n,row)=>n+(row.methodExecutions?.length||0),0)||0
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
    selectionPolicy:selection,autoMethodControl:true,maxAttempts:budget.maxAttempts,tokenBudget:budget.tokenBudget,timeBudgetSeconds:budget.timeBudgetSeconds
   })
   setJobId(job.id);setCampaign({id:job.id,status:job.status,phase:'preparing_lab'});setInstall(null);setModelChanged(false);setSettingsOpen(false)
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
 const closure=verify?.campaignId===campaign?.id?verify.recursiveClosure:undefined
 const controlTarget=closure?.revisedMethodControl?.matchedControlFound!==true
  ?closure?.revisedMethodControl?.treatedGenerationId
  :closure?.initialMethodControl?.matchedControlFound!==true
    ?closure?.initialMethodControl?.treatedGenerationId
    :closure?.methodControl?.matchedControlFound!==true
      ?closure?.methodControl?.treatedGenerationId:undefined
 const runMethodControl=async()=>{
  if(!controlTarget||busy||working||installing)return
  setBusy(true);setError('')
  try{
   const job=await post<{id:string;status:string}>('/api/rsi/start',{
    mode:'continuous',confirmation:'START_RSI_EVOLUTION',
    controlForGenerationId:controlTarget
   })
   setJobId(job.id);setCampaign({id:job.id,status:job.status,phase:'preparing_lab',controlRun:true})
   setVerify(null);setInstall(null);setSettingsOpen(false)
   try{localStorage.setItem(SAVED,job.id)}catch{}
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const status=campaign?.controlRun?(working?'개선법 제거 대조군을 실제 실행하는 중입니다':campaign.status==='completed'?'대조군 실행 완료 · 원래 캠페인에서 검증 결과를 다시 확인하세요':'대조군 실행 점검이 필요합니다'):campaign?campaign.status==='completed'?(adopted>0?'검증된 개선본을 선택했습니다':research>0?'연구를 보존했습니다 · 채택한 코드 없음':'개선 근거가 없어 종료했습니다'):campaign.status==='cancelled'?'중단했습니다':
  campaign.status==='needs_review'||campaign.status==='failed'?'이번 작업을 계속하지 못했습니다':phases[campaign.phase]||'개선을 준비하고 있습니다':''
 return <div className="mx-auto w-full max-w-[920px] px-5 py-6 sm:px-7">
  <Headline eyebrow="DOMINION" title="계속 발전하기" description="기존 지식으로 연구하고 새 방법과 코드를 만들며, 검증된 버전이 다음 개선을 이어갑니다."
   actions={<Button variant="secondary" onClick={()=>{void providers.reload();void catalogue.reload()}} aria-label="새로고침"><RefreshCw size={15}/></Button>}/>
  <Notice message={error||providers.error||install?.error||campaign?.error||''}/>
  {campaign&&!working&&<div className="mb-3 flex justify-end"><Button variant="ghost" onClick={()=>setSettingsOpen(v=>!v)}>{settingsOpen?'설정 닫기':'모델 바꾸기'}</Button></div>}
  {(!campaign||settingsOpen)&&<Panel className="p-5 sm:p-7">
   <div className="mb-5 flex items-center gap-3"><BrainCircuit size={25} className="text-[#A94E30]"/><div><h2 className="font-bold">더 나은 버전이 다음 개선을 이어갑니다</h2><p className="mt-1 text-[12px] text-[#7C7469]">코딩 없이 모델과 선택 방식만 고르면 됩니다.</p></div></div>
   <div className="grid gap-3 sm:grid-cols-2"><FormField label="연결된 서비스"><SelectInput value={providerId} disabled={working||busy} onChange={e=>{setModelChanged(true);setProviderId(e.target.value);setModel(enabled.find(p=>p.id===e.target.value)?.model||'')}}>
    {!enabled.length&&<option value="">먼저 모델을 연결해 주세요</option>}{enabled.map(provider=><option key={provider.id} value={provider.id}>{provider.name}</option>)}
   </SelectInput></FormField><FormField label="개선을 맡길 모델"><ModelPicker providerId={providerId} value={model} onChange={value=>{setModelChanged(true);setModel(value)}} disabled={working||busy}/></FormField></div>
   <div className="mt-5"><p className="mb-2 text-[12px] font-semibold">비교 후 누가 선택할까요?</p><div className="grid grid-cols-2 gap-2">
    {([{id:'manual',label:'내가 선택',detail:'결과를 보고 버튼으로 고릅니다'},{id:'automatic',label:'우세한 쪽 자동 선택',detail:'확인된 개선만 이어서 사용합니다'}] as const).map(option=><Button variant="bare" type="button" key={option.id} aria-pressed={selection===option.id}
     disabled={working||busy} onClick={()=>setSelection(option.id)} className={'rounded-xl border p-3 text-left disabled:opacity-60 '+(selection===option.id?'border-[#C96442] bg-[#F9EDE6]':'border-[#E7E0D5] bg-white')}>
     <strong className="text-[13px]">{option.label}</strong><span className="mt-1 block text-[11px] text-[#7C7469]">{option.detail}</span></Button>)}
   </div></div>
   <div className="mt-5"><FormField label="중점적으로 나아졌으면 하는 점 (선택)"><TextArea rows={2} value={request} onChange={e=>setRequest(e.target.value)} disabled={working||busy} placeholder="예: 기억을 더 잘하고, 같은 일을 적은 비용으로 해 줘. 비워 두어도 됩니다."/></FormField></div>
   <div className="mt-5"><FormField label="한 번에 진행할 작업량"><SelectInput value={workload} disabled={working||busy||installing} onChange={e=>setWorkload(e.target.value as Workload)}>{Object.entries(workloads).map(([key,value])=><option key={key} value={key}>{value.label} · 최대 {value.maxAttempts}단계 · {(value.tokenBudget/10000).toLocaleString()}만 토큰 · {value.timeBudgetSeconds/60}분</option>)}</SelectInput></FormField></div>
   <div className="mt-4 flex flex-wrap items-center justify-between gap-3"><p className="text-[11px] leading-5 text-[#8A8176]">연구만 한 단계도 다음 구현으로 이어집니다. 중단해도 지식은 남습니다.</p><Button variant="primary" onClick={()=>void start()} disabled={busy||working||installing||!providerId||!model.trim()}><Play size={15}/>발전 시작</Button></div>
   <p className="mt-3 text-[11px] leading-5 text-[#8A8176]">시작하면 프로그램 복사본에서 수정과 실제 비교를 실행합니다. 개선법 A의 후손 B가 채택되면 A 이전 생성기로 동일 과제를 수행하는 자동 대조 작업이 별도로 예약되므로 추가 모델 사용량이 발생할 수 있습니다. 선택한 버전은 다음 개선에 사용됩니다. 완료 후 ‘검증본을 4317에 반영’을 누르면 작업 브랜치에 적용하고 서버를 다시 빌드합니다. 메인은 바뀌지 않습니다. 선택을 기다리는 시간은 제외하며, 사용량은 호출 전 추정치와 제공자 보고로 제한합니다.</p>
  </Panel>}
  {campaign&&<Panel className="mt-4 p-5"><div className="flex flex-wrap items-center justify-between gap-3"><div className="flex items-center gap-2">{working?<LoaderCircle size={16} className="animate-spin text-[#A94E30]"/>:<Check size={16}/>}<strong className="text-[14px]">{status}</strong></div>
   {working?<Button variant="secondary" disabled={busy} onClick={()=>void stop()}><Square size={13}/>중단</Button>:
    <div className="flex flex-wrap gap-2">{campaign.controlRun&&campaign.controlParentCampaignId?
     <Button variant="secondary" disabled={busy} onClick={()=>{
      setJobId(campaign.controlParentCampaignId!);setCampaign(null);setVerify(null);
      try{localStorage.setItem(SAVED,campaign.controlParentCampaignId!)}catch{}
     }}><ArrowLeft size={14}/>원래 RSI 캠페인으로 돌아가기</Button>:
     <><Button variant="ghost" onClick={backToSetup}><ArrowLeft size={14}/>돌아가기</Button><Button variant="primary" disabled={busy||installing||!providerId||!model.trim()} onClick={()=>void start(true)}><Play size={14}/>같은 목표로 이어가기</Button></>}</div>}</div>
   {campaign.objective&&<p className="mt-3 text-[13px] leading-6 text-[#5C544A]">목표: {campaign.objective}</p>}
   <p className="mt-3 text-[12px] text-[#7C7469]">개선 시도 {campaign.attempts||0}회 <ArrowRight size={12} className="mx-1 inline"/>채택 {campaign.acceptedGenerations||0}회 · 생성에 사용한 토큰 {campaign.tokensEstimated?'약 ':''}{(campaign.tokens||0).toLocaleString()}</p>
   <div className="mt-3 grid grid-cols-2 gap-2 text-[12px] sm:grid-cols-4">{[{label:'새 연구 근거',value:research},{label:'검증·선택한 코드',value:adopted},{label:'선택 버전으로 후속 실행',value:inherited},{label:'저장된 방법의 실제 실행',value:executedMethods}].map(item=><div key={item.label} className="rounded-lg bg-[#FAF7F1] p-3"><span className="block text-[#7C7469]">{item.label}</span><strong className="mt-1 block text-lg">{item.value}</strong></div>)}</div>
   <div className="mt-3 rounded-xl border border-[#E7E0D5] bg-white p-4">
    <div className="flex flex-wrap items-start justify-between gap-2"><div><p className="text-[13px] font-semibold">채택 검증 시험</p><p className="mt-1 text-[11px] text-[#7C7469]">저장된 세대별 독립 평가 영수증과 계보를 다시 대조합니다. 새 모델 호출은 없습니다.</p></div>
     <Button variant="secondary" disabled={busy} onClick={()=>void runVerify()}>{busy?<LoaderCircle size={13} className="animate-spin"/>:<ShieldCheck size={13}/>}검증 시험</Button></div>
    {verify&&verify.campaignId===campaign.id&&<div className="mt-3">
     <p className={verify.capabilityDemonstrated?'rounded-lg bg-[#F2F6EF] px-3 py-2 text-[12px] font-semibold text-[#4E6B45]':'rounded-lg bg-[#FAF7F1] px-3 py-2 text-[12px] font-semibold text-[#7C4A2E]'}>{verify.capabilityDemonstrated?'기준 충족 · 4세대 A→B→A2→B2 독립 평가와 2회 방법 제거 대조를 확인했습니다':`기준 미달 · 독립 검증 채택 ${verify.verifiedStreak}/4세대`}</p>
     {verify.recursiveClosure?.methodControl?.matchedControlFound===true&&
      <p className="mt-2 text-[12px] text-[#5C544A]">개선법 A 사용/미사용 대조: {verify.recursiveClosure.methodControl.measuredAdvantage?'동일 동결 과제에서 후손 개선 우위 측정':'대조군이 실측되었으나 후손의 우위 부족'} · 비무작위 비교</p>}
     {!working&&!campaign.controlRun&&!verify.capabilityDemonstrated&&controlTarget
       &&verify.recursiveClosure?.methodControl?.matchedControlFound!==true&&
       <div className="mt-3 rounded-lg border border-[#E7E0D5] p-3">
        <p className="text-[12px] font-semibold text-[#5C544A]">개선법 A를 제거한 실제 후손 대조 실험</p>
        <p className="mt-1 text-[11px] leading-5 text-[#7C7469]">A 수정 전 소스를 분리 실행하여 동일한 모델·동결 과제와 비교합니다. 채택·운영 반영은 하지 않으며 유료 모델 호출이 발생합니다.</p>
        <Button className="mt-2" variant="secondary" disabled={busy||installing} onClick={()=>void runMethodControl()}><Play size={13}/>대조군 1회 실행</Button>
       </div>}
     {verify.recursiveClosure?.initialMethodControl?.matchedControlFound===true&&
      <p className="mt-2 text-[12px] text-[#5C544A]">A→B 원본 대조: {verify.recursiveClosure.initialMethodControl.measuredAdvantage?'후손 능력 향상 측정':'동일 과제 우위 미달'} · 대조 {verify.recursiveClosure.initialMethodControl.controlCount||0}건</p>}
     {verify.recursiveClosure?.revisedMethodControl?.matchedControlFound===true&&
      <p className="mt-1 text-[12px] text-[#5C544A]">A2→B2 재귀 대조: {verify.recursiveClosure.revisedMethodControl.measuredAdvantage?'개선된 방법의 후속 능력 향상 측정':'동일 과제 우위 미달'} · 대조 {verify.recursiveClosure.revisedMethodControl.controlCount||0}건</p>}
     {verify.generations.length>0?<div className="mt-2 grid gap-1">{verify.generations.map(row=><div key={row.index} className="rounded-md bg-[#FAF7F1] px-2 py-1.5 text-[11px]">
      <div className="flex flex-wrap justify-between gap-2"><span className="font-semibold">{row.index}. {row.attempt?row.attempt+'번째 시도':'세대'}{row.domain?' · '+row.domain:''}</span><span className={row.independentReceipt?'text-[#4E6B45]':'text-[#7C7469]'}>{row.accepted?(row.independentReceipt?'독립 검증 일치':'영수증 불일치'):'채택 안 됨'}</span></div>
      <p className="mt-0.5 text-[#81786B]">{row.improvedHoldoutDomains?.length?'개선 확인 분야: '+row.improvedHoldoutDomains.join(', '):'개선 확인 분야 없음'}{row.regressions!=null?' · 회귀 '+row.regressions:''}{row.metaTransfer?' · 전이 확인':''}{row.reason?' · '+row.reason:''}</p>
     </div>)}</div>:<p className="mt-2 text-[11px] text-[#7C7469]">기록된 세대가 없습니다.</p>}
    </div>}
   </div>
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
    {item.methodControlQueued?.controlJobId&&<p className="mt-1 text-[#81786B]">개선법 제거 대조 자동 등록 · 작업 ID {item.methodControlQueued.controlJobId} · {item.methodControlQueued.status||'대기'}</p>}
    {item.generatorCommit&&<p className="mt-2 text-[#81786B]">실행기 {item.generatorCommit.slice(0,10)}{item.acceptedCommit?` · 채택 ${item.acceptedCommit.slice(0,10)}`:''}</p>}
   </div>)}</div></details>}
  </Panel>}
  {campaign?.comparison&&<Results comparison={campaign.comparison} waiting={working&&campaign.phase==='waiting_for_choice'} busy={busy} onChoose={side=>void choose(side)}/>}
 </div>
}
