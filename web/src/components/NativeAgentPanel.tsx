import {useEffect,useState} from 'react'
import {Check,Play,RefreshCw,ShieldAlert,Square,Sparkles} from 'lucide-react'
import {get,post,type Provider} from '../api'
import {useLocale} from '../i18n'

type Call={id:string;name:string;arguments:unknown}
type Run={id:string;instruction:string;status:string;waiting_for_children?:boolean;answer?:string|null;pending_approval?:Call|null;in_flight?:Call|null;steps:number;cost_krw:number;budget_krw:number;input_tokens:number;cached_input_tokens:number;output_tokens:number;events:{seq:number;kind:string;message:string}[]}
export function NativeAgentPanel({providers}:{providers:Provider[]}){
 const en=useLocale()==='en'
 const [goal,setGoal]=useState('')
 const [repository,setRepository]=useState('')
 const [budget,setBudget]=useState('10000')
 const [run,setRun]=useState<Run|null>(null)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [observed,setObserved]=useState('')
 useEffect(()=>{
  let live=true
  const reload=()=>{void get<Run[]>('/api/v1/runs').then(rows=>{if(live)setRun(current=>rows.find(r=>r.id===current?.id)||rows[0]||null)}).catch(e=>{if(live)setError((e as Error).message)})}
  reload();const interval=setInterval(reload,2500)
  return()=>{live=false;clearInterval(interval)}
 },[])
 const send=async(action:()=>Promise<unknown>)=>{
  if(busy)return
  setBusy(true);setError('')
  try{await action();const runs=await get<Run[]>('/api/v1/runs');setRun(current=>runs.find(r=>r.id===current?.id)||runs[0]||null)}
  catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const create=async()=>{
  if(!goal.trim())return
  await send(async()=>{
   const data={instruction:goal.trim(),budget_krw:Number(budget)}
   const next=await post<Run>(repository.trim()?'/api/v1/runs/from-repository':'/api/v1/runs',repository.trim()?{...data,repository:repository.trim()}:data)
   setRun(next)
  })
 }
 const action=(suffix:string,data:unknown={})=>{if(run)void send(()=>post('/api/v1/runs/'+run.id+'/'+suffix,data))}
 return <section className="mt-4 rounded-xl border border-[#E7E0D5] bg-white">
  <div className="flex items-center gap-2 border-b border-[#F0EAE1] px-4 py-3"><Sparkles size={17} className="text-[#AF5B3A]"/><div>
   <h2 className="text-[13px] font-bold">{en?'Autonomous work':'독자 Rust 엔진 · 자율 작업'}</h2>
   <p className="mt-0.5 text-[11px] text-[#8A8176]">{en?'Tasks run in Dominion’s independent Rust engine.':'메인 홈에서 바로 도미니언 Rust 엔진으로 작업을 실행합니다.'}</p>
  </div></div>
  <div className="grid gap-3 p-4">
   <label htmlFor="native-goal" className="text-[12px] font-semibold">{en?'What would you like done?':'무엇을 해드릴까요?'}</label>
   <textarea id="native-goal" rows={3} maxLength={20000} value={goal} onChange={e=>setGoal(e.target.value)} placeholder="파일 생성·수정, 코드 조사, 구현, 검증 등 목표를 입력하세요." className="w-full resize-y rounded-xl border border-[#E7E0D5] bg-[#FCFBF9] p-3 text-[12px] outline-none focus:border-[#A36A4A]"/>
   <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_130px]">
    <label className="text-[11px] text-[#746A5F]">Git 저장소 (선택)<input className="mt-1 h-9 w-full rounded-lg border border-[#E7E0D5] px-3 text-[12px]" value={repository} onChange={e=>setRepository(e.target.value)} placeholder="workspace 상대 경로"/></label>
    <label className="text-[11px] text-[#746A5F]">예산 (원)<input className="mt-1 h-9 w-full rounded-lg border border-[#E7E0D5] px-3 text-[12px]" type="number" min="0" max="1000000" value={budget} onChange={e=>setBudget(e.target.value)}/></label>
   </div>
   <div className="flex flex-wrap items-center justify-between gap-2"><span className="text-[11px] text-[#796C5D]">모델: {providers[0]?.model||'서버 설정'}</span>
    <button type="button" disabled={busy||!goal.trim()} onClick={()=>void create()} className="inline-flex h-9 items-center gap-2 rounded-lg bg-[#AE5337] px-4 text-[12px] font-semibold text-white disabled:opacity-40"><Play size={14}/>{busy?'실행 중…':'작업 시작'}</button></div>
   {error&&<p role="alert" className="rounded-lg bg-[#FFF3EE] p-3 text-[11px] text-[#A54027]">{error}</p>}
   {run&&<div className="rounded-xl border border-[#E7E0D5] bg-[#FCFBF9] p-3">
    <div className="flex justify-between gap-2"><strong className="text-[12px]">{run.status}</strong><span className="text-[11px] text-[#82796D]">{run.steps}턴 · {run.cost_krw.toLocaleString('ko-KR',{maximumFractionDigits:2})}원 / {run.budget_krw.toLocaleString('ko-KR')}원</span></div>
    <p className="mt-2 line-clamp-2 text-[11px] text-[#766F67]">{run.instruction}</p>
    <p className="mt-2 text-[11px] text-[#82796D]">입력 {run.input_tokens.toLocaleString()} · 캐시 {run.cached_input_tokens.toLocaleString()} · 출력 {run.output_tokens.toLocaleString()} 토큰</p>
    {run.pending_approval&&<div className="mt-3 rounded-lg border border-[#E5C4AB] bg-white p-3">
     <p className="flex items-center gap-2 text-[12px] font-semibold"><ShieldAlert size={16}/> 승인 요청 · {run.pending_approval.name}</p>
     <pre className="mt-2 max-h-40 overflow-auto whitespace-pre-wrap break-all text-[11px]">{JSON.stringify(run.pending_approval.arguments,null,2)}</pre>
     <div className="mt-2 flex gap-2"><button type="button" disabled={busy} onClick={()=>action('approval',{allow:true})} className="inline-flex items-center gap-1 rounded-lg bg-[#AE5337] px-3 py-2 text-[11px] text-white"><Check size={13}/> 승인</button>
      <button type="button" disabled={busy} onClick={()=>action('approval',{allow:false})} className="rounded-lg border px-3 py-2 text-[11px]">거부</button></div></div>}
    {run.in_flight&&<div className="mt-3 rounded-lg border border-[#E5C4AB] bg-white p-3"><p className="text-[12px] font-bold">중단된 도구 결과 확인</p>
     <textarea className="mt-2 w-full rounded-lg border p-2 text-[12px]" value={observed} onChange={e=>setObserved(e.target.value)} placeholder="확인한 결과"/>
     <div className="mt-2 flex gap-2"><button type="button" disabled={busy||!observed.trim()} onClick={()=>action('reconcile',{succeeded:true,result:observed})} className="rounded-lg border px-2 py-1 text-[11px]">성공 기록</button>
      <button type="button" disabled={busy||!observed.trim()} onClick={()=>action('reconcile',{succeeded:false,result:observed})} className="rounded-lg border px-2 py-1 text-[11px]">실패 기록</button></div></div>}
    {run.answer&&<p className="mt-3 whitespace-pre-wrap break-words border-t border-[#E7E0D5] pt-3 text-[12px]">{run.answer}</p>}
    <div className="mt-3 flex flex-wrap gap-2">
     {run.status==='running'&&<button type="button" disabled={busy} onClick={()=>action('cancel')} className="inline-flex items-center gap-1 rounded-lg border px-3 py-2 text-[11px]"><Square size={13}/> 중단</button>}
     {run.status==='paused'&&!run.in_flight&&!run.waiting_for_children&&<button type="button" disabled={busy} onClick={()=>action('resume')} className="inline-flex items-center gap-1 rounded-lg border px-3 py-2 text-[11px]"><RefreshCw size={13}/> 재개</button>}
     <button type="button" onClick={()=>{window.location.hash='#/chat'}} className="rounded-lg border px-3 py-2 text-[11px]">실행 상세 보기</button>
    </div>
   </div>}
  </div>
 </section>
}
