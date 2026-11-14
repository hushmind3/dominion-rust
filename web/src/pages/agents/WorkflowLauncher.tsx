import {useMemo,useState} from 'react'
import {ArrowRight,Play,Workflow} from 'lucide-react'
import {post} from '../../api'
import {Button,SelectInput,TextInput} from '../../components/Primitives'
import {StatusPill} from '../../components/StatusPill'

type Flow={id?:string;name:string;steps:{agentId:string;harness?:string;sdkProvider?:string;sdkTools?:string}[]}
type Run={id:string;kind:string;status:string;input?:{workflowId?:string};
 result?:{sessionId?:string;outputs?:unknown[];artifacts?:unknown[];usage?:{totalTokens?:number}}}
export function WorkflowLauncher({agentId,flows,jobs,reload}:{
 agentId:string;flows:Flow[];jobs:Run[];reload:()=>Promise<unknown>
}){
 const linked=useMemo(()=>flows.filter(flow=>flow.id&&flow.steps?.some(step=>step.agentId===agentId)),[flows,agentId])
 const [selected,setSelected]=useState(''),[input,setInput]=useState('')
 const [busy,setBusy]=useState(false),[error,setError]=useState(''),[notice,setNotice]=useState('')
 const [pendingId,setPendingId]=useState('')
 const workflowId=linked.some(flow=>flow.id===selected)?selected:linked[0]?.id||''
 const recent=jobs.filter(job=>job.kind==='workflow'&&job.input?.workflowId===workflowId)
  .sort((a,b)=>String(b.id).localeCompare(String(a.id)))[0]
 if(!linked.length)return <p className="text-[11px] text-[#8A8176]">
  배정된 워크플로가 없습니다. <a className="underline" href="#/workflows">워크플로에서 단계를 지정</a>
 </p>
 const launch=async()=>{
  if(!workflowId||busy)return
  setBusy(true);setError('');setNotice('')
  const requestId=pendingId||crypto.randomUUID()
  setPendingId(requestId)
  try{
   const flow=linked.find(f=>f.id===workflowId)
   const sdkCostApproved=flow?.steps.some(s=>s.harness==='copilot-sdk'&&s.sdkProvider==='github')||false
   const sdkToolsApproved=flow?.steps.some(s=>s.harness==='copilot-sdk'&&s.sdkTools==='trusted_full')||false
   if((sdkCostApproved||sdkToolsApproved)&&!window.confirm(
    (sdkCostApproved?'GitHub Copilot SDK의 유료 모델 호출을 승인합니다. ':'')+
    (sdkToolsApproved?'외부 실행기의 셸·파일 수정 권한을 승인합니다. 관리자 활성화가 별도로 필요합니다. ':'')+
    '작업을 등록할까요?'))return
   const job=await post<Run>('/api/agents/'+encodeURIComponent(agentId)+'/workflows/'+
    encodeURIComponent(workflowId)+'/run',{input,requestId,sdkCostApproved,sdkToolsApproved})
   setNotice('실행 작업 '+job.id.slice(0,8)+' 등록 · 활동에서 상태 확인')
   setPendingId('')
   await reload()
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 return <section className="rounded-xl border border-[#E9E0D4] bg-[#FAF7F1] p-3">
  <div className="mb-2 flex items-center gap-2 text-[11px] font-semibold text-[#5B5147]"><Workflow size={14}/> 배정된 작업 실행</div>
  <SelectInput aria-label="워크플로 선택" className="w-full" value={workflowId}
   onChange={e=>{setSelected(e.target.value);setPendingId('');setError('')}}>
   {linked.map(flow=><option key={flow.id} value={flow.id}>{flow.name}</option>)}
  </SelectInput>
  <div className="mt-2 flex gap-2">
   <TextInput aria-label="워크플로 실행 입력" className="min-w-0 flex-1" value={input}
    onChange={e=>{setInput(e.target.value);setPendingId('')}} placeholder="이 작업에서 처리할 요청"/>
   <Button size="sm" disabled={busy} onClick={()=>void launch()}><Play size={13}/> {busy?'등록 중':'실행'}</Button>
  </div>
  {error&&<p role="alert" className="mt-2 text-[11px] text-red-700">{error}</p>}
  {notice&&<p className="mt-2 text-[11px] text-emerald-700">{notice}</p>}
  {recent&&<div className="mt-2 flex flex-wrap items-center justify-between gap-2 text-[11px] text-[#736A5F]">
   <StatusPill status={recent.status}>{recent.status}</StatusPill>
   {recent.result&&<span>완료 단계 {recent.result.outputs?.length||0} · 확인된 파일 {recent.result.artifacts?.length||0} · 모델 토큰 {recent.result.usage?.totalTokens??'미집계'}</span>}
   <a href="#/workflows" className="inline-flex items-center gap-1 font-semibold text-[#C96442]">
    결과·단계별 영수증 <ArrowRight size={12}/></a>
  </div>}
 </section>
}
