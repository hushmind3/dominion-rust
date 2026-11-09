import {useEffect,useState} from 'react'
import {ArrowRight,BrainCircuit,Check,LoaderCircle,Play,RefreshCw,Square} from 'lucide-react'
import {Provider,get,post} from '../api'
import {ModelPicker} from '../components/ModelPicker'
import {Button,SelectInput,TextArea} from '../components/Primitives'
import {FormField,Headline,Notice,Panel,useRemote} from '../components/UI'

type Measurement={id:string;metrics:{key:string;baseline:number;candidate:number;improved:boolean;regressed:boolean}[]}
type Comparison={attempt:number;candidateId:string;winner:'baseline'|'candidate'|'inconclusive';
 canSelectCandidate:boolean;selected?:string;metrics:{pairedCases:number;regressions:number;measurements?:Measurement[]}}
type Campaign={id:string;status:string;phase:string;attempts?:number;acceptedGenerations?:number;tokens?:number;
 tokensEstimated?:boolean;
 error?:string;stopReason?:string;selectionPolicy?:string;comparison?:Comparison;
 history?:{attempt:number;accepted:boolean;selection?:string;error?:string;tokens?:number;domain:string}[]}
type Signal={id:string;kind:string;title:string;detail:string;evidence:unknown[];suggestedRequest:string}
type BacklogItem={id:string;kind:string;title:string;detail:string;status:'open'|'in_progress'|'done'|'dismissed';
 attempts?:number;tokensSpent?:number;lastError?:string;targetFiles?:string[];suggestedRequest?:string;
 evidence?:unknown[];lastCampaignId?:string;campaigns?:string[];lastSeenAt?:string}
type Check={id:string;title:string;inspected:number}
const itemLabels:Record<string,string>={open:'대기',in_progress:'수정 중',done:'완료',dismissed:'제외'}
const SAVED='dominion.rsi.evolutionJob'
const PREFERENCES='dominion.rsi.preferences'
function preferences():{selection:'manual'|'automatic';budget:number}{
 try{const saved=JSON.parse(localStorage.getItem(PREFERENCES)||'null');return {selection:saved?.selection==='automatic'?'automatic':'manual',budget:[0,1,2,3].includes(saved?.budget)?saved.budget:0}}
 catch{return {selection:'manual',budget:0}}
}
const terminal=(status:string)=>['completed','failed','cancelled','needs_review'].includes(status)
const phases:Record<string,string>={preparing_lab:'개선 준비 중',building_generator:'선택한 버전 준비 중',
 diagnosing_and_editing:'문제를 찾고 개선하는 중',measuring:'기존 버전과 개선 버전 비교 중',
 waiting_for_choice:'어느 버전을 사용할지 선택해 주세요',selecting:'선택한 버전으로 이어가는 중'}
const budgets=[{label:'한 번',seconds:600,tokens:30000,attempts:1},
 {label:'짧게',seconds:300,tokens:20000,attempts:2},
 {label:'보통',seconds:900,tokens:50000,attempts:3},{label:'길게',seconds:3600,tokens:150000,attempts:6}]
const stopLabels:Record<string,string>={
 no_progress:'3회 연속 실제 코드 변경이 없어 중단했습니다.',
 token_budget:'토큰 예산을 모두 사용해 중단했습니다.',
 time_budget:'시간 예산을 모두 사용해 중단했습니다.',
 attempt_limit:'시도 상한에 도달해 종료했습니다. 실제 코드 변경 여부는 아래 기록을 확인하세요.'}
const winners={candidate:'개선 버전이 우세합니다',baseline:'기존 버전이 우세합니다',inconclusive:'아직 우열을 확인하지 못했습니다'}
function pair(comparison:Comparison,key:string){
 const rows=comparison.metrics.measurements?.flatMap(row=>row.metrics).filter(row=>row.key===key)||[]
 if(!rows.length)return null
 const sum=(side:'baseline'|'candidate')=>rows.reduce((total,row)=>total+row[side],0)
 return {baseline:sum('baseline')/(key==='quality'?rows.length:1),candidate:sum('candidate')/(key==='quality'?rows.length:1)}
}
function Results({comparison,waiting,busy,onChoose}:{comparison:Comparison;waiting:boolean;busy:boolean;onChoose:(side:'baseline'|'candidate')=>void}){
 const accuracy=pair(comparison,'quality'),time=pair(comparison,'elapsedMs'),tokens=pair(comparison,'tokens')
 const rows=[{label:'검증 문제 정확도',values:accuracy,format:(v:number)=>(v*100).toFixed(1)+'%'},
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
 const signals=useRemote<{signals:Signal[];checks:Check[];items:BacklogItem[]}>('/api/rsi/signals',{signals:[],checks:[],items:[]})
 const enabled=providers.value.filter(provider=>provider.enabled!==false)
 const [providerId,setProviderId]=useState(''),[model,setModel]=useState('')
 const [request,setRequest]=useState(''),[selection,setSelection]=useState<'manual'|'automatic'>(()=>preferences().selection)
 const [budget,setBudget]=useState(()=>preferences().budget),[busy,setBusy]=useState(false),[error,setError]=useState('')
 const [settingsOpen,setSettingsOpen]=useState(false)
 const [jobId,setJobId]=useState(()=>{try{return localStorage.getItem(SAVED)||''}catch{return ''}})
 const [campaign,setCampaign]=useState<Campaign|null>(null)
 useEffect(()=>{try{localStorage.setItem(PREFERENCES,JSON.stringify({selection,budget}))}catch{}},[selection,budget])
 useEffect(()=>{
  if(enabled.length&&!enabled.some(provider=>provider.id===providerId)){
   setProviderId(enabled[0].id);setModel(enabled[0].model||'')
  }
 },[providers.value,providerId])
 useEffect(()=>{
  if(!jobId&&catalogue.value.campaigns.length){
   const latest=catalogue.value.campaigns.find(item=>!terminal(item.status))||catalogue.value.campaigns[0]
   setJobId(latest.id)
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
  if(campaign&&terminal(campaign.status))void signals.reload()
 },[campaign?.status])
 const working=!!campaign&&!terminal(campaign.status)
 const start=async(improvementItemId?:string,itemRequest?:string)=>{
  if(busy||working||!providerId||!model.trim())return
  setBusy(true);setError('')
  try{
   const cap=improvementItemId?budgets[0]:budgets[budget]
   const job=await post<{id:string;status:string}>('/api/rsi/start',{
    providerId,model,request:itemRequest??request,mode:'continuous',confirmation:'START_RSI_EVOLUTION',
    selectionPolicy:selection,maxAttempts:cap.attempts,timeBudgetSeconds:cap.seconds,tokenBudget:cap.tokens,
    ...(improvementItemId?{improvementItemId}: {})
   })
   setJobId(job.id);setCampaign({id:job.id,status:job.status,phase:'preparing_lab'});setSettingsOpen(false)
   try{localStorage.setItem(SAVED,job.id)}catch{}
   void signals.reload()
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const startItem=async(item:BacklogItem)=>{
  if(busy||working)return
  if(!providerId||!model.trim()){setError('개선을 실행할 모델을 먼저 선택하세요.');setSettingsOpen(true);return}
  setRequest(item.suggestedRequest||item.detail);setBudget(0)
  await start(item.id,item.suggestedRequest||item.detail)
 }
 const dismissItem=async(item:BacklogItem)=>{
  if(busy)return
  setBusy(true);setError('')
  try{await post('/api/rsi/backlog/'+encodeURIComponent(item.id)+'/dismiss',{});await signals.reload()}
  catch(e){setError((e as Error).message)}finally{setBusy(false)}
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
 const failureReason=campaign&&!(campaign.acceptedGenerations||0)
  ?campaign.error||(campaign.stopReason?stopLabels[campaign.stopReason]||campaign.stopReason:'')
   ||(campaign.history||[]).slice().reverse().find(item=>item.error)?.error||''
  :''
 const status=campaign?campaign.status==='completed'?(campaign.acceptedGenerations?'개선 버전을 채택했습니다':'후보를 만들지 못하고 끝났습니다'):campaign.status==='cancelled'?'중단했습니다':
  campaign.status==='needs_review'||campaign.status==='failed'?'이번 작업을 계속하지 못했습니다':phases[campaign.phase]||'개선을 준비하고 있습니다':''
 return <div className="mx-auto w-full max-w-[920px] px-5 py-6 sm:px-7">
  <Headline eyebrow="DOMINION" title="계속 발전하기" description="개선할 부분을 스스로 찾고, 기존 버전과 비교합니다. 어느 쪽이 더 나은지 보고 선택하세요."
   actions={<Button variant="secondary" onClick={()=>{void providers.reload();void catalogue.reload();void signals.reload()}} aria-label="새로고침"><RefreshCw size={15}/></Button>}/>
  <Notice message={error||providers.error||(campaign?.error?'이번 개선을 완료하지 못해 기존 버전을 유지했습니다. 다시 시작할 수 있습니다.':'')}/>
  <Panel className="mb-4 p-5">
   <div className="flex flex-wrap items-center justify-between gap-2"><div><h2 className="text-[14px] font-bold">자동 점검 · 개선 바구니</h2><p className="mt-1 text-[11px] text-[#7C7469]">모델을 부르기 전에 기록을 결정적으로 점검하고, 찾은 후보를 여기에 쌓아 둡니다.</p></div>
    <span className="rounded-full bg-[#F9EDE6] px-2 py-1 text-[11px] font-semibold text-[#A94E30]">점검 {(signals.value.checks||[]).length}개 · 후보 {(signals.value.items||[]).filter(item=>item.status==='open'||item.status==='in_progress').length}개</span></div>
   <div className="mt-3 flex flex-wrap gap-1.5">{(signals.value.checks||[]).map(check=><span key={check.id} className="rounded-full border border-[#E7E0D5] bg-[#FAF7F1] px-2 py-1 text-[10px] text-[#7C7469]">{check.title} {check.inspected}</span>)}</div>
   {!(signals.value.items||[]).length&&<p className="mt-3 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-[11px] leading-5 text-[#7C7469]">지금 기록에서는 자동으로 찾은 후보가 없습니다. 전체 점검은 계속 돌아가며, 새 실패나 토큰 낭비가 생기면 여기에 쌓입니다.</p>}
   <div className="mt-3 grid gap-2">{(signals.value.items||[]).map(item=><div key={item.id} className="rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3">
    <div className="flex flex-wrap items-start justify-between gap-2"><strong className="block text-[12px] text-[#3A342D]">{item.title}</strong><span className="rounded-full bg-white px-2 py-0.5 text-[10px] font-semibold text-[#7C7469]">{itemLabels[item.status]||item.status}</span></div>
    <p className="mt-1 text-[11px] leading-5 text-[#7C7469]">{item.detail}</p>
    <p className="mt-1 text-[10px] text-[#8A8176]">시도 {item.attempts||0}회 · 사용 토큰 {(item.tokensSpent||0).toLocaleString()}{item.targetFiles?.length?` · 대상 ${item.targetFiles.join(', ')}`:''}</p>
    {item.lastError&&<p className="mt-1 break-words text-[10px] text-[#A94E30]">지난 실패: {item.lastError}</p>}
    <details className="mt-2 text-[10px] text-[#7C7469]"><summary className="cursor-pointer">자동 점검 근거 보기</summary><pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap rounded-lg bg-white p-2">{JSON.stringify(item.evidence??[],null,2)}</pre></details>
    <div className="mt-2 flex flex-wrap gap-2">{item.status!=='done'&&item.status!=='dismissed'&&<Button size="sm" variant="secondary" disabled={busy||working} onClick={()=>void startItem(item)}>이 후보로 한 번에 수정</Button>}
     {item.status==='open'&&<Button size="sm" variant="ghost" disabled={busy||working} onClick={()=>void dismissItem(item)}>이 후보 제외</Button>}</div>
   </div>)}</div>
  </Panel>
  {campaign&&!working&&<div className="mb-3 flex justify-end"><Button variant="ghost" onClick={()=>setSettingsOpen(v=>!v)}>{settingsOpen?'설정 닫기':'모델·작업량 바꾸기'}</Button></div>}
  {(!campaign||settingsOpen)&&<Panel className="p-5 sm:p-7">
   <div className="mb-5 flex items-center gap-3"><BrainCircuit size={25} className="text-[#A94E30]"/><div><h2 className="font-bold">더 나은 버전이 다음 개선을 이어갑니다</h2><p className="mt-1 text-[12px] text-[#7C7469]">코딩 없이 모델과 선택 방식만 고르면 됩니다.</p></div></div>
   <div className="grid gap-3 sm:grid-cols-2"><FormField label="연결된 서비스"><SelectInput value={providerId} disabled={working||busy} onChange={e=>{setProviderId(e.target.value);setModel(enabled.find(p=>p.id===e.target.value)?.model||'')}}>
    {!enabled.length&&<option value="">먼저 모델을 연결해 주세요</option>}{enabled.map(provider=><option key={provider.id} value={provider.id}>{provider.name}</option>)}
   </SelectInput></FormField><FormField label="개선을 맡길 모델"><ModelPicker providerId={providerId} value={model} onChange={setModel} disabled={working||busy}/></FormField></div>
   <div className="mt-5"><p className="mb-2 text-[12px] font-semibold">비교 후 누가 선택할까요?</p><div className="grid grid-cols-2 gap-2">
    {([{id:'manual',label:'내가 선택',detail:'결과를 보고 버튼으로 고릅니다'},{id:'automatic',label:'우세한 쪽 자동 선택',detail:'확인된 개선만 이어서 사용합니다'}] as const).map(option=><button type="button" key={option.id} aria-pressed={selection===option.id}
     disabled={working||busy} onClick={()=>setSelection(option.id)} className={'rounded-xl border p-3 text-left disabled:opacity-60 '+(selection===option.id?'border-[#C96442] bg-[#F9EDE6]':'border-[#E7E0D5] bg-white')}>
     <strong className="text-[13px]">{option.label}</strong><span className="mt-1 block text-[11px] text-[#7C7469]">{option.detail}</span></button>)}
   </div></div>
   <div className="mt-5"><FormField label="중점적으로 나아졌으면 하는 점 (선택)"><TextArea rows={2} value={request} onChange={e=>setRequest(e.target.value)} disabled={working||busy} placeholder="예: 기억을 더 잘하고, 같은 일을 적은 비용으로 해 줘. 비워 두어도 됩니다."/></FormField></div>
   <div className="mt-4 flex flex-wrap items-end justify-between gap-4"><FormField label="이번 작업량"><SelectInput value={budget} disabled={working||busy} onChange={e=>setBudget(Number(e.target.value))}>
    {budgets.map((cap,index)=><option value={index} key={cap.label}>{cap.label} · 최대 {cap.seconds/60}분 / {cap.tokens.toLocaleString()}토큰</option>)}
   </SelectInput></FormField><Button variant="primary" onClick={()=>void start()} disabled={busy||working||!providerId||!model.trim()}><Play size={15}/>발전 시작</Button></div>
   <p className="mt-3 text-[11px] leading-5 text-[#8A8176]">시작하면 프로그램 복사본에서 수정과 실제 비교를 실행합니다. 선택한 버전은 다음 개선에 사용되며, 이전 선택은 다음 작업에도 이어집니다. 선택을 기다리는 시간은 작업 시간에 포함하지 않습니다.</p>
  </Panel>}
  {campaign&&<Panel className="mt-4 p-5"><div className="flex flex-wrap items-center justify-between gap-3"><div className="flex items-center gap-2">{working?<LoaderCircle size={16} className="animate-spin text-[#A94E30]"/>:<Check size={16}/>}<strong className="text-[14px]">{status}</strong></div>
   {working?<Button variant="secondary" disabled={busy} onClick={()=>void stop()}><Square size={13}/>중단</Button>:
    <Button variant="primary" disabled={busy||!providerId||!model.trim()} onClick={()=>void start()}><Play size={14}/>이어서 발전하기</Button>}</div>
   <p className="mt-3 text-[12px] text-[#7C7469]">개선 시도 {campaign.attempts||0}회 <ArrowRight size={12} className="mx-1 inline"/>채택 {campaign.acceptedGenerations||0}회 · 생성에 사용한 토큰 {campaign.tokensEstimated?'약 ':''}{(campaign.tokens||0).toLocaleString()}</p>
   {!working&&failureReason&&<p className="mt-2 break-words text-[12px] text-[#A94E30]">{failureReason}</p>}
   {campaign.history&&campaign.history.length>0&&<details className="mt-3 text-[12px]"><summary className="cursor-pointer text-[#7C7469]">지난 비교 결과</summary><div className="mt-2 grid gap-2">{campaign.history.map(item=><div key={item.attempt} className="rounded-lg bg-[#FAF7F1] p-2">{item.attempt}번째 · {item.accepted?'개선 버전 선택':item.error?'개선 효과 확인 못함':'기존 유지'} · {(item.tokens||0).toLocaleString()}토큰</div>)}</div></details>}
  </Panel>}
  {campaign?.comparison&&<Results comparison={campaign.comparison} waiting={working&&campaign.phase==='waiting_for_choice'} busy={busy} onChoose={side=>void choose(side)}/>}
 </div>
}
