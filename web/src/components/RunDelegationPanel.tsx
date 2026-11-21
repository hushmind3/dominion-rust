import {useState} from 'react'
import {ChevronDown,ChevronUp,Play,RefreshCw,UsersRound} from 'lucide-react'
import {post} from '../api'
import {Button,TextArea,TextInput} from './Primitives'
import {Notice,Panel,useRemote} from './UI'
import {useLocale} from '../i18n'

type Child={
 id:string;instruction:string;status:string;steps:number;max_steps:number;
 cost_krw:number;budget_krw:number;read_only:boolean;delegation_profile?:string
}
export function RunDelegationPanel({runId,parentStatus,isTeam,onSelect}:{
 runId:string;parentStatus:string;isTeam:boolean;onSelect:(id:string)=>void
}){
 const ko=useLocale()==='ko'
 const [open,setOpen]=useState(false)
 const children=useRemote<Child[]>(open?'/api/v1/runs/'+encodeURIComponent(runId)+'/subagents':null,[])
 const [instruction,setInstruction]=useState(''),[budget,setBudget]=useState('500'),[steps,setSteps]=useState('12')
 const [readOnly,setReadOnly]=useState(true)
 const [busy,setBusy]=useState(false),[error,setError]=useState('')
 const spawn=async()=>{
  const limit=Number(steps),cost=Number(budget)
  if(!instruction.trim()||instruction.length>20_000||!Number.isFinite(cost)||cost<=0||!Number.isInteger(limit)||limit<1||limit>100){
   setError(ko?'지시문·예산·실행 단계(1~100)를 확인하세요.':'Check instructions, positive budget and step limit (1–100).')
   return
  }
  setBusy(true);setError('')
  try{
   const child=await post<Child>('/api/v1/runs/'+encodeURIComponent(runId)+'/subagents',{
    instruction:instruction.trim(),budget_krw:cost,max_steps:limit,read_only:readOnly
   })
   setInstruction('')
   await children.reload()
   onSelect(child.id)
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 return <Panel className="mt-4 overflow-hidden">
  <button type="button" aria-expanded={open} onClick={()=>setOpen(v=>!v)} className="flex w-full items-center justify-between gap-2 p-3 text-left text-xs font-semibold">
   <span className="flex items-center gap-2"><UsersRound size={16}/>{ko?'하위 에이전트 · 작업 위임':'Delegated subagents'}{open?' ('+children.value.length+')':''}</span>
   {open?<ChevronUp size={15}/>:<ChevronDown size={15}/>}
  </button>
  {open&&<div className="space-y-3 border-t border-[#E7E0D5] p-3">
   <div className="flex justify-end"><Button size="sm" variant="ghost" onClick={()=>void children.reload()}><RefreshCw size={13}/>{ko?'목록 갱신':'Refresh'}</Button></div>
   {children.value.map(child=><button type="button" key={child.id} onClick={()=>onSelect(child.id)}
    className="flex w-full min-w-0 flex-wrap items-center gap-2 rounded-lg border border-[#E7E0D5] bg-white p-3 text-left hover:bg-[#FAF7F1]">
    <span className="rounded bg-[#F3EEE5] px-2 py-1 text-[11px] font-semibold">{child.status}</span>
    <span className="min-w-0 flex-1 truncate text-xs">{child.instruction}</span>
    <span className="text-[11px] text-[#7C7469]">{child.read_only?(ko?'읽기 전용':'Read only'):(ko?'수정 가능':'Writer')} · {child.steps}/{child.max_steps} · {child.cost_krw.toLocaleString()}원</span>
   </button>)}
   {!children.value.length&&!children.loading&&<p className="text-[11px] text-[#9C9388]">{ko?'아직 위임한 작업이 없습니다.':'No delegated runs yet.'}</p>}
   {parentStatus==='running'&&!isTeam&&<div className="rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3">
    <strong className="text-xs">{ko?'기존 실행에서 하위 에이전트 위임':'Delegate from this Run'}</strong>
    <p className="mt-1 text-[11px] text-[#7C7469]">{ko?'부모 작업의 남은 예산에서 하위 작업 비용이 예약됩니다. 모델·도구 권한은 Rust 엔진에서 상속·검증합니다.':'Budget is reserved from the parent; model identity and tool permissions remain owned by Rust.'}</p>
    <TextArea rows={2} className="mt-2" value={instruction} onChange={e=>setInstruction(e.target.value)} placeholder={ko?'하위 에이전트에게 위임할 목표':'Objective for the subagent'}/>
    <div className="mt-2 grid gap-2 sm:grid-cols-2">
     <label className="text-[11px]">{ko?'예약 예산 (원)':'Reserved budget (KRW)'}<TextInput type="number" min={1} value={budget} onChange={e=>setBudget(e.target.value)}/></label>
     <label className="text-[11px]">{ko?'최대 단계':'Max steps'}<TextInput type="number" min={1} max={100} value={steps} onChange={e=>setSteps(e.target.value)}/></label>
    </div>
    <label className="mt-2 flex items-center gap-2 text-xs"><TextInput type="checkbox" checked={readOnly} onChange={e=>setReadOnly(e.target.checked)}/>{ko?'읽기 전용 조사':'Read-only investigation'}</label>
    <Button variant="primary" size="sm" className="mt-2" disabled={busy||!instruction.trim()} onClick={()=>void spawn()}><Play size={13}/>{ko?'하위 작업 시작':'Start subagent'}</Button>
   </div>}
   <Notice message={error||children.error}/>
  </div>}
 </Panel>
}
