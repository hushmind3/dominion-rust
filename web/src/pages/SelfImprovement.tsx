import {useEffect,useState} from 'react'
import {BrainCircuit,CheckCircle2,ClipboardCheck,Play,RefreshCw,Square,Terminal} from 'lucide-react'
import {Provider,get,post,resolveApproval} from '../api'
import {ModelPicker} from '../components/ModelPicker'
import {Button,SelectInput,TextArea} from '../components/Primitives'
import {FormField,Headline,Notice,Panel,StateTag,useRemote} from '../components/UI'

type DevStatus={
 config:{sourceDir:string};git:boolean;branch:string;dirty:number;exists:boolean
}
type Job={
 id:string;status:string;sessionId?:string;error?:string;
 result?:{sessionId?:string;messageId?:string};promptState?:{inFlight?:boolean}
}
type Message={id:string;role:string;content:string;created_at:string}
type Approval={id:string;sessionId:string;status:string;tool:string;args:unknown}
const SAVED_JOB='dominion.rsi.activeJob'
const terminal=(status:string)=>['completed','failed','cancelled','needs_review'].includes(status)
const defaultTask='DOMINION의 100턴 회의실에서 오래된 결정과 교정을 기억하지 못하는 문제를 실제 코드에서 추적하고 수정한 뒤 관련 테스트로 검증해 주세요.'

export function SelfImprovementPage(){
 const providers=useRemote<Provider[]>('/api/providers',[])
 const dev=useRemote<DevStatus>('/api/dev/status',{
  config:{sourceDir:''},git:false,branch:'',dirty:0,exists:false,
 })
 const activeProviders=providers.value.filter(provider=>provider.enabled!==false)
 const [providerId,setProviderId]=useState('')
 const [model,setModel]=useState('')
 const [request,setRequest]=useState(defaultTask)
 const [jobId,setJobId]=useState(()=>{
  try{return localStorage.getItem(SAVED_JOB)||''}catch{return ''}
 })
 const [job,setJob]=useState<Job|null>(null)
 const [approvals,setApprovals]=useState<Approval[]>([])
 const [answer,setAnswer]=useState('')
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 const selectedProvider=activeProviders.find(provider=>provider.id===providerId)
 useEffect(()=>{
  if(!activeProviders.length)return
  if(!activeProviders.some(provider=>provider.id===providerId)){
   setProviderId(activeProviders[0].id)
   setModel(activeProviders[0].model||'')
  }
 },[providers.value,providerId])
 const sourceReady=dev.value.git&&dev.value.branch==='dominion-sub'&&dev.value.dirty===0
 useEffect(()=>{
  if(!jobId)return
  let alive=true
  const refresh=async()=>{
   try{
    const next=await get<Job>('/api/jobs/'+encodeURIComponent(jobId))
    if(!alive)return
    setJob(next)
    const session=next.sessionId||next.result?.sessionId
    if(session){
     const pending=await get<Approval[]>('/api/approvals')
     if(!alive)return
     setApprovals(pending.filter(a=>a.sessionId===session&&a.status==='pending'))
     if(terminal(next.status)){
      const messages=await get<Message[]>('/api/sessions/'+encodeURIComponent(session)+'/messages')
      if(!alive)return
      setAnswer([...messages].reverse().find(item=>item.role==='assistant'&&item.content.trim())?.content||'')
     }
    }
   }catch(e){if(alive)setError((e as Error).message)}
  }
  void refresh()
  const timer=window.setInterval(()=>void refresh(),1800)
  return()=>{alive=false;window.clearInterval(timer)}
 },[jobId])
 const start=async()=>{
  if(busy||!sourceReady||!providerId||!model.trim())return
  setBusy(true);setError('');setNotice('');setAnswer('');setApprovals([])
  try{
   const created=await post<Job>('/api/rsi/start',{providerId,model,request})
   setJob(created);setJobId(created.id)
   try{localStorage.setItem(SAVED_JOB,created.id)}catch{}
   setNotice('선택한 모델에 작업을 맡겼습니다. 코드 조사·수정·검증 과정은 실행 기록에 남습니다.')
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const stop=async()=>{
  if(!jobId)return
  try{
   await post('/api/jobs/'+encodeURIComponent(jobId)+'/cancel',{})
   setNotice('중단 요청을 전달했습니다.')
  }catch(e){setError((e as Error).message)}
 }
 const approve=async(id:string,allow:boolean)=>{
  try{
   await resolveApproval(id,allow)
   setApprovals(previous=>previous.filter(item=>item.id!==id))
  }catch(e){setError((e as Error).message)}
 }
 const working=job&&!terminal(job.status)
 const statusText=job?.status==='completed'?'작업 종료':job?.status==='running'?'모델이 코드 작업 중':job?.status==='queued'?'실행 대기':job?.status==='needs_review'?'추가 검토 필요':job?.status==='failed'?'실행 실패':job?.status==='cancelled'?'중단됨':''
 return <div className="mx-auto w-full max-w-[1080px] px-5 py-5 sm:px-7 lg:pb-10">
  <Headline eyebrow="DOMINION · RSI" title="재귀적 자기개선" description="모델을 선택하고 개선할 문제를 알려주면 Rust 하네스가 코드 조사·수정·테스트를 실행합니다." actions={<Button variant="secondary" onClick={()=>{void providers.reload();void dev.reload()}}><RefreshCw size={15}/> 새로고침</Button>}/>
  <Notice message={error||providers.error||dev.error}/>
  {notice&&<p role="status" className="mb-4 rounded-xl border border-[#E7E0D5] bg-white px-4 py-3 text-[12px] text-[#5C544A]">{notice}</p>}
  <Panel className="p-5">
   <div className="mb-5 flex items-center gap-3">
    <span className="grid size-10 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#A94E30]"><BrainCircuit size={22}/></span>
    <div><strong className="text-[15px]">AI에게 DOMINION 개선 맡기기</strong>
     <p className="mt-1 text-[12px] text-[#7C7469]">Git diff, SHA, JSON은 입력하지 않습니다. 모델이 기존 코드를 직접 읽고 작업합니다.</p></div>
   </div>
   <div className="grid gap-3 md:grid-cols-2">
    <FormField label="제공자">
     <SelectInput value={providerId} onChange={e=>{const id=e.target.value;setProviderId(id);setModel(activeProviders.find(p=>p.id===id)?.model||'')}} disabled={busy||Boolean(working)}>
      {activeProviders.length===0&&<option value="">연결된 제공자 없음</option>}
      {activeProviders.map(provider=><option key={provider.id} value={provider.id}>{provider.name}</option>)}
     </SelectInput>
    </FormField>
    <FormField label="수정 작업을 수행할 모델">
     <ModelPicker providerId={providerId} value={model} onChange={setModel} disabled={busy||Boolean(working)} placeholder="모델 선택"/>
    </FormField>
   </div>
   <div className="mt-4"><FormField label="어떤 문제를 고칠까요?">
    <TextArea rows={4} value={request} onChange={e=>setRequest(e.target.value)} disabled={busy||Boolean(working)} placeholder={defaultTask}/>
   </FormField></div>
   <div className="mt-4 flex flex-wrap items-center justify-between gap-3">
    <p className="text-[11px] text-[#7C7469]">소스: {dev.value.config.sourceDir||'미연결'} · 브랜치: {dev.value.branch||'미확인'}</p>
    <Button variant="primary" disabled={busy||Boolean(working)||!sourceReady||!providerId||!model.trim()} onClick={()=>void start()}>
     <Play size={16}/>{busy?'작업 등록 중…':'모델에게 수정 맡기기'}
    </Button>
   </div>
   {!sourceReady&&<p role="status" className="mt-3 rounded-lg bg-[#FAF0E7] p-3 text-[12px] text-[#8F4E32]">작업 전 개발자 메뉴에서 DOMINION 소스 폴더를 연결하고 dominion-sub 브랜치와 미저장 변경 0개를 확인해야 합니다.</p>}
   <p className="mt-4 text-[11px] text-[#8A8176]">모델은 소스 파일을 수정하고 테스트합니다. 파일 변경 등 기존 하네스의 실행 승인은 유지하며 main 수정·커밋·푸시는 요청하지 않습니다.</p>
  </Panel>
  {job&&<Panel className="mt-4 p-5">
   <div className="mb-3 flex flex-wrap items-center justify-between gap-2">
    <div className="flex items-center gap-2"><Terminal size={17}/><strong className="text-[14px]">실행 현황</strong><StateTag online={job.status==='completed'}>{statusText||job.status}</StateTag></div>
    {working&&<Button variant="secondary" onClick={()=>void stop()}><Square size={13}/> 중단</Button>}
   </div>
   <p className="break-all text-[11px] text-[#8A8176]">작업 ID: {job.id}</p>
   {!!approvals.length&&<div className="mt-4 grid gap-2">
    {approvals.map(item=><div key={item.id} className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-[#E7D8C8] bg-[#FBF6EF] p-3">
     <div><p className="text-[12px] font-semibold"><ClipboardCheck size={14} className="mr-1 inline"/> 실행 승인 필요: {item.tool}</p>
      <p className="mt-1 max-w-[550px] break-all text-[11px] text-[#7C7469]">{JSON.stringify(item.args).slice(0,340)}</p></div>
     <div className="flex gap-2"><Button variant="secondary" size="sm" onClick={()=>void approve(item.id,false)}>거절</Button><Button variant="primary" size="sm" onClick={()=>void approve(item.id,true)}>승인</Button></div>
    </div>)}
   </div>}
   {job.error&&<Notice message={job.error}/>}
   {answer&&<div className="mt-4 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-4">
    <div className="mb-2 flex items-center gap-2 text-[12px] font-bold"><CheckCircle2 size={15}/> 모델 작업 보고</div>
    <p className="whitespace-pre-wrap break-words text-[12px] leading-6 text-[#3A342D]">{answer}</p>
   </div>}
   {!answer&&!job.error&&<p className="mt-3 text-[12px] text-[#7C7469]">{working?'모델이 소스 조사·수정 작업을 진행하고 있습니다.':'완료된 답변이 있으면 이곳에 표시합니다.'}</p>}
   <p className="mt-3 text-[11px] text-[#8A8176]">작업이 종료됐어도 실제 수정과 테스트 성공 여부는 모델 보고와 실행 기록을 함께 확인해야 합니다. 자동 병합·배포는 하지 않습니다.</p>
  </Panel>}
 </div>
}
