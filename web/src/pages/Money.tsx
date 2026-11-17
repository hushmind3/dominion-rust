import {useEffect,useState} from 'react'
import {ArrowUpRight,CheckCircle2,Database,LoaderCircle,Search,Wallet,RefreshCw,Square} from 'lucide-react'
import {get,post,type Agent} from '../api'

type Evidence={
 sourceId?:string;url?:string;claim?:string;publishedAt?:string;
 sourceName?:string;kind?:string;contentStatus?:string
}
type ArchivedSource={id:string;title:string;url:string;sourceName?:string;
 publishedAt?:string;verification?:string;description?:string;changeTags?:string[]}
type Lead={
 id:string;title:string;offer?:string;customer?:string;
 whyPay?:string;implementationPlan?:string;firstPaymentStep?:string;
 counterfactual?:string;firstTest?:string;evidenceStatus?:string;
 causalChain?:string;existingAlternatives?:string;firstBuyerAccess?:string;
 evidence?:Evidence[];origin?:string;status?:string;implementationRunId?:string
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
 const [candidatesMore,setCandidatesMore]=useState(false)
 const [candidatesOffset,setCandidatesOffset]=useState(0)
 const [loadingCandidates,setLoadingCandidates]=useState(false)
 const [archiveQuery,setArchiveQuery]=useState('')
 const [archiveResults,setArchiveResults]=useState<ArchivedSource[]>([])
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
 async function resumeImplementation(id:string){
  if(busy)return
  setError('');setBusy('implement')
  try{
   // Resume the SAME native Run; native journal rejects unresolved effects.
   await post('/api/v1/runs/'+encodeURIComponent(id)+'/resume',{})
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
     <span className="text-[11px] font-bold text-[#C96442]">후보 {index+1} · {lead.origin==='model_hypothesis'?'AI 가설':lead.evidence?.length===2?'독립 2개 출처':'단일 출처'}</span>
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
       <button type="button" disabled={!!busy} onClick={()=>void resumeImplementation(lead.implementationRunId!)}
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
     <div className="mt-2 flex flex-wrap gap-2">
      {(item.evidence||[]).filter(e=>e.url?.startsWith('https://')).map((e,i)=>
       <a key={e.sourceId||i} href={e.url} target="_blank" rel="noreferrer"
        className="text-[11px] text-[#A75835] underline">{e.sourceName||'출처 '+(i+1)}</a>)}
     </div>
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
