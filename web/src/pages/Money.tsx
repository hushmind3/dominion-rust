import {useEffect,useState} from 'react'
import {ArrowUpRight,CheckCircle2,Database,LoaderCircle,Search,Wallet,RefreshCw,Square} from 'lucide-react'
import {get,post,type Agent} from '../api'

type Evidence={
 sourceId?:string;url?:string;claim?:string;publishedAt?:string;
 sourceName?:string;kind?:string;contentStatus?:string
}
type Lead={
 id:string;title:string;offer?:string;customer?:string;
 whyPay?:string;implementationPlan?:string;firstPaymentStep?:string;
 counterfactual?:string;firstTest?:string;evidenceStatus?:string;
 evidence?:Evidence[];origin?:string;status?:string
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
const initial:Report={drafts:[],modelCalls:0}
const won=(n:number)=>Math.max(0,n).toLocaleString('ko-KR')+'원'

export function MoneyPage(){
 const [report,setReport]=useState<Report>(initial)
 const [stats,setStats]=useState<Stats|null>(null)
 const [agents,setAgents]=useState<Agent[]>([])
 const [agentId,setAgentId]=useState('general')
 const [busy,setBusy]=useState<'find'|'infer'|'scan'|'automation'|'cancel'|''>('')
 const [loading,setLoading]=useState(true)
 const [error,setError]=useState('')
 const inferring=report.reasoningStatus==='running'||report.reasoningStatus==='queued'
 const refresh=async()=>{
  const [latest,summary]=await Promise.all([
   get<Report>('/api/money/find'),get<Stats>('/api/money/stats')
  ])
  setReport(latest);setStats(summary)
 }
 useEffect(()=>{
  let live=true
  void Promise.all([get<Report>('/api/money/find'),
   get<Stats>('/api/money/stats'),get<Agent[]>('/api/agents')])
   .then(([r,s,a])=>{if(live){setReport(r);setStats(s);setAgents(a)}})
   .catch(e=>{if(live)setError((e as Error).message)})
   .finally(()=>{if(live)setLoading(false)})
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
 async function action(kind:'find'|'infer'|'scan'|'automation'|'cancel',payload?:unknown){
  if(busy)return
  setBusy(kind);setError('')
  try{
   if(kind==='find')setReport(await post<Report>('/api/money/find',{mode:'db'}))
   if(kind==='infer')setReport(await post<Report>('/api/money/find',{
    mode:'agent',agentId,budgetKrw:100
   }))
   if(kind==='scan')await post('/api/money/collect',{force:true})
   if(kind==='automation')await post('/api/money/automation',payload)
   if(kind==='cancel')await post('/api/money/inference/cancel',{})
   await refresh()
  }catch(e){setError((e as Error).message)}
  finally{setBusy('')}
 }
 async function reject(id:string){
  if(busy)return
  setError('')
  try{
   await post('/api/money/candidates/'+encodeURIComponent(id)+'/reject',{})
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
    <button type="button" disabled={!!busy||inferring}
     onClick={()=>void action('find')}
     className="flex items-center gap-2 rounded-xl bg-[#C96442] px-5 py-3 text-[13px] font-bold text-white disabled:opacity-50">
     {busy==='find'?<LoaderCircle size={17} className="animate-spin"/>:<Search size={17}/>} 돈벌이 찾기
    </button>
    <button type="button" disabled={!!busy||inferring}
     onClick={()=>void action('scan')}
     className="flex items-center gap-2 rounded-xl border border-[#E1D3C4] bg-white px-4 py-3 text-[12px] font-semibold disabled:opacity-50">
     {busy==='scan'?<LoaderCircle size={16} className="animate-spin"/>:<RefreshCw size={16}/>}
     공개 자료 갱신
    </button>
    <label className="flex items-center gap-2 text-[12px] text-[#6B6259]">
     <input type="checkbox" checked={!!stats?.automation?.enabled}
      disabled={!stats||!!busy} onChange={event=>
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
    <button type="button" disabled={!!busy||inferring||!candidates.length}
     onClick={()=>void action('infer')}
     className="rounded-lg border border-[#D6B297] bg-[#F8EFE6] px-4 py-2 text-[12px] font-bold text-[#9B5633] disabled:opacity-50">
     {busy==='infer'?'분석 시작 중…':'선택 모델로 다시 추론'}
    </button>
    <span className="text-[11px] text-[#998A7C]">요청 1회 · 실행 예산 100원 (제공자 요금 미등록 시 실제 과금과 차이 가능)</span>
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
   {stats?.lastIngest?.capturedAt&&<p className="mt-3 text-[11px] text-[#998A7C]">
    마지막 수집: {stats.lastIngest.capturedAt} · 새 자료 {stats.lastIngest.new??0}건
   </p>}
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
    {lead.counterfactual&&<p className="mt-3 rounded-lg bg-[#F8F5EE] px-3 py-2 text-[11px] leading-5 text-[#776B5E]">
     실패 조건: {lead.counterfactual}
    </p>}
    <div className="mt-4 flex flex-wrap items-center gap-3 border-t border-[#EEEAE5] pt-3">
     <span className="text-[11px] font-bold text-[#8B7D70]">근거 원문</span>
     {(lead.evidence||[]).map((item,i)=>item.url?.startsWith('https://')?
      <a key={item.sourceId||i} href={item.url} target="_blank" rel="noreferrer"
       className="inline-flex items-center gap-1 text-[11px] font-semibold text-[#A75835] hover:underline">
       {item.sourceName||'자료 '+(i+1)} <ArrowUpRight size={12}/>
      </a>:null)}
     <span className="ml-auto inline-flex items-center gap-1 text-[10px] text-[#948678]">
      <CheckCircle2 size={13}/>자료와 수익 검증 상태 분리
     </span>
     <button type="button" disabled={!!busy} onClick={()=>void reject(lead.id)}
      className="text-[11px] text-[#9B8F86] hover:text-rose-600">후보 제외</button>
    </div>
   </article>)}
   {!loading&&!candidates.length&&<section className="grid place-items-center rounded-2xl border border-dashed border-[#DCD1C2] py-14 text-center text-[#887A6A]">
    <Database size={28} className="mb-3"/>
    <p className="text-sm font-semibold">저장된 기회가 아직 없습니다.</p>
    <p className="mt-2 text-[12px]">돈벌이 찾기를 누르면 공개 시장 자료부터 수집합니다.</p>
   </section>}
  </div>
 </main>
}
