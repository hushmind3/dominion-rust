import {useEffect,useState} from 'react'
import {ArrowUpRight,CheckCircle2,Database,LoaderCircle,Search,Wallet,RefreshCw,Square} from 'lucide-react'
import {get,post,type Agent} from '../api'

type Evidence={
 sourceId?:string;url?:string;claim?:string;publishedAt?:string;
 sourceName?:string;kind?:string;contentStatus?:string;
 quote?:string;quoteStatus?:string;revisionId?:string;storyKey?:string
}
type ArchivedSource={id:string;title:string;url:string;sourceName?:string;
 publishedAt?:string;verification?:string;description?:string;changeTags?:string[];
 feedOriginal?:string;feedOriginalTruncated?:boolean;revisionId?:string;storyKey?:string}
type Lead={
 id:string;title:string;offer?:string;customer?:string;
 whyPay?:string;implementationPlan?:string;firstPaymentStep?:string;
 counterfactual?:string;firstTest?:string;evidenceStatus?:string;
 causalChain?:string;existingAlternatives?:string;firstBuyerAccess?:string;
 evidence?:Evidence[];origin?:string;status?:string;implementationRunId?:string
 citationStatus?:string;unmatchedSourceIds?:string[];sourceQuoteChecks?:Array<{
   sourceId:string;quote:string;status:string;revisionId?:string
 }>;
 decisionHistory?:Array<{action:string;reason?:string;from:string;to:string;at:string}>
}
type Report={
 drafts:Lead[];candidateIds?:string[];sourceCount?:number;modelCalls?:number;
 searchedAt?:string;reasoningRunId?:string|null;reasoningStatus?:string;
 inferenceError?:string;inferenceCostKrw?:number;
 inferenceInputTokens?:number;inferenceOutputTokens?:number;message?:string
}
type Stats={
 signals:number;candidates:number;metrics?:number;lastCatalog?:{newMetrics?:number;capturedAt?:string}|null;lastIngest?:{
 found?:number;new?:number;capturedAt?:string;sources?:Array<{name:string;found?:number;error?:string}>
 }|null;
 automation?:{enabled?:boolean;periodMinutes?:number}
}
type CandidatePage={items:Lead[];hasMore:boolean;nextOffset:number}
type PriceResult={id:string;status:string;spreadKrw:number|null;
 landedCostKrw?:number|null;buyerTotalKrw?:number|null;issues:string[];
 tradeVerified:false;sourceChecks?:Record<string,{status:string;revisionId?:string;
 currentRevisionId?:string;revisionStatus?:string;revisionChangedSinceQuote?:boolean}>;
 createdAt?:string}
type PriceFields={
 buyerAmount:string;supplierAmount:string;buyerCurrency:string;supplierCurrency:string;
 buyerFx:string;supplierFx:string;quantity:string;spec:string;unit:string;asOf:string;
 buyerKind:string;supplierKind:string;shippingKrw:string;feesKrw:string;
 taxKrw:string;laborKrw:string;returnsKrw:string;otherKrw:string;
 buyerSourceId:string;supplierSourceId:string;buyerQuote:string;supplierQuote:string;
 buyerRevisionId:string;supplierRevisionId:string
}
const initialPrice:PriceFields={buyerAmount:'',supplierAmount:'',buyerCurrency:'KRW',
 supplierCurrency:'KRW',buyerFx:'',supplierFx:'',quantity:'1',spec:'',unit:'unit',
 asOf:'',buyerKind:'',supplierKind:'',shippingKrw:'',feesKrw:'',
 taxKrw:'',laborKrw:'',returnsKrw:'',otherKrw:'',buyerSourceId:'',
 supplierSourceId:'',buyerQuote:'',supplierQuote:'',buyerRevisionId:'',supplierRevisionId:''}
const initial:Report={drafts:[],modelCalls:0}
const won=(n:number)=>Math.max(0,n).toLocaleString('ko-KR')+'원'

export function MoneyPage(){
 const [report,setReport]=useState<Report>(initial)
 const [stats,setStats]=useState<Stats|null>(null)
 const [agents,setAgents]=useState<Agent[]>([])
 const [agentId,setAgentId]=useState('general')
 const [budgetKrw,setBudgetKrw]=useState(100)
 const [implementationId,setImplementationId]=useState('')
 const [implementationRuns,setImplementationRuns]=useState<Record<string,{status:string;steps?:number;cost_krw?:number}>>({})
 const [busy,setBusy]=useState<'find'|'infer'|'scan'|'automation'|'cancel'|'implement'|''>('')
 const [loading,setLoading]=useState(true)
 const [error,setError]=useState('')
 const [moduleEnabled,setModuleEnabled]=useState(false)
 const [moduleLoading,setModuleLoading]=useState(true)
 const [showAllCandidates,setShowAllCandidates]=useState(false)
 const [candidatesFilter,setCandidatesFilter]=useState<'all'|'draft'|'held'|'rejected'|'implementation_requested'>('all')
 const [candidateQuery,setCandidateQuery]=useState('')
 const [candidateReason,setCandidateReason]=useState('')
 const [candidatePage,setCandidatePage]=useState<Lead[]>([])
 const [priceDraft,setPriceDraft]=useState<PriceFields>(initialPrice)
 const [priceReviews,setPriceReviews]=useState<Record<string,PriceResult[]>>({})
 const [priceBusy,setPriceBusy]=useState(false)
 const [candidatesMore,setCandidatesMore]=useState(false)
 const [candidatesOffset,setCandidatesOffset]=useState(0)
 const [loadingCandidates,setLoadingCandidates]=useState(false)
 const [archiveQuery,setArchiveQuery]=useState('')
 const [archiveResults,setArchiveResults]=useState<ArchivedSource[]>([])
 const [sourceHistory,setSourceHistory]=useState<Record<string,{
    revisions:ArchivedSource[];relatedSameTitle:ArchivedSource[];
    legacyRevisionUnavailable?:boolean;relationStatus:string
 }>>({})
 const [archiveSearched,setArchiveSearched]=useState(false)
 const [searchingArchive,setSearchingArchive]=useState(false)
 const inferring=report.reasoningStatus==='running'||report.reasoningStatus==='queued'
 const runIds=(report.drafts||[]).map(lead=>lead.implementationRunId).filter(Boolean).join('|')
 const refresh=async()=>{
  const [latest,summary,config]=await Promise.all([
   get<Report>('/api/money/find'),get<Stats>('/api/money/stats'),
   get<{enabled:boolean}>('/api/money/settings')
  ])
  setReport(latest);setStats(summary);setModuleEnabled(config.enabled)
 }
 useEffect(()=>{
  let live=true
  void Promise.all([get<Report>('/api/money/find'),
   get<Stats>('/api/money/stats'),get<Agent[]>('/api/agents'),
   get<{enabled:boolean}>('/api/money/settings')])
   .then(([r,s,a,c])=>{if(live){setReport(r);setStats(s);setAgents(a);setModuleEnabled(c.enabled)}})
   .catch(e=>{if(live)setError((e as Error).message)})
   .finally(()=>{if(live){setLoading(false);setModuleLoading(false)}})
  return()=>{live=false}
 },[])
 useEffect(()=>{
  if(!inferring)return
  let active=true
  const timer=window.setInterval(()=>{
   void get<Report>('/api/money/find').then(value=>{
    if(active)setReport(value)
   }).catch(e=>{if(active)setError((e as Error).message)})
  },2500)
  return()=>{active=false;window.clearInterval(timer)}
 },[inferring])
 useEffect(()=>{
  const ids=runIds.split('|').filter(Boolean)
  if(!ids.length)return
  let live=true
  let timer:ReturnType<typeof setInterval>|undefined
  const check=async()=>{
   const results=await Promise.all(ids.map(async id=>{
    try{return {id,run:await get<{status:string;steps:number;cost_krw:number}>('/api/v1/runs/'+encodeURIComponent(id))}}
    catch{return {id,run:null}}
   }))
   if(!live)return
   const next:Record<string,{status:string;steps?:number;cost_krw?:number}>={}
   for(const {id,run} of results)next[id]=run||{status:'unknown'}
   setImplementationRuns(next)
   if(results.every(({run})=>run&&['completed','cancelled','failed'].includes(run.status))&&timer)
    clearInterval(timer)
  }
  timer=setInterval(()=>{void check()},3000)
  void check()
  return()=>{live=false;if(timer)clearInterval(timer)}
 },[runIds])
 async function configureMoney(enabled:boolean){
  if(busy||moduleLoading)return
  setError('');setBusy('automation');setModuleLoading(true)
  try{
   const config=await post<{enabled:boolean}>('/api/money/settings',{enabled})
   setModuleEnabled(config.enabled)
   await refresh()
  }catch(e){setError((e as Error).message)}
  finally{setBusy('');setModuleLoading(false)}
 }
 async function showSourceHistory(id:string){
  setError('')
  try{
   const record=await get<{revisions:ArchivedSource[];relatedSameTitle:ArchivedSource[];
       legacyRevisionUnavailable?:boolean;relationStatus:string}>(
     '/api/money/sources/'+encodeURIComponent(id)+'/history')
   setSourceHistory(prev=>({...prev,[id]:record}))
  }catch(e){setError((e as Error).message)}
 }
 async function searchArchive(){
  const term=archiveQuery.trim()
  if(term.length<2||term.length>160)return
  setError('');setSearchingArchive(true)
  try{
   const result=await get<{results:ArchivedSource[]}>(
    '/api/money/search?q='+encodeURIComponent(term))
   setArchiveResults(result.results);setArchiveSearched(true)
  }catch(e){setError((e as Error).message)}
  finally{setSearchingArchive(false)}
 }
 async function implement(id:string){
  if(busy)return
  setError('');setBusy('implement');setImplementationId(id)
  try{
   // This button is an explicit approval to start a native agent Run,
   // not permission for sales, deployments or external payments.
   await post('/api/money/candidates/'+encodeURIComponent(id)+'/implement',{
    approved:true,agentId,budgetKrw
   })
   await refresh()
  }catch(e){setError((e as Error).message)}
  finally{setBusy('');setImplementationId('')}
 }
 async function resumeImplementation(candidateId:string,runId:string){
  if(busy)return
  setError('');setBusy('implement')
  try{
   // First reconcile any domain-reserved-but-not-started Run using its
   // persisted candidate receipt. Do not bypass that link via native resume.
   const result=await post<{status:string;id:string}>(
    '/api/money/candidates/'+encodeURIComponent(candidateId)+'/implement',
    {approved:true,agentId,budgetKrw})
   // Ordinary paused runs with an already committed receipt still use the
   // shared native Run resume and unresolved-tool reconciliation policy.
   if(result.status==='paused'){
    await post('/api/v1/runs/'+encodeURIComponent(runId)+'/resume',{})
   }
   await refresh()
  }catch(e){setError((e as Error).message)}
  finally{setBusy('')}
 }
 async function cancelImplementation(id:string){
  if(busy)return
  setError('');setBusy('cancel')
  try{
   await post('/api/v1/runs/'+encodeURIComponent(id)+'/cancel',{})
   await refresh()
  }catch(e){setError((e as Error).message)}
  finally{setBusy('')}
 }
 async function action(kind:'find'|'infer'|'scan'|'automation'|'cancel',payload?:unknown){
  if(busy)return
  setBusy(kind);setError('')
  try{
   if(kind==='find')setReport(await post<Report>('/api/money/find',{mode:'db'}))
   if(kind==='infer')setReport(await post<Report>('/api/money/find',{
    mode:'agent',agentId,budgetKrw
   }))
   if(kind==='scan')await post('/api/money/collect',{force:true})
   if(kind==='automation')await post('/api/money/automation',payload)
   if(kind==='cancel')await post('/api/money/inference/cancel',{})
   await refresh()
  }catch(e){setError((e as Error).message)}
  finally{setBusy('')}
 }
 async function loadCandidates(offset=0,append=false){
  if(!moduleEnabled)return
  setLoadingCandidates(true)
  try{
   const params=new URLSearchParams({
    status:candidatesFilter,q:candidateQuery.trim(),offset:String(offset)
   })
   const page=await get<CandidatePage>('/api/money/candidates?'+params.toString())
   setCandidatePage(prev=>append?[...prev,...page.items]:page.items)
   setCandidatesOffset(page.nextOffset);setCandidatesMore(page.hasMore)
  }catch(e){setError((e as Error).message)}
  finally{setLoadingCandidates(false)}
 }
 useEffect(()=>{
  if(!showAllCandidates||!moduleEnabled)return
  let live=true
  const params=new URLSearchParams({status:candidatesFilter,q:candidateQuery.trim(),offset:'0'})
  void get<CandidatePage>('/api/money/candidates?'+params.toString())
   .then(page=>{if(live){setCandidatePage(page.items);setCandidatesOffset(page.nextOffset);setCandidatesMore(page.hasMore)}})
   .catch(e=>{if(live)setError((e as Error).message)})
  return()=>{live=false}
 },[showAllCandidates,moduleEnabled,candidatesFilter])
 function priceField(key:keyof PriceFields,value:string){
  setPriceDraft(prev=>({...prev,[key]:value}))
 }
 function numericPrice(key:keyof PriceFields):number|null{
  const text=priceDraft[key].trim()
  if(!text)return null
  const parsed=Number(text)
  return Number.isFinite(parsed)?parsed:null
 }
 async function loadPriceHistory(id:string){
  setPriceBusy(true);setError('')
  try{
   const history=await get<PriceResult[]>('/api/money/candidates/'+encodeURIComponent(id)+'/prices')
   setPriceReviews(prev=>({...prev,[id]:history}))
  }catch(e){setError((e as Error).message)}
  finally{setPriceBusy(false)}
 }
 async function comparePrice(id:string){
  if(priceBusy||busy)return
  setPriceBusy(true);setError('')
  const ref={quantity:numericPrice('quantity'),spec:priceDraft.spec.trim(),
   unit:priceDraft.unit.trim(),asOf:priceDraft.asOf.trim()}
  const costs={shippingKrw:numericPrice('shippingKrw'),feesKrw:numericPrice('feesKrw'),
   taxKrw:numericPrice('taxKrw'),laborKrw:numericPrice('laborKrw'),
   returnsKrw:numericPrice('returnsKrw'),otherKrw:numericPrice('otherKrw')}
  const buyer={...ref,amount:numericPrice('buyerAmount'),currency:priceDraft.buyerCurrency,
   krwPerUnit:numericPrice('buyerFx'),quoteKind:priceDraft.buyerKind,
   sourceId:priceDraft.buyerSourceId,quote:priceDraft.buyerQuote.trim(),
   revisionId:priceDraft.buyerRevisionId.trim()}
  const supplier={...ref,amount:numericPrice('supplierAmount'),currency:priceDraft.supplierCurrency,
   krwPerUnit:numericPrice('supplierFx'),quoteKind:priceDraft.supplierKind,
   sourceId:priceDraft.supplierSourceId,quote:priceDraft.supplierQuote.trim(),
   revisionId:priceDraft.supplierRevisionId.trim()}
  try{
   const result=await post<PriceResult>(
    '/api/money/candidates/'+encodeURIComponent(id)+'/price-compare',
    {buyer,supplier,costs,fxAsOf:priceDraft.asOf.trim()})
   setPriceReviews(prev=>({...prev,[id]:[result,...(prev[id]||[]).filter(x=>x.id!==result.id)].slice(0,12)}))
  }catch(e){setError((e as Error).message)}
  finally{setPriceBusy(false)}
 }
 async function decide(id:string,action:'hold'|'restore'|'reject'){
  if(busy)return
  setError('');setBusy('find')
  try{
   await post('/api/money/candidates/'+encodeURIComponent(id)+'/decision',{
    action,reason:candidateReason.trim().slice(0,500)
   })
   setCandidateReason('')
   await refresh()
   if(showAllCandidates)await loadCandidates()
  }catch(e){setError((e as Error).message)}
  finally{setBusy('')}
 }
 async function reject(id:string){
  if(busy)return
  setError('')
  try{
   await post('/api/money/candidates/'+encodeURIComponent(id)+'/decision',{action:'reject',reason:''})
   await refresh()
  }catch(e){setError((e as Error).message)}
 }
 const candidates=report.drafts||[]
 return <main className="mx-auto w-full max-w-5xl px-5 pb-12 pt-7 text-[#352C23] sm:px-8">
  <header className="mb-7 flex flex-wrap items-center justify-between gap-4">
   <div>
    <p className="mb-1 text-[11px] font-extrabold tracking-[.2em] text-[#BD7656]">DOMINION MONEY</p>
    <h1 className="flex items-center gap-2 text-[26px] font-extrabold tracking-tight sm:text-[34px]">
     <Wallet className="text-[#C96442]"/> 돈벌이 찾기
    </h1>
    <p className="mt-2 max-w-[650px] text-sm leading-6 text-[#84766A]">
     한국·중국·일본·미국·유럽 공개 자료의 구매·공급·비용 신호를 축적하고,
     무엇을 팔 수 있을지 검증 전 가설로 제시합니다.
    </p>
   </div>
  </header>
  <section className="mb-5 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-[#E4D7C8] bg-[#FFFCF6] px-4 py-3">
   <div>
    <p className="text-[12px] font-bold">머니 도구 활성화</p>
    <p className="mt-1 text-[11px] text-[#817367]">자동 수집과 별개의 설정입니다. 끄더라도 저장 자료와 기존 작업 기록은 유지됩니다.</p>
   </div>
   <label className="inline-flex cursor-pointer items-center gap-2 text-[12px] font-semibold">
    <input type="checkbox" aria-label="머니 도구 활성화" checked={moduleEnabled}
     disabled={moduleLoading||!!busy}
     onChange={event=>void configureMoney(event.target.checked)}/>
    {moduleEnabled?'사용 중':'사용 안 함'}
   </label>
  </section>
  <div className="mb-5 grid grid-cols-2 gap-3 sm:grid-cols-5">
   {([
    ['누적 시장 신호',stats?.signals],
    ['저장된 사업 후보',stats?.candidates],
    ['공개 기술 지표',stats?.metrics],
    ['현재 보고 근거',report.sourceCount],
    ['이번 모델 호출',report.modelCalls]
   ] as const).map(([name,value])=><section key={name}
    className="rounded-xl border border-[#E8DFD3] bg-[#FFFEFB] p-4">
     <p className="text-[11px] font-semibold text-[#9A8778]">{name}</p>
     <p className="mt-1 text-[24px] font-bold tabular-nums">{value===undefined?'—':value.toLocaleString()}</p>
    </section>)}
  </div>
  <section className="rounded-2xl border border-[#E4D7C8] bg-[#FFFCF6] p-5 shadow-sm">
   <div className="flex flex-wrap items-center gap-3">
    <button type="button" disabled={!moduleEnabled||!!busy||inferring}
     onClick={()=>void action('find')}
     className="flex items-center gap-2 rounded-xl bg-[#C96442] px-5 py-3 text-[13px] font-bold text-white disabled:opacity-50">
     {busy==='find'?<LoaderCircle size={17} className="animate-spin"/>:<Search size={17}/>} 돈벌이 찾기
    </button>
    <button type="button" disabled={!moduleEnabled||!!busy||inferring}
     onClick={()=>void action('scan')}
     className="flex items-center gap-2 rounded-xl border border-[#E1D3C4] bg-white px-4 py-3 text-[12px] font-semibold disabled:opacity-50">
     {busy==='scan'?<LoaderCircle size={16} className="animate-spin"/>:<RefreshCw size={16}/>}
     공개 자료 갱신
    </button>
    <label className="flex items-center gap-2 text-[12px] text-[#6B6259]">
     <input type="checkbox" checked={!!stats?.automation?.enabled}
      disabled={!moduleEnabled||!stats||!!busy} onChange={event=>
       void action('automation',{enabled:event.target.checked})}/>
     30분 간격 자동 수집 (모델 호출 없음)
    </label>
   </div>
   <div className="mt-4 flex flex-wrap items-center gap-2 border-t border-[#EEE3D8] pt-4">
    <label htmlFor="money-agent" className="text-[12px] font-semibold">AI 심화 추론</label>
    <select id="money-agent" value={agentId} onChange={event=>setAgentId(event.target.value)}
     className="min-w-[150px] max-w-[250px] rounded-lg border border-[#DCCFC1] bg-white px-3 py-2 text-[12px]">
     {agents.length?agents.map(a=><option key={a.id} value={a.id}>{a.name||a.id}</option>):
      <option value="general">일반 에이전트</option>}
    </select>
    <button type="button" disabled={!moduleEnabled||!!busy||inferring||!candidates.length}
     onClick={()=>void action('infer')}
     className="rounded-lg border border-[#D6B297] bg-[#F8EFE6] px-4 py-2 text-[12px] font-bold text-[#9B5633] disabled:opacity-50">
     {busy==='infer'?'분석 시작 중…':'선택 모델로 다시 추론'}
    </button>
    <label className="inline-flex items-center gap-2 text-[11px] text-[#998A7C]">
     모델 실행 한도
     <input type="number" min="0" max="1000000" step="10" value={budgetKrw}
      onChange={event=>setBudgetKrw(Number(event.target.value))}
      className="w-[100px] rounded-md border border-[#DCCFC1] bg-white px-2 py-1 text-[#514638]"/>
     원 · 제공자 요금 미등록 시 실제 과금과 차이 가능
    </label>
    {inferring&&<>
      <span className="inline-flex items-center gap-1 text-[12px] text-[#B16342]">
       <LoaderCircle size={14} className="animate-spin"/> 모델 분석 중
      </span>
      <button type="button" onClick={()=>void action('cancel')} disabled={!!busy}
       className="inline-flex items-center gap-1 text-[12px] text-rose-700"><Square size={12}/>취소</button>
    </>}
   </div>
   {!!report.inferenceCostKrw&&<p className="mt-2 text-[11px] text-[#84766A]">
    이번 분석 기록: {won(report.inferenceCostKrw)} · 입력 {report.inferenceInputTokens||0} / 출력 {report.inferenceOutputTokens||0} 토큰
   </p>}
   {stats?.lastCatalog?.capturedAt&&<p className="mt-3 text-[11px] text-[#998A7C]">
    GitHub·Hugging Face 지표 수집: {stats.lastCatalog.capturedAt} · 새 지표 {stats.lastCatalog.newMetrics??0}건
   </p>}
   {stats?.lastIngest?.capturedAt&&<p className="mt-3 text-[11px] text-[#998A7C]">
    마지막 수집: {stats.lastIngest.capturedAt} · 새 자료 {stats.lastIngest.new??0}건
   </p>}
  </section>
  <section className="mt-4 rounded-2xl border border-[#E4D7C8] bg-[#FFFCF6] p-4">
   <form onSubmit={event=>{event.preventDefault();void searchArchive()}}
    className="flex flex-wrap items-center gap-2">
    <label htmlFor="money-archive" className="text-[12px] font-semibold text-[#66594E]">축적 근거 검색</label>
    <input id="money-archive" value={archiveQuery} maxLength={160}
     onChange={event=>setArchiveQuery(event.target.value)}
     placeholder="산업·기술·고객 문제를 검색 (한국어·중국어·영어)"
     className="min-w-[180px] flex-1 rounded-lg border border-[#DCCFC1] bg-white px-3 py-2 text-[12px]"/>
    <button type="submit" disabled={!moduleEnabled||searchingArchive||archiveQuery.trim().length<2}
     className="rounded-lg bg-[#6C5B4B] px-4 py-2 text-[12px] font-bold text-white disabled:opacity-50">
     {searchingArchive?'검색 중…':'DB 검색 · 모델 호출 0회'}
    </button>
   </form>
   {archiveSearched&&<div className="mt-3 space-y-2" aria-live="polite">
    <p className="text-[11px] text-[#8B7D70]">누적 근거 {archiveResults.length}건 검색 · 기사 언급은 실제 구매 검증과 별개</p>
    {archiveResults.map(row=><div key={row.id} className="rounded-lg border border-[#ECE4D9] bg-white px-3 py-2">
     <a href={row.url} target="_blank" rel="noreferrer"
      className="text-[12px] font-semibold text-[#A75835] hover:underline">
      {row.title} <ArrowUpRight size={11} className="inline"/>
     </a>
     <p className="mt-1 text-[10px] text-[#8B7D70]">
      {row.sourceName||'공개 출처'} · {row.publishedAt||'발행일 미상'} · {row.verification||'검증 전'}
     </p>
     {row.feedOriginal&&<details className="mt-2">
      <summary className="cursor-pointer text-[11px] text-[#877567]">
       수집 당시 RSS 항목 펼치기{row.feedOriginalTruncated?' (길이 제한으로 일부 생략)':''}
      </summary>
      <pre className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap break-all rounded bg-[#F8F5EE] p-2 text-[10px]">
       {row.feedOriginal}
      </pre>
     </details>}
     <button type="button" onClick={()=>void showSourceHistory(row.id)}
      className="mt-2 text-[11px] font-semibold text-[#A75835] hover:underline">
      저장 버전·동일 제목 출처 비교
     </button>
     {sourceHistory[row.id]&&<div className="mt-2 space-y-2 rounded-lg bg-[#F8F5EE] p-3 text-[11px]">
      <p className="font-semibold">저장 리비전 {sourceHistory[row.id].revisions.length}건
       {sourceHistory[row.id].legacyRevisionUnavailable?' · 이전 버전 기록 없음':''}</p>
      {sourceHistory[row.id].revisions.map((rev,i)=><details key={rev.revisionId||i}>
       <summary className="cursor-pointer">{rev.revisionId?.slice(0,12)||'레거시'} · {rev.title}</summary>
       <p className="mt-1 whitespace-pre-wrap">{rev.description||'설명 없음'}</p>
       {rev.feedOriginal&&<pre className="mt-2 max-h-48 overflow-auto whitespace-pre-wrap break-all">{rev.feedOriginal}</pre>}
      </details>)}
      <p>동일 제목의 다른 URL {sourceHistory[row.id].relatedSameTitle.length}건
       · 독립 보도 또는 실제 거래를 의미하지 않습니다.</p>
      {sourceHistory[row.id].relatedSameTitle.map(other=><a key={other.id}
       className="block text-[#A75835] underline" href={other.url}
       target="_blank" rel="noreferrer">{other.sourceName||'다른 출처'} · {other.title}</a>)}
     </div>}
    </div>)}
   </div>}
  </section>
  {error&&<p role="alert" className="mt-4 rounded-lg border border-rose-200 bg-rose-50 p-3 text-sm text-rose-700">{error}</p>}
  {report.inferenceError&&<p role="status" className="mt-4 text-[12px] text-amber-800">{report.inferenceError}</p>}
  {report.message&&<p className="mt-5 text-[12px] text-[#84766A]">{report.message}</p>}
  <div className="mt-6 space-y-4">
   {candidates.map((lead,index)=><article key={lead.id}
    className="rounded-2xl border border-[#E8DFD3] bg-white p-5 shadow-[0_2px_10px_rgba(55,42,25,.04)]">
    <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
     <span className="text-[11px] font-bold text-[#C96442]">후보 {index+1} · {lead.origin==='model_hypothesis'?'AI 가설':lead.evidence?.length===2?'2개 출처 언급':'단일 출처'}</span>
     <span className="text-[10px] text-[#95857A]">{lead.evidenceStatus||'검증 전'}</span>
    </div>
    <h2 className="text-lg font-extrabold">{lead.title}</h2>
    <p className="mt-2 text-[13px] leading-6 text-[#4A3F34]">{lead.offer}</p>
    <dl className="mt-4 grid gap-4 text-[12px] sm:grid-cols-2">
     <div><dt className="font-bold text-[#9D735C]">누가 돈을 내는가</dt><dd className="mt-1 leading-5">{lead.customer||'잠재 고객 검증 필요'}</dd></div>
     <div><dt className="font-bold text-[#9D735C]">왜 돈이 되는가</dt><dd className="mt-1 leading-5">{lead.whyPay||'지불 의사 미확인'}</dd></div>
     <div><dt className="font-bold text-[#9D735C]">구현 방법</dt><dd className="mt-1 leading-5">{lead.implementationPlan||lead.firstTest||'원문 확인 후 설계'}</dd></div>
     <div><dt className="font-bold text-[#9D735C]">첫 유료 검증</dt><dd className="mt-1 leading-5">{lead.firstPaymentStep||'고객 확인 필요'}</dd></div>
    </dl>
    {lead.causalChain&&<p className="mt-3 text-[12px] leading-5 text-[#64584A]">
     <strong className="text-[#9D735C]">돈벌이 연결 논리: </strong>{lead.causalChain}
    </p>}
    {lead.existingAlternatives&&<p className="mt-2 text-[12px] leading-5 text-[#64584A]">
     <strong className="text-[#9D735C]">기존 대체재: </strong>{lead.existingAlternatives}
    </p>}
    {lead.firstBuyerAccess&&<p className="mt-2 text-[12px] leading-5 text-[#64584A]">
     <strong className="text-[#9D735C]">첫 구매자 접근: </strong>{lead.firstBuyerAccess}
    </p>}
    {lead.citationStatus&&<details className="mt-3 rounded-lg bg-[#F8F5EE] px-3 py-2 text-[11px] leading-5 text-[#776B5E]">
     <summary className="cursor-pointer font-semibold">인용·리비전 검증: {lead.citationStatus}</summary>
     {(lead.sourceQuoteChecks||[]).map((q,i)=><p key={i} className="mt-1">
      {q.sourceId} · {q.status} · {q.quote} {q.revisionId?'(버전 '+q.revisionId.slice(0,10)+')':''}
     </p>)}
     {!!lead.unmatchedSourceIds?.length&&<p className="mt-1 text-rose-700">
      저장 자료에 없는 출처: {lead.unmatchedSourceIds.join(', ')}
     </p>}
     {(lead.evidence||[]).map((e,i)=><p key={e.sourceId||i} className="mt-1">
      {e.sourceName||'근거 '+(i+1)} · {e.quoteStatus||'인용 미검증'} ·
      {e.revisionId?.slice(0,10)||'버전 정보 없음'}
      {e.quote?' · '+e.quote:''}
     </p>)}
    </details>}
    {lead.counterfactual&&<p className="mt-3 rounded-lg bg-[#F8F5EE] px-3 py-2 text-[11px] leading-5 text-[#776B5E]">
     실패 조건: {lead.counterfactual}
    </p>}
    <div className="mt-4 flex flex-wrap items-center gap-3 border-t border-[#EEEAE5] pt-3">
     {lead.implementationRunId?<>
      <span role="status" className="text-[11px] font-semibold text-[#946044]">
       공통 하네스 작업 · {implementationRuns[lead.implementationRunId]?.status||'상태 확인 중'} ·
       {implementationRuns[lead.implementationRunId]?.steps??0}단계
      </span>
      <a href="#/activity" className="text-[11px] font-semibold text-[#A75835] hover:underline">
       실행·승인 상세 보기
      </a>
      {implementationRuns[lead.implementationRunId]?.status==='paused'&&
       <button type="button" disabled={!!busy} onClick={()=>void resumeImplementation(lead.id,lead.implementationRunId!)}
        className="text-[11px] font-semibold text-[#A75835]">기존 승인 작업 재개</button>}
      {['queued','running','awaiting_approval','paused'].includes(implementationRuns[lead.implementationRunId]?.status||'')&&
       <button type="button" disabled={!!busy} onClick={()=>void cancelImplementation(lead.implementationRunId!)}
        className="text-[11px] text-rose-700">작업 취소</button>}
     </>:<button type="button" disabled={!moduleEnabled||!!busy||inferring||!Number.isFinite(budgetKrw)||budgetKrw<0||budgetKrw>1000000}
      onClick={()=>void implement(lead.id)}
      className="rounded-lg border border-[#D6B297] bg-[#F8EFE6] px-3 py-2 text-[11px] font-bold text-[#9B5633] disabled:opacity-50">
      {busy==='implement'&&implementationId===lead.id?'작업 요청 중…':'이 후보 구현 승인'}
     </button>}
     <span className="text-[10px] text-[#948678]">구현은 도미니언 공통 실행기 사용 · 외부 변경은 별도 승인</span>
     <span className="text-[11px] font-bold text-[#8B7D70]">근거 원문</span>
     {(lead.evidence||[]).map((item,i)=>item.url?.startsWith('https://')?
      <a key={item.sourceId||i} href={item.url} target="_blank" rel="noreferrer"
       className="inline-flex items-center gap-1 text-[11px] font-semibold text-[#A75835] hover:underline">
       {item.sourceName||'자료 '+(i+1)} <ArrowUpRight size={12}/>
      </a>:null)}
     <span className="ml-auto inline-flex items-center gap-1 text-[10px] text-[#948678]">
      <CheckCircle2 size={13}/>자료와 수익 검증 상태 분리
     </span>
     <button type="button" disabled={!moduleEnabled||!!busy||!!lead.implementationRunId}
      onClick={()=>void decide(lead.id,'hold')}
      className="text-[11px] text-[#9B8F86] hover:text-[#A75835]">나중에 검토</button>
     <button type="button" disabled={!moduleEnabled||!!busy||!!lead.implementationRunId} onClick={()=>void reject(lead.id)}
      className="text-[11px] text-[#9B8F86] hover:text-rose-600">후보 제외</button>
    </div>
   </article>)}
   {!loading&&!candidates.length&&<section className="grid place-items-center rounded-2xl border border-dashed border-[#DCD1C2] py-14 text-center text-[#887A6A]">
    <Database size={28} className="mb-3"/>
    <p className="text-sm font-semibold">저장된 기회가 아직 없습니다.</p>
    <p className="mt-2 text-[12px]">돈벌이 찾기를 누르면 공개 시장 자료부터 수집합니다.</p>
   </section>}
  </div>
  <section className="mt-6 rounded-2xl border border-[#E4D7C8] bg-[#FFFCF6] p-4">
   <button type="button" disabled={!moduleEnabled} aria-expanded={showAllCandidates}
    onClick={()=>setShowAllCandidates(value=>!value)}
    className="text-[13px] font-bold text-[#624E3F] disabled:opacity-50">
    {showAllCandidates?'보관한 후보 접기':'보류·제외·하위 후보 다시 보기'}
   </button>
   {showAllCandidates&&<div className="mt-3 space-y-3">
    <form className="flex flex-wrap gap-2" onSubmit={event=>{event.preventDefault();void loadCandidates()}}>
     <select aria-label="보관 후보 상태" value={candidatesFilter}
      onChange={event=>setCandidatesFilter(event.target.value as typeof candidatesFilter)}
      className="rounded-lg border border-[#DCCFC1] bg-white px-2 py-2 text-[12px]">
      <option value="all">전체 후보</option><option value="draft">미선택 후보</option>
      <option value="held">보류</option><option value="rejected">제외</option>
      <option value="implementation_requested">구현 승인</option>
     </select>
     <input aria-label="보관 후보 검색" value={candidateQuery} maxLength={160}
      onChange={event=>setCandidateQuery(event.target.value)}
      placeholder="후보 제목·제안·구매자 검색"
      className="min-w-[180px] flex-1 rounded-lg border border-[#DCCFC1] bg-white px-3 py-2 text-[12px]"/>
     <button type="submit" disabled={loadingCandidates}
      className="rounded-lg bg-[#6C5B4B] px-4 py-2 text-[12px] font-bold text-white disabled:opacity-50">
      {loadingCandidates?'조회 중…':'DB에서 검색'}
     </button>
    </form>
    <input aria-label="보류·제외 판단 이유" maxLength={500} value={candidateReason}
     onChange={event=>setCandidateReason(event.target.value)}
     placeholder="선택 사항: 보류·제외 판단 이유 (기록에 보존)"
     className="w-full rounded-lg border border-[#DCCFC1] bg-white px-3 py-2 text-[12px]"/>
    {candidatePage.map(item=><details key={item.id}
     className="rounded-lg border border-[#E8DFD3] bg-white p-3">
     <summary className="cursor-pointer text-[12px] font-semibold">
      {item.title} · {item.status||'draft'}
     </summary>
     <p className="mt-2 text-[12px] leading-5">{item.offer||'사업 제안 내용 없음'}</p>
     <p className="mt-1 text-[11px] text-[#84766A]">구매자: {item.customer||'추가 조사 필요'}</p>
     {item.citationStatus&&<p className="mt-2 text-[11px] text-[#A75835]">
      원문 인용 확인: {item.citationStatus}
      {!!item.unmatchedSourceIds?.length&&' · DB에 없는 출처: '+item.unmatchedSourceIds.join(', ')}
     </p>}
     {!!item.sourceQuoteChecks?.length&&<details className="mt-2">
      <summary className="cursor-pointer text-[11px] font-semibold">인용 불일치와 기록된 리비전</summary>
      {item.sourceQuoteChecks.map((check,i)=><p key={i} className="mt-1 text-[11px] text-[#817367]">
       {check.sourceId} · {check.status} · {check.revisionId?.slice(0,12)||'원문 버전 미확인'}
       {' — '+check.quote}
      </p>)}
     </details>}
     <div className="mt-2 flex flex-wrap gap-2">
      {(item.evidence||[]).filter(e=>e.url?.startsWith('https://')).map((e,i)=>
       <a key={e.sourceId||i} href={e.url} target="_blank" rel="noreferrer"
        className="text-[11px] text-[#A75835] underline">{e.sourceName||'출처 '+(i+1)}</a>)}
     </div>
     <details className="mt-3 border-t border-[#E8DFD3] pt-3">
      <summary className="cursor-pointer text-[12px] font-semibold text-[#A75835]">
       가격 차이·환율·전체 비용 비교 (선택)
      </summary>
      <p className="mt-2 text-[11px] text-[#8B7D70]">
       숫자는 사용자 입력 가정입니다. 원문 구매 계약이나 실거래로 인정하지 않으며,
       비용을 모르면 빈칸으로 남겨 계산을 보류합니다. 확인한 0원만 0으로 입력하세요.
      </p>
      <form className="mt-2 space-y-2" onSubmit={event=>{event.preventDefault();void comparePrice(item.id)}}>
       <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
        {([
         ['buyerAmount','구매자 제시 총액'],['supplierAmount','공급자 제시 총액'],
         ['quantity','양쪽 기준 수량'],['spec','동일 규격/사양'],
         ['unit','단위'],['asOf','가격·환율 기준일 YYYY-MM-DD']
        ] as Array<[keyof PriceFields,string]>).map(([key,label])=>
         <label key={key} className="text-[11px] text-[#71665D]">{label}
          <input aria-label={label} type="text" value={priceDraft[key]}
           onChange={event=>priceField(key,event.target.value)}
           className="mt-1 w-full rounded-lg border border-[#DCCFC1] p-2 text-[12px]"/>
         </label>)}
        {(['buyerCurrency','supplierCurrency'] as const).map((key)=>
         <label key={key} className="text-[11px] text-[#71665D]">
          {key==='buyerCurrency'?'구매 통화':'공급 통화'}
          <select value={priceDraft[key]} onChange={event=>priceField(key,event.target.value)}
           className="mt-1 w-full rounded-lg border border-[#DCCFC1] bg-white p-2 text-[12px]">
           {['KRW','USD','CNY','JPY','EUR','THB'].map(currency=><option key={currency}>{currency}</option>)}
          </select>
         </label>)}
        {(['buyerKind','supplierKind'] as const).map(key=>
         <label key={key} className="text-[11px] text-[#71665D]">
          {key==='buyerKind'?'구매가 성격':'공급가 성격'}
          <select value={priceDraft[key]} onChange={event=>priceField(key,event.target.value)}
           className="mt-1 w-full rounded-lg border border-[#DCCFC1] bg-white p-2 text-[12px]">
           <option value="">미확인</option><option value="asking">호가</option>
           <option value="indicative">희망·제시가격</option>
           <option value="claimed_transaction">실거래 주장 (미검증)</option>
          </select>
         </label>)}
        {([
         ['buyerFx','구매 통화 1단위당 원화 환율'],
         ['supplierFx','공급 통화 1단위당 원화 환율'],
         ['shippingKrw','배송·물류비 (원)'],['feesKrw','중개·결제수수료 (원)'],
         ['taxKrw','관세·세금 (원)'],['laborKrw','노동비 (원)'],
         ['returnsKrw','반품·환불 충당 (원)'],['otherKrw','기타 비용 (원)']
        ] as Array<[keyof PriceFields,string]>).map(([key,label])=>
         <label key={key} className="text-[11px] text-[#71665D]">{label}
          <input aria-label={label} type="text" value={priceDraft[key]}
           onChange={event=>priceField(key,event.target.value)}
           className="mt-1 w-full rounded-lg border border-[#DCCFC1] p-2 text-[12px]"/>
         </label>)}
        {(['buyerSourceId','supplierSourceId'] as const).map(key=>
         <label key={key} className="text-[11px] text-[#71665D]">
          {key==='buyerSourceId'?'구매 근거 출처':'공급 근거 출처'}
          <select value={priceDraft[key]} onChange={event=>{
           const sourceId=event.target.value
           const revisionId=(item.evidence||[]).find(e=>e.sourceId===sourceId)?.revisionId||''
           setPriceDraft(prev=>key==='buyerSourceId'
            ?{...prev,buyerSourceId:sourceId,buyerRevisionId:revisionId}
            :{...prev,supplierSourceId:sourceId,supplierRevisionId:revisionId})
          }}
           className="mt-1 w-full rounded-lg border border-[#DCCFC1] bg-white p-2 text-[12px]">
           <option value="">출처 없이 입력한 가정</option>
           {(item.evidence||[]).filter(e=>!!e.sourceId).map(e=>
            <option key={e.sourceId} value={e.sourceId}>{e.sourceName||e.claim||e.sourceId}</option>)}
          </select>
         </label>)}
        {(['buyerRevisionId','supplierRevisionId'] as const).map(key=>
         <label key={key} className="text-[11px] text-[#71665D]">
          {key==='buyerRevisionId'?'구매 근거 저장 리비전 (선택)':'공급 근거 저장 리비전 (선택)'}
          <input aria-label={key} type="text" maxLength={64} value={priceDraft[key]}
           placeholder="저장된 SHA-256 리비전 ID"
           onChange={event=>priceField(key,event.target.value)}
           className="mt-1 w-full rounded-lg border border-[#DCCFC1] p-2 text-[12px]"/>
         </label>)}
        {(['buyerQuote','supplierQuote'] as const).map(key=>
         <label key={key} className="text-[11px] text-[#71665D]">
          {key==='buyerQuote'?'구매 근거 원문 인용':'공급 근거 원문 인용'}
          <input aria-label={key} type="text" maxLength={400} value={priceDraft[key]}
           onChange={event=>priceField(key,event.target.value)}
           className="mt-1 w-full rounded-lg border border-[#DCCFC1] p-2 text-[12px]"/>
         </label>)}
       </div>
       <div className="flex flex-wrap gap-2">
        <button type="submit" disabled={priceBusy||!!busy}
         className="rounded-lg bg-[#6C5B4B] px-3 py-2 text-[12px] font-semibold text-white disabled:opacity-50">
         {priceBusy?'계산·기록 중…':'원화 차액 계산 후 후보 DB에 저장'}
        </button>
        <button type="button" disabled={priceBusy} onClick={()=>void loadPriceHistory(item.id)}
         className="rounded-lg border border-[#DCCFC1] px-3 py-2 text-[12px]">과거 비교 다시 보기</button>
       </div>
      </form>
      {(priceReviews[item.id]||[]).map(review=>
       <div key={review.id} className="mt-2 rounded-lg bg-[#F8F5EE] p-3 text-[11px]">
        <p className="font-semibold">{review.status==='comparable_user_input_not_validated_trade'
         ?'비교 가능한 가정 기준':'미확인 조건으로 계산 보류'}
         · {review.spreadKrw==null?'차액 미확인':review.spreadKrw.toLocaleString('ko-KR')+'원'}
        </p>
        {review.issues.map((issue,index)=><p key={index} className="mt-1">미확인: {issue}</p>)}
        <p className="mt-1">실제 계약·결제 검증: 없음 · 추정 환율은 입력값 기준</p>
        {Object.entries(review.sourceChecks||{}).map(([side,check])=><p key={side} className="mt-1 break-all text-[#76685A]">
         {side==='buyer'?'구매 근거':'공급 근거'}: {check.status}
         {check.revisionId?' · 리비전 '+check.revisionId.slice(0,12):''}
         {check.revisionStatus?' · '+check.revisionStatus:''}
         {check.revisionChangedSinceQuote?' · 이후 원문 갱신됨':''}
        </p>)}
        {review.createdAt&&<p className="mt-1 text-[#8B7D70]">저장: {review.createdAt}</p>}
       </div>)}
     </details>
     {!!item.decisionHistory?.length&&<p className="mt-2 text-[11px] text-[#8B7D70]">
      판단 이력: {item.decisionHistory.map(d=>d.action+(d.reason?' ('+d.reason+')':'')).join(' · ')}
     </p>}
     {!item.implementationRunId&&<div className="mt-3 flex flex-wrap gap-3 text-[11px]">
      {item.status!=='draft'&&<button type="button" disabled={!!busy}
       onClick={()=>void decide(item.id,'restore')} className="font-semibold text-[#A75835]">다시 검토</button>}
      {item.status!=='held'&&<button type="button" disabled={!!busy}
       onClick={()=>void decide(item.id,'hold')} className="text-[#8B7D70]">보류</button>}
      {item.status!=='rejected'&&<button type="button" disabled={!!busy}
       onClick={()=>void decide(item.id,'reject')} className="text-rose-700">제외</button>}
     </div>}
    </details>)}
    {!loadingCandidates&&!candidatePage.length&&<p className="text-[12px] text-[#84766A]">조건에 맞는 저장 후보가 없습니다.</p>}
    {candidatesMore&&<button type="button" disabled={loadingCandidates}
     onClick={()=>void loadCandidates(candidatesOffset,true)}
     className="rounded-lg border border-[#DCCFC1] px-3 py-2 text-[12px] disabled:opacity-50">이전 후보 더 보기</button>}
   </div>}
  </section>
 </main>
}
