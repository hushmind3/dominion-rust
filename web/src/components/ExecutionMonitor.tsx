import {useEffect,useState} from 'react'
import {Activity,Check,Clock3,Play,RefreshCw,ShieldAlert,Square,Terminal} from 'lucide-react'
import {get,post} from '../api'
import {Button,TextInput,TextArea} from './Primitives'
import {Panel,useRemote} from './UI'

type Call={id:string;name:string;arguments:unknown}
type Timeline={seq:number;kind:string;message:string;at_ms:number;detail?:unknown}
type NativeRun={
 id:string;instruction:string;status:string;created_at_ms:number;updated_at_ms:number;
 repository?:string|null;steps:number;max_steps:number;input_tokens:number;output_tokens:number;
 cost_krw:number;budget_krw:number;answer?:string|null;
 pending_approval?:Call|null;in_flight?:Call|null;read_batch_in_flight?:Call[];
 waiting_for_children?:boolean;cancel_after_tool?:boolean;events:Timeline[];
}
type NativeConfig={model:string;economy_model:string;configured:boolean}
const labels:Record<string,string>={
 queued:'대기',running:'실행 중',awaiting_approval:'승인 요청',
 paused:'일시 중단',completed:'완료',failed:'실패',cancelled:'취소됨'
}
const current=(s:string)=>['running','queued'].includes(s)
const money=(value:number)=>value.toLocaleString('ko-KR',{maximumFractionDigits:2})+'원'
const timestamp=(ms:number)=>new Date(ms).toLocaleString('ko-KR')
const receipt=(event:Timeline)=>['tool_started','tool_result','tool_error',
 'tool_reconciled','tool_denied','tool_policy_denied','tool_hook_denied',
 'mcp_remote_outcome_unknown','verification','approval_required',
 'tool_outcome_unknown'].includes(event.kind)

function ToolHistory({events}:{events:Timeline[]}){
 const history=events.filter(receipt).slice(-30).reverse()
 return <div className="mt-3 max-h-56 space-y-2 overflow-y-auto" aria-label="실제 실행 기록">
  {history.map(e=><div key={e.seq} className="flex items-start gap-2 rounded-lg bg-[#FAF7F1] px-3 py-2 text-xs">
   <Terminal size={13} className="mt-0.5 shrink-0 text-[#7C7469]"/>
   <span className="min-w-0 flex-1 break-words">{e.message}</span>
   <span className="shrink-0 text-[10px] text-[#7C7469]">{e.kind}</span>
  </div>)}
  {!history.length&&<p className="text-xs text-[#9C9388]">기록된 도구 실행이 없습니다.</p>}
 </div>
}

/** ExecutionMonitor uses the layout of frontend commit 879dc23d, but all
 * operations and receipts come from Dominion's independent Rust run store. */
export function ExecutionMonitor(){
 const runs=useRemote<NativeRun[]>('/api/v1/runs',[])
 const config=useRemote<NativeConfig>('/api/v1/config',{model:'',economy_model:'',configured:false})
 const [prompt,setPrompt]=useState('')
 const [repo,setRepo]=useState('')
 const [budget,setBudget]=useState('10000')
 const [maxSteps,setMaxSteps]=useState('30')
 const [selected,setSelected]=useState<string|null>(null)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [info,setInfo]=useState('')
 const [observation,setObservation]=useState('')
 const focused=runs.value.find(run=>run.id===selected)||runs.value[0]
 const activeCount=runs.value.filter(run=>current(run.status)).length
 useEffect(()=>{
  const timer=setInterval(()=>{void runs.reload()},4500)
  return()=>clearInterval(timer)
 },[runs.reload])
 const perform=async(work:()=>Promise<void>,notice:string)=>{
  if(busy)return
  setBusy(true);setError('');setInfo('')
  try{await work();await runs.reload();setInfo(notice)}
  catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const start=()=>void perform(async()=>{
  if(!prompt.trim())throw Error('작업 내용을 입력해 주세요.')
  const steps=Number(maxSteps),amount=Number(budget)
  if(!Number.isInteger(steps)||steps<1||steps>100)throw Error('모델 턴은 1~100이어야 합니다.')
  if(!Number.isFinite(amount)||amount<0||amount>1_000_000)throw Error('예산은 0~1,000,000원이어야 합니다.')
  const payload={instruction:prompt.trim(),max_steps:steps,budget_krw:amount}
  const run=await post<NativeRun>(repo.trim()?'/api/v1/runs/from-repository':'/api/v1/runs',
   repo.trim()?{...payload,repository:repo.trim()}:payload)
  setSelected(run.id);setPrompt('')
 },'Rust 엔진에서 작업을 시작했습니다.')
 const action=(suffix:string,body:unknown={})=>{
  if(!focused)return
  void perform(async()=>{
   await post('/api/v1/runs/'+encodeURIComponent(focused.id)+'/'+suffix,body)
   if(suffix==='reconcile')setObservation('')
  },suffix==='cancel'?'중단을 요청했습니다.':suffix==='reconcile'?'실제 도구 결과를 기록했습니다.':'실행 상태가 변경됐습니다.')
 }
 const canResume=focused?.status==='paused'&&!focused.in_flight&&
  !focused.pending_approval&&!focused.cancel_after_tool&&
  !focused.read_batch_in_flight?.length&&!focused.waiting_for_children
 return <Panel className="mb-8 overflow-hidden">
  <div className="flex flex-wrap items-center justify-between gap-3 border-b border-[#E7E0D5] px-5 py-4">
   <div className="flex items-center gap-3"><span className="grid size-10 place-items-center rounded-xl bg-[#24211D] text-white"><Activity size={20}/></span>
    <div><h2 className="m-0 text-lg font-bold text-[#24211D]">실행 모니터</h2>
     <p className="m-0 text-xs text-[#7C7469]">Rust 에이전트의 실행·도구 결과·승인을 실제 기록에서 확인합니다.</p></div></div>
   <div className="flex items-center gap-3"><span className="text-xs font-semibold text-[#5C544A]">진행 중 {activeCount}개</span>
    <Button size="sm" variant="ghost" onClick={()=>void runs.reload()}><RefreshCw size={14}/> 새로고침</Button></div>
  </div>
  <div className="border-b border-[#E7E0D5] p-5">
   <label htmlFor="execution-prompt" className="mb-2 block text-sm font-semibold text-[#3A342D]">새 작업</label>
   <TextArea id="execution-prompt" rows={3} maxLength={20000} value={prompt}
    onChange={e=>setPrompt(e.target.value)} placeholder="실제 실행할 목표를 입력하세요."/>
   <div className="mt-3 grid gap-2 sm:grid-cols-[minmax(0,1fr)_115px_100px]">
    <TextInput aria-label="Git 저장소" value={repo} onChange={e=>setRepo(e.target.value)} placeholder="작업 저장소 (선택)"/>
    <TextInput aria-label="예산 (원)" type="number" min="0" max="1000000" value={budget} onChange={e=>setBudget(e.target.value)}/>
    <TextInput aria-label="최대 모델 턴" type="number" min="1" max="100" value={maxSteps} onChange={e=>setMaxSteps(e.target.value)}/>
   </div>
   <div className="mt-3 flex flex-wrap items-center justify-between gap-3">
    <span className="text-[11px] text-[#746A5F]">활성 모델: {config.value.model||'서버 설정'} · 개별 실행 예산 적용</span>
    <Button variant="primary" disabled={busy||!prompt.trim()} onClick={start}><Play size={15}/> 작업 시작</Button>
   </div>
   {info&&<p role="status" className="mt-2 text-xs text-emerald-700">{info}</p>}
  </div>
  <div className="grid min-w-0 gap-4 p-5 lg:grid-cols-2">
   <div className="max-h-72 space-y-2 overflow-y-auto">
    {runs.value.map(run=><Button key={run.id} variant="ghost" onClick={()=>{setSelected(run.id);setObservation('')}}
     className={'!flex !w-full !items-center !justify-between !gap-2 !rounded-lg !border !px-3 !py-3 !text-left '+(focused?.id===run.id?'!border-[#D7A78F] !bg-[#F7E8E1]':'!border-[#E7E0D5]')}>
      <span className="min-w-0 flex-1"><strong className="block truncate text-xs text-[#3A342D]">{run.instruction}</strong>
       <small className="block truncate text-[11px] text-[#7C7469]">{run.repository||'일반 작업'} · {timestamp(run.created_at_ms)}</small></span>
      <span className="shrink-0 text-xs font-medium text-[#5C544A]">{labels[run.status]||run.status}</span>
    </Button>)}
    {!runs.value.length&&<p className="py-8 text-center text-sm text-[#7C7469]">진행한 작업이 없습니다.</p>}
   </div>
   <div className="min-w-0 border-t border-[#E7E0D5] pt-4 lg:border-l lg:border-t-0 lg:pl-5 lg:pt-0">
    {focused?<><div className="flex items-center justify-between gap-2">
      <h3 className="m-0 text-sm font-bold text-[#3A342D]">실행 상세</h3>
      <span className="text-xs font-semibold text-[#5C544A]">{labels[focused.status]||focused.status}</span>
     </div>
     <div className="mt-3 grid grid-cols-3 gap-2 rounded-lg bg-[#FAF7F1] p-3 text-center">
      <div><strong className="block text-lg">{focused.steps}</strong><small className="text-[10px]">모델 턴</small></div>
      <div><strong className="block text-lg">{focused.events.filter(e=>e.kind==='tool_result'||e.kind==='tool_error'||e.kind==='tool_reconciled').length}</strong><small className="text-[10px]">도구 결과</small></div>
      <div><strong className="block text-lg">{(focused.input_tokens+focused.output_tokens).toLocaleString()}</strong><small className="text-[10px]">총 토큰</small></div>
     </div>
     <p className="mt-3 flex items-center gap-2 text-xs text-[#7C7469]"><Clock3 size={13}/>{timestamp(focused.created_at_ms)} · {money(focused.cost_krw)} 사용 / {money(focused.budget_krw)} 예산</p>
     {focused.pending_approval&&<section className="mt-3 rounded-lg border border-[#E5C4AB] bg-white p-3">
       <p className="flex items-center gap-2 text-xs font-bold"><ShieldAlert size={14}/> {focused.pending_approval.name} 승인 요청</p>
       <pre className="mt-2 max-h-36 overflow-auto whitespace-pre-wrap break-all text-[11px]">{JSON.stringify(focused.pending_approval.arguments,null,2)}</pre>
       <div className="mt-2 flex gap-2">
        <Button variant="primary" size="sm" disabled={busy} onClick={()=>action('approval',{allow:true})}><Check size={13}/> 1회 승인</Button>
        <Button size="sm" disabled={busy} onClick={()=>action('approval',{allow:false})}>거부</Button>
       </div>
      </section>}
     {focused.in_flight&&<section className="mt-3 rounded-lg border border-[#E5C4AB] bg-white p-3">
       <p className="text-xs font-bold">결과를 확정하지 못한 도구 · {focused.in_flight.name}</p>
       <TextArea rows={3} className="mt-2" value={observation} onChange={e=>setObservation(e.target.value)}
        placeholder="실제로 확인한 결과를 적어야 합니다."/>
       <div className="mt-2 flex gap-2">
        <Button size="sm" disabled={busy||!observation.trim()} onClick={()=>action('reconcile',{succeeded:true,result:observation})}>적용됨</Button>
        <Button size="sm" disabled={busy||!observation.trim()} onClick={()=>action('reconcile',{succeeded:false,result:observation})}>적용 안 됨</Button>
       </div>
      </section>}
     {current(focused.status)&&<Button variant="danger" size="sm" className="mt-3" disabled={busy} onClick={()=>action('cancel')}><Square size={14}/> 중단</Button>}
     {canResume&&<Button size="sm" className="mt-3" disabled={busy} onClick={()=>action('resume')}><RefreshCw size={14}/> 재개</Button>}
     {focused.answer&&<p className="mt-3 whitespace-pre-wrap break-words rounded-lg bg-[#FAF7F1] p-3 text-xs">{focused.answer}</p>}
     <ToolHistory events={focused.events}/>
    </>:<p className="text-sm text-[#7C7469]">선택한 실행이 없습니다.</p>}
   </div>
  </div>
  {error&&<p role="alert" className="px-5 pb-4 text-xs text-rose-600">{error}</p>}
  {runs.error&&<p role="alert" className="px-5 pb-4 text-xs text-rose-600">{runs.error}</p>}
 </Panel>
}
