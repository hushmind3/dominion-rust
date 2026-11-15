import {useCallback,useEffect,useState} from 'react'
import {Activity,BrainCircuit,Check,Clapperboard,Cpu,FileText,GitBranch,Play,RefreshCw,ShieldAlert,Square,UsersRound} from 'lucide-react'
import {get,getBlob,post,setAuthToken} from '../api'
import {PageContainer} from '../components/Primitives'
import type {PageId} from '../components/WorkspaceShell'

type Event={seq:number;kind:string;message:string;detail?:unknown;at_ms:number}
type Call={id:string;name:string;arguments:unknown}
type Run={id:string;instruction:string;status:string;plan?:string|null;answer?:string|null;events:Event[];pending_approval?:Call|null;in_flight?:Call|null;repository?:string|null;base_commit?:string|null;parent_run_id?:string|null;steps:number;input_tokens:number;cached_input_tokens:number;cache_write_tokens?:number;output_tokens:number;context_chars_avoided:number;tool_schema_chars_avoided?:number;reserved_budget_krw?:number;delegated_spent_krw?:number;read_only?:boolean;waiting_for_children?:boolean;cancel_after_tool?:boolean;read_batch_in_flight?:Call[];cost_krw:number;budget_krw:number;created_at_ms:number}
type Room={id:string;topic:string;status:string;participants:{name:string;role:string;economy:boolean}[];messages:{speaker:string;content:string}[];rounds:number;max_rounds:number;cost_krw:number;budget_krw:number}
type Scene={heading:string;narration:string;seconds:number}
type Video={id:string;prompt:string;status:string;backend:string;scenes:Scene[];error?:string|null;cost_krw:number}
type Improvement={id:string;repository:string;target_file:string;target_files?:string[];attempts?:number;selection_reason?:string|null;instruction:string;test_command:string;score_command?:string|null;status:string;baseline?:{tests_passed:boolean;score?:number}|null;candidate?:{tests_passed:boolean;score?:number}|null;reflection?:string|null;error?:string|null;cost_krw:number}
type Config={model:string;economy_model:string;configured:boolean;shell_enabled:boolean;max_steps:number;mcp_server_count:number;context_max_chars:number;max_parallel_reads:number;denied_tools:string[];tool_result_max_chars:number}
type MemoryNote={id:string;scope:string;title:string;content:string;updated_at_ms:number}
type ProjectSkill={name:string;path:string}

type McpProgressView={progress:number;total?:number;message?:string}
const currentMcpProgress=(run:Run):McpProgressView|null=>{
 if(!run.in_flight?.name.startsWith('mcp__'))return null
 for(const event of (run.events||[]).slice().reverse()){
  if(event.kind!=='mcp_progress'||!event.detail||typeof event.detail!=='object')continue
  const detail=event.detail as Record<string,unknown>
  if(detail.call_id!==run.in_flight.id||typeof detail.progress!=='number')continue
  return {progress:detail.progress,total:typeof detail.total==='number'?detail.total:undefined,message:typeof detail.message==='string'?detail.message:undefined}
 }
 return null
}
const uncertainRemoteId=(run:Run):string|null=>{
 const event=(run.events||[]).slice().reverse().find(e=>e.kind==='mcp_remote_outcome_unknown')
 const detail=event?.detail
 return detail && typeof detail==='object' && typeof (detail as Record<string,unknown>).remote_request_id==='string'
  ? String((detail as Record<string,unknown>).remote_request_id) : null
}

const money=(n:number)=>Number(n||0).toLocaleString('ko-KR',{maximumFractionDigits:2})+'원'
const field='w-full rounded-lg border border-[#E2DAD0] bg-[#FFFDF9] px-3 py-2 text-[12px] outline-none focus:border-[#C96442]'
const btn='inline-flex min-h-9 items-center justify-center gap-2 rounded-lg border border-[#DAD2C6] bg-white px-3 py-2 text-[12px] font-semibold text-[#51463A] transition hover:bg-[#F8F0E5] disabled:opacity-45'
const primary='inline-flex min-h-9 items-center justify-center gap-2 rounded-lg bg-[#C96442] px-4 py-2 text-[12px] font-semibold text-white transition hover:bg-[#B15637] disabled:opacity-45'
const panel='rounded-xl border border-[#E7E0D5] bg-white p-5'
const paneTitle={home:'독자 Rust 엔진',chat:'에이전트 실행',agents:'자율 에이전트',rooms:'멀티모델 그룹',video:'영상 제작',rsi:'재귀적 자기개선',models:'모델 설정',connections:'연결 설정',settings:'엔진 설정',activity:'실행 현황',automations:'작업 실행',workflows:'작업 실행',memory:'장기 기억',extensions:'프로젝트 스킬',developer:'개발자 작업',capabilities:'실행 능력'} as Record<PageId,string>

function SceneEditor({scene,index,onSave,disabled}:{scene:Scene;index:number;onSave:(scene:Scene,index:number)=>Promise<unknown>;disabled:boolean}){
 const [heading,setHeading]=useState(scene.heading)
 const [narration,setNarration]=useState(scene.narration)
 const [seconds,setSeconds]=useState(String(scene.seconds))
 useEffect(()=>{setHeading(scene.heading);setNarration(scene.narration);setSeconds(String(scene.seconds))},[scene.heading,scene.narration,scene.seconds])
 return <div className="grid gap-2 rounded-lg border border-[#E9E3DB] bg-[#FCFAF7] p-3">
  <div className="text-[11px] font-bold text-[#796E62]">장면 {index+1}</div>
  <input className={field} aria-label="장면 제목" value={heading} onChange={e=>setHeading(e.target.value)}/>
  <textarea className={field} rows={2} aria-label="장면 내레이션" value={narration} onChange={e=>setNarration(e.target.value)}/>
  <div className="flex items-end gap-2"><label className="max-w-32 text-[11px]">길이(초)<input className={field} type="number" min="1" max="30" value={seconds} onChange={e=>setSeconds(e.target.value)}/></label>
  <button type="button" className={btn} disabled={disabled} onClick={()=>void onSave({heading,narration,seconds:Number(seconds)},index)}>이 장면 수정</button></div>
 </div>
}
export function RustWorkspacePage({area}:{area:PageId}){
 const mode=area==='rooms'?'rooms':area==='video'?'videos':area==='rsi'?'rsi':area==='memory'?'memory':area==='extensions'?'skills':['models','connections','settings'].includes(area)?'settings':'runs'
 const [runs,setRuns]=useState<Run[]>([])
 const [rooms,setRooms]=useState<Room[]>([])
 const [videos,setVideos]=useState<Video[]>([])
 const [improvements,setImprovements]=useState<Improvement[]>([])
 const [config,setConfig]=useState<Config|null>(null)
 const [notes,setNotes]=useState<MemoryNote[]>([])
 const [noteTitle,setNoteTitle]=useState('')
 const [noteContent,setNoteContent]=useState('')
 const [noteScope,setNoteScope]=useState('global')
 const [noteQuery,setNoteQuery]=useState('')
 const [skills,setSkills]=useState<ProjectSkill[]>([])
 const [skillText,setSkillText]=useState('')
 const [selectedId,setSelectedId]=useState('')
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 const [instruction,setInstruction]=useState('')
 const [repository,setRepository]=useState('')
 const [budget,setBudget]=useState('10000')
 const [steps,setSteps]=useState('30')
 const [directive,setDirective]=useState('')
 const [reconciliation,setReconciliation]=useState('')
 const [childInstruction,setChildInstruction]=useState('')
 const [childReadOnly,setChildReadOnly]=useState(true)
 const [artifact,setArtifact]=useState('')
 const [artifactNames,setArtifactNames]=useState<string[]>([])
 const [artifactBody,setArtifactBody]=useState('')
 const [roomTopic,setRoomTopic]=useState('')
 const [roomA,setRoomA]=useState('설계자')
 const [roomB,setRoomB]=useState('검증자')
 const [roleA,setRoleA]=useState('문제 해결과 전체 구조')
 const [roleB,setRoleB]=useState('비판적 검증과 비용 최적화')
 const [roomMessage,setRoomMessage]=useState('')
 const [videoPrompt,setVideoPrompt]=useState('')
 const [videoBackend,setVideoBackend]=useState('local')
 const [rsiRepo,setRsiRepo]=useState('')
 const [target,setTarget]=useState('')
 const [extraTargets,setExtraTargets]=useState('')
 const [automaticSelection,setAutomaticSelection]=useState(true)
 const [rsiGoal,setRsiGoal]=useState('')
 const [testCommand,setTestCommand]=useState('cargo test')
 const [scoreCommand,setScoreCommand]=useState('')
 const [token,setToken]=useState('')
 const refresh=useCallback(async()=>{
  if(mode==='runs')setRuns(await get<Run[]>('/api/v1/runs'))
  if(mode==='rooms')setRooms(await get<Room[]>('/api/v1/groups'))
  if(mode==='videos')setVideos(await get<Video[]>('/api/v1/videos'))
  if(mode==='rsi')setImprovements(await get<Improvement[]>('/api/v1/improvements'))
  if(mode==='settings')setConfig(await get<Config>('/api/v1/config'))
  if(mode==='memory')setNotes(await get<MemoryNote[]>('/api/v1/memory?scope='+encodeURIComponent(noteScope)+'&query='+encodeURIComponent(noteQuery)))
  if(mode==='skills')setRuns(await get<Run[]>('/api/v1/runs'))
 },[mode,noteScope,noteQuery])
 useEffect(()=>{
  let active=true
  const update=()=>{void refresh().catch(e=>{if(active)setError((e as Error).message)})}
  update()
  const timer=setInterval(update,2500)
  const onFocus=()=>update()
  window.addEventListener('dominion-refresh',onFocus)
  return()=>{active=false;clearInterval(timer);window.removeEventListener('dominion-refresh',onFocus)}
 },[refresh])
 useEffect(()=>{setSelectedId('');setError('');setNotice('')},[mode])
 const run=runs.find(x=>x.id===selectedId)||runs[0]
 const room=rooms.find(x=>x.id===selectedId)||rooms[0]
 const video=videos.find(x=>x.id===selectedId)||videos[0]
 const improvement=improvements.find(x=>x.id===selectedId)||improvements[0]
 const perform=async(task:()=>Promise<unknown>,success?:string)=>{
  if(busy)return
  setBusy(true);setError('');setNotice('')
  try{const result=await task();if(success)setNotice(success);await refresh();return result}
  catch(e){setError((e as Error).message);return undefined}
  finally{setBusy(false)}
 }
 const createRun=async()=>{
  if(!instruction.trim())return
  const data={instruction:instruction.trim(),budget_krw:Number(budget),max_steps:Number(steps)}
  const result=await perform(()=>post<Run>(repository.trim()?'/api/v1/runs/from-repository':'/api/v1/runs',repository.trim()?{...data,repository:repository.trim()}:data),'새 작업을 시작했습니다.')
  if(result)setSelectedId((result as Run).id)
 }
 const action=(id:string,kind:string,payload:unknown={})=>perform(()=>post('/api/v1/runs/'+id+'/'+kind,payload))
 const loadFiles=async(id:string)=>{
  const result=await perform(()=>get<{files:string[]}>('/api/v1/runs/'+id+'/files'))
  if(result){setArtifactNames((result as {files:string[]}).files);setArtifact('');setArtifactBody('')}
 }
 const openFile=async(id:string,name:string)=>{
  const result=await perform(()=>get<{content:string}>('/api/v1/runs/'+id+'/files/'+name.split('/').map(encodeURIComponent).join('/')))
  if(result){setArtifact(name);setArtifactBody((result as {content:string}).content)}
 }
 const saveScene=(scene:Scene,index:number)=>perform(()=>post('/api/v1/videos/'+video.id+'/scenes/'+index,scene),'장면을 수정했습니다.')
 const date=(ms:number)=>new Date(ms).toLocaleString('ko-KR')
 const title=paneTitle[area]||'Rust 실행 엔진'
 return <PageContainer className="!max-w-[1260px] !py-5">
  <div className="mb-5 flex flex-wrap items-start justify-between gap-3">
   <div><div className="mb-1 text-[10px] font-bold uppercase tracking-widest text-[#B05D41]">DOMINION · Rust Engine</div>
    <h1 className="text-[21px] font-extrabold tracking-tight">{title}</h1>
    <p className="mt-1 text-[12px] text-[#81776A]">메인 DOMINION 프런트엔드 · 독자 Rust SDK와 직접 연결된 실행 화면</p></div>
   <button type="button" className={btn} onClick={()=>void refresh().catch(e=>setError((e as Error).message))}><RefreshCw size={14}/> 새로고침</button>
  </div>
  {error&&<p role="alert" className="mb-4 rounded-lg border border-[#E7BBAC] bg-[#FFF2EC] p-3 text-[12px] text-[#A23F2A]">{error}</p>}
  {notice&&<p role="status" className="mb-4 rounded-lg border border-[#D8E5D7] bg-[#F5FAF2] p-3 text-[12px] text-[#47784A]">{notice}</p>}
  {mode==='runs'&&<>
   <section className={panel}>
    <h2 className="mb-3 flex items-center gap-2 text-[14px] font-bold"><Activity size={17} className="text-[#C96442]"/> 새 자율 작업</h2>
    <textarea aria-label="목표" className={field} rows={4} value={instruction} maxLength={20000} onChange={e=>setInstruction(e.target.value)} placeholder="무슨 작업을 맡길까요? 파일 편집, 조사, 구현, 검증 등 목표를 적어 주세요."/>
    <div className="mt-3 grid gap-3 sm:grid-cols-[minmax(0,1fr)_145px_125px]">
     <label className="text-[11px] font-semibold text-[#82796E]"><GitBranch size={13} className="mr-1 inline"/>작업할 Git 저장소 (선택)<input className={field} value={repository} onChange={e=>setRepository(e.target.value)} placeholder="workspace 내부 상대 경로"/></label>
     <label className="text-[11px] font-semibold text-[#82796E]">예산 (원)<input className={field} type="number" min="0" max="1000000" value={budget} onChange={e=>setBudget(e.target.value)}/></label>
     <label className="text-[11px] font-semibold text-[#82796E]">최대 모델 턴<input className={field} type="number" min="1" max="100" value={steps} onChange={e=>setSteps(e.target.value)}/></label>
    </div>
    <button type="button" className={primary+' mt-4'} disabled={busy||!instruction.trim()} onClick={()=>void createRun()}><Play size={15}/> 실행 시작</button>
   </section>
   <div className="mt-4 grid gap-4 lg:grid-cols-[280px_minmax(0,1fr)]">
    <section className={panel}><h2 className="mb-3 text-[13px] font-bold">실행 기록 <span className="text-[#B05D41]">{runs.length}</span></h2>
     <div className="max-h-[650px] space-y-2 overflow-auto">{runs.length===0?<p className="text-[12px] text-[#8B8175]">아직 작업이 없습니다.</p>:runs.map(item=><button type="button" key={item.id} onClick={()=>{setSelectedId(item.id);setArtifactNames([]);setArtifactBody('')}} className={'w-full rounded-lg border p-3 text-left text-[11px] '+(run?.id===item.id?'border-[#D7A78F] bg-[#FFF6F0]':'border-[#E8E1D7] bg-[#FCFAF7]')}>
      <span className="font-bold text-[#A95636]">{item.status}</span><p className="mt-1 line-clamp-2 text-[#4E473E]">{item.instruction}</p><small className="mt-2 block text-[#8F857B]">{date(item.created_at_ms)}</small></button>)}</div>
    </section>
    <section className={panel}>{!run?<p className="text-[12px] text-[#8B8175]">작업을 생성하면 진행 상황과 승인 요청이 표시됩니다.</p>:<>
     <div className="flex flex-wrap items-center justify-between gap-2"><h2 className="text-[14px] font-bold">작업 상세 <span className="text-[#A95636]">{run.status}</span></h2><span className="text-[11px] text-[#8E8376]">모델 {run.steps}턴 · 총사용 {money(run.cost_krw+(run.delegated_spent_krw||0))} · 예약 {money(run.reserved_budget_krw||0)} / {money(run.budget_krw)}</span></div>
     {run.repository&&<p className="mt-2 break-all text-[11px] text-[#806B58]">Git: {run.repository} · {run.base_commit?.slice(0,12)}</p>}
     <p className="mt-3 whitespace-pre-wrap rounded-lg bg-[#F9F6F2] p-3 text-[12px]">{run.instruction}</p>
     <div className="mt-3 text-[11px] text-[#7F766D]">입력 {run.input_tokens.toLocaleString()} · 캐시 읽기 {run.cached_input_tokens.toLocaleString()} · 캐시 쓰기 {(run.cache_write_tokens||0).toLocaleString()} · 출력 {run.output_tokens.toLocaleString()} 토큰 · 생략한 도구 스키마 약 {(run.tool_schema_chars_avoided||0).toLocaleString()}자</div>
     {run.waiting_for_children&&<p role="status" className="mt-3 rounded-lg bg-[#F7F2EA] p-3 text-[12px]">하위 에이전트 작업을 기다리는 중입니다. 완료 후 부모 엔진이 자동으로 검증을 이어갑니다.</p>}
     {run.plan&&<details className="mt-3 rounded-lg border border-[#ECE5DB] p-3"><summary className="cursor-pointer text-[12px] font-semibold">실행 계획</summary><pre className="mt-2 whitespace-pre-wrap text-[11px]">{run.plan}</pre></details>}
     {run.pending_approval&&<div className="mt-4 rounded-lg border border-[#E5B99F] bg-[#FFF8F1] p-3">
      <h3 className="flex items-center gap-2 text-[12px] font-bold"><ShieldAlert size={16}/> 실행 승인 필요</h3>
      <pre className="mt-2 max-h-52 overflow-auto whitespace-pre-wrap break-all text-[11px]">{run.pending_approval.name+'\n'+JSON.stringify(run.pending_approval.arguments,null,2)}</pre>
      <div className="mt-3 flex gap-2"><button type="button" className={primary} disabled={busy} onClick={()=>void action(run.id,'approval',{allow:true})}><Check size={14}/> 승인</button>
       <button type="button" className={btn} disabled={busy} onClick={()=>void action(run.id,'approval',{allow:false})}>거부</button></div>
     </div>}
     {run.status==='running'&&run.in_flight?.name.startsWith('mcp__')&&<div role="status" className="mt-4 rounded-lg border border-[#D6E0DB] bg-[#F5FAF7] p-3">
      <h3 className="text-[12px] font-bold">원격 MCP 도구 실행 중</h3>
      <p className="mt-1 text-[11px]">{run.in_flight.name} · {currentMcpProgress(run)?.message||'원격 응답 대기 중'}</p>
      {currentMcpProgress(run)?.total&&currentMcpProgress(run)!.total!>0&&<progress className="mt-2 w-full" max={currentMcpProgress(run)!.total} value={currentMcpProgress(run)!.progress}/>}
     </div>}
     {run.status==='paused'&&run.in_flight&&<div className="mt-4 rounded-lg border border-[#E5B99F] bg-[#FFF8F1] p-3">
      <h3 className="text-[12px] font-bold">결과 확인이 필요한 도구: {run.in_flight.name}</h3>
      <p className="mt-1 text-[11px]">연결을 종료해도 원격 작업이 중단됐다고 단정할 수 없습니다. 원격 시스템의 실제 결과를 확인한 뒤 기록하세요. 기록 동작은 도구를 다시 실행하지 않습니다.</p>
      {uncertainRemoteId(run)&&<p className="mt-2 break-all rounded-md bg-white p-2 font-mono text-[11px]">원격 요청 ID: {uncertainRemoteId(run)}</p>}
      <textarea className={field+' mt-2'} aria-label="실행 결과 설명" value={reconciliation} onChange={e=>setReconciliation(e.target.value)}/>
      <div className="mt-2 flex gap-2"><button type="button" className={btn} disabled={busy||!reconciliation.trim()} onClick={()=>void action(run.id,'reconcile',{succeeded:true,result:reconciliation})}>실행 성공 기록</button>
       <button type="button" className={btn} disabled={busy||!reconciliation.trim()} onClick={()=>void action(run.id,'reconcile',{succeeded:false,result:reconciliation})}>실행 실패 기록</button></div>
     </div>}
     <div className="mt-3 flex flex-wrap gap-2">{run.status==='running'&&<button type="button" className={btn} disabled={busy} onClick={()=>void action(run.id,'cancel')}><Square size={14}/> 중단</button>}
      {run.status==='paused'&&!run.in_flight&&!run.cancel_after_tool&&!run.read_batch_in_flight?.length&&!run.waiting_for_children&&<button type="button" className={btn} disabled={busy} onClick={()=>void action(run.id,'resume')}><RefreshCw size={14}/> 재개</button>}
      <button type="button" className={btn} disabled={busy} onClick={()=>void loadFiles(run.id)}><FileText size={14}/> 결과 파일</button>
     </div>
     {!['completed','failed','cancelled'].includes(run.status)&&<div className="mt-3 flex gap-2"><input className={field} value={directive} onChange={e=>setDirective(e.target.value)} placeholder="실행 중 추가 지시"/><button type="button" className={btn} disabled={busy||!directive.trim()} onClick={()=>void perform(async()=>{await post('/api/v1/runs/'+run.id+'/directive',{instruction:directive});setDirective('')})}>지시</button></div>}
     {run.answer&&<section className="mt-4 rounded-lg border border-[#E5DDCF] bg-[#FAF8F4] p-3"><h3 className="text-[12px] font-bold">최종 결과</h3><p className="mt-2 whitespace-pre-wrap break-words text-[12px]">{run.answer}</p></section>}
     <details className="mt-3" open={run.status==='running'||run.status==='awaiting_approval'}><summary className="cursor-pointer text-[12px] font-bold">실행 타임라인 ({run.events?.length||0})</summary>
      <div className="mt-2 max-h-64 space-y-2 overflow-auto">{(run.events||[]).slice(-30).map(e=><p key={e.seq} className="border-l-2 border-[#E3D4C5] pl-3 text-[11px]"><strong>{e.kind}</strong> · {e.message}</p>)}</div>
     </details>
     {artifactNames.length>0&&<div className="mt-4 rounded-lg border border-[#E5DDCF] p-3"><h3 className="text-[12px] font-bold">작업 산출물</h3>{artifactNames.map(name=><button type="button" key={name} className="mr-2 mt-2 rounded bg-[#F4EEE8] px-2 py-1 text-[11px]" onClick={()=>void openFile(run.id,name)}>{name}</button>)}</div>}
     {artifact&&<details open className="mt-3"><summary className="break-all text-[12px] font-bold">{artifact}</summary><pre className="mt-2 max-h-72 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-[#F8F4EF] p-3 text-[11px]">{artifactBody}</pre></details>}
     {!run.parent_run_id&&<div className="mt-4 border-t border-[#EEE7DE] pt-4">
      <h3 className="mb-2 text-[12px] font-bold">독립 서브에이전트 위임</h3>
      <label className="mb-2 flex items-center gap-2 text-[11px] text-[#81766B]"><input type="checkbox" checked={childReadOnly} onChange={e=>setChildReadOnly(e.target.checked)}/> 읽기 전용 조사 작업자 (쓰기·셸 차단)</label>
      <div className="flex gap-2"><input className={field} value={childInstruction} onChange={e=>setChildInstruction(e.target.value)} placeholder="하위 작업 지시"/><button type="button" className={btn} disabled={busy||!childInstruction.trim()} onClick={()=>void perform(async()=>{await post('/api/v1/runs/'+run.id+'/subagents',{instruction:childInstruction,read_only:childReadOnly,budget_krw:Math.max(0,run.budget_krw-run.cost_krw-(run.reserved_budget_krw||0)-(run.delegated_spent_krw||0))/4});setChildInstruction('')})}>위임</button></div>
      <ChildList parent={run.id}/>
     </div>}
    </>}</section>
   </div>
  </>}
  {mode==='rooms'&&<>
   <section className={panel}><h2 className="mb-3 flex items-center gap-2 text-[14px] font-bold"><UsersRound size={17}/> 그룹대화 시작</h2>
    <textarea className={field} rows={3} aria-label="그룹 토론 주제" placeholder="공유 주제·문제·목표" value={roomTopic} onChange={e=>setRoomTopic(e.target.value)}/>
    <div className="mt-3 grid gap-3 sm:grid-cols-2"><label className="text-[11px]">참가자 1<input className={field} value={roomA} onChange={e=>setRoomA(e.target.value)}/><input className={field+' mt-1'} value={roleA} onChange={e=>setRoleA(e.target.value)}/></label>
     <label className="text-[11px]">참가자 2<input className={field} value={roomB} onChange={e=>setRoomB(e.target.value)}/><input className={field+' mt-1'} value={roleB} onChange={e=>setRoleB(e.target.value)}/></label></div>
    <button type="button" className={primary+' mt-3'} disabled={busy||!roomTopic.trim()} onClick={()=>void perform(async()=>{const created=await post<Room>('/api/v1/groups',{topic:roomTopic,participants:[{name:roomA,role:roleA,economy:false},{name:roomB,role:roleB,economy:true}],budget_krw:5000,max_rounds:12});setSelectedId(created.id)},'그룹을 만들었습니다.')}><UsersRound size={15}/> 그룹 생성</button>
   </section>
   <div className="mt-4 grid gap-4 lg:grid-cols-[280px_minmax(0,1fr)]">
    <section className={panel}><h2 className="mb-3 text-[13px] font-bold">그룹 목록</h2>{rooms.map(r=><button type="button" key={r.id} onClick={()=>setSelectedId(r.id)} className={'mb-2 w-full rounded-lg border p-3 text-left text-[12px] '+(room?.id===r.id?'border-[#D7A78F] bg-[#FFF6F0]':'border-[#E8E1D7]')}>{r.topic}<span className="mt-1 block text-[11px] text-[#8C8174]">{r.status} · {r.rounds}/{r.max_rounds}턴</span></button>)}</section>
    <section className={panel}>{room?<><h2 className="text-[14px] font-bold">{room.topic}</h2><p className="mt-2 text-[11px] text-[#7A7269]">{room.participants.map(p=>p.name).join(' · ')} · {money(room.cost_krw)} / {money(room.budget_krw)}</p>
     <div className="mt-4 max-h-[500px] space-y-3 overflow-auto">{room.messages.map((m,i)=><div key={i} className="rounded-lg bg-[#F8F5F1] p-3"><strong className="text-[11px] text-[#B45E43]">{m.speaker}</strong><p className="mt-1 whitespace-pre-wrap text-[12px]">{m.content}</p></div>)}</div>
     {room.status==='open'&&<><div className="mt-4 flex gap-2"><input className={field} value={roomMessage} onChange={e=>setRoomMessage(e.target.value)} placeholder="참가자에게 의견 전달 (이름을 @멘션할 수 있습니다)"/><button type="button" className={btn} disabled={busy||!roomMessage.trim()} onClick={()=>void perform(async()=>{await post('/api/v1/groups/'+room.id+'/messages',{content:roomMessage});setRoomMessage('')})}>전송</button></div>
      <button type="button" className={primary+' mt-3'} disabled={busy} onClick={()=>void perform(()=>post('/api/v1/groups/'+room.id+'/next',{}))}><Play size={14}/> 다음 참가자 발언</button></>}</>:<p className="text-[12px]">그룹을 선택하세요.</p>}</section>
   </div>
  </>}
  {mode==='videos'&&<>
   <section className={panel}><h2 className="mb-3 flex items-center gap-2 text-[14px] font-bold"><Clapperboard size={17}/> 영상 생성</h2>
    <textarea className={field} rows={3} aria-label="영상 주제" placeholder="영상 주제, 분위기, 메시지 및 분량" value={videoPrompt} onChange={e=>setVideoPrompt(e.target.value)}/>
    <div className="mt-3 flex flex-wrap items-end gap-3"><label className="text-[11px]">영상 백엔드<select className={field} value={videoBackend} onChange={e=>setVideoBackend(e.target.value)}><option value="local">로컬 FFmpeg (기본)</option><option value="veo">Veo 생성 API (유료)</option></select></label>
     <button type="button" className={primary} disabled={busy||!videoPrompt.trim()} onClick={()=>void perform(async()=>{const v=await post<Video>('/api/v1/videos',{prompt:videoPrompt,backend:videoBackend});setSelectedId(v.id)},'영상 작업을 시작했습니다.')}><Play size={14}/> 제작 시작</button></div>
    <p className="mt-2 text-[11px] text-[#85776B]">로컬은 스토리보드·자막·내레이션 중심이며, Veo는 별도의 API 설정과 과금이 필요합니다.</p>
   </section>
   <div className="mt-4 grid gap-4 lg:grid-cols-[280px_minmax(0,1fr)]">
    <section className={panel}><h2 className="mb-3 text-[13px] font-bold">영상 목록</h2>{videos.map(v=><button type="button" key={v.id} onClick={()=>setSelectedId(v.id)} className={'mb-2 w-full rounded-lg border p-3 text-left text-[12px] '+(video?.id===v.id?'border-[#D7A78F] bg-[#FFF6F0]':'border-[#E8E1D7]')}>{v.prompt}<small className="mt-1 block text-[#8B8076]">{v.status} · {v.backend}</small></button>)}</section>
    <section className={panel}>{video?<><h2 className="text-[14px] font-bold">{video.prompt}</h2><p className="mt-1 text-[11px] text-[#85776B]">{video.status} · {money(video.cost_krw)}</p>
     {video.error&&<p role="alert" className="mt-3 text-[12px] text-rose-700">{video.error}</p>}
     {video.status==='completed'&&<AuthenticatedVideo id={video.id}/>}
     <div className="mt-4 grid gap-3">{video.scenes.map((s,i)=><SceneEditor key={video.id+'-'+i} scene={s} index={i} onSave={saveScene} disabled={busy||video.status==='rendering'}/>)}</div>
     {['draft','failed','completed'].includes(video.status)&&<button type="button" className={primary+' mt-4'} disabled={busy} onClick={()=>void perform(()=>post('/api/v1/videos/'+video.id+'/render',{}))}><RefreshCw size={14}/> 렌더링 {video.status==='completed'?'재실행':'실행'}</button>}
    </>:<p className="text-[12px]">영상 작업을 선택하세요.</p>}</section>
   </div>
  </>}
  {mode==='rsi'&&<>
   <section className={panel}><h2 className="mb-3 flex items-center gap-2 text-[14px] font-bold"><BrainCircuit size={17}/> 비가중치 재귀적 개선 실험</h2>
    <label className="block text-[11px]">개선할 Git 저장소 (작업공간 상대 경로)<input className={field} value={rsiRepo} onChange={e=>setRsiRepo(e.target.value)} placeholder="my-project"/></label>
    <label className="mt-3 flex items-center gap-2 text-[12px] font-semibold text-[#51463A]"><input type="checkbox" checked={automaticSelection} onChange={e=>setAutomaticSelection(e.target.checked)}/> AI가 코드 구조를 살펴보고 개선할 파일 자동 선택</label>
    <p className="mt-1 text-[11px] text-[#82776B]">자동 선택은 승인 후 시작합니다. 파일을 직접 지정하려면 선택을 해제하세요.</p>
    {!automaticSelection&&<div className="mt-3 grid gap-3 sm:grid-cols-2">
     <label className="text-[11px]">첫 번째 파일<input className={field} value={target} onChange={e=>setTarget(e.target.value)} placeholder="src/lib.rs"/></label>
     <label className="text-[11px]">추가 파일 (쉼표로 구분, 최대 8개)<input className={field} value={extraTargets} onChange={e=>setExtraTargets(e.target.value)} placeholder="src/engine.rs, src/sdk.rs"/></label>
    </div>}
    <textarea className={field+' mt-3'} rows={3} aria-label="개선 목표" value={rsiGoal} onChange={e=>setRsiGoal(e.target.value)} placeholder="무엇을 개선할까요?"/>
    <div className="mt-3 grid gap-3 sm:grid-cols-2"><label className="text-[11px]">테스트 명령<input className={field} value={testCommand} onChange={e=>setTestCommand(e.target.value)}/></label><label className="text-[11px]">개선 점수 명령 (기존 테스트가 통과한다면 필요)<input className={field} value={scoreCommand} onChange={e=>setScoreCommand(e.target.value)} placeholder="마지막 줄에 높은 점수가 좋도록 숫자 출력"/></label></div>
    <button type="button" className={primary+' mt-3'} disabled={busy||!rsiRepo.trim()||(!automaticSelection&&!target.trim())||!rsiGoal.trim()||!testCommand.trim()} onClick={()=>void perform(async()=>{const trial=await post<Improvement>('/api/v1/improvements',{repository:rsiRepo,target_file:automaticSelection?'':target.trim(),target_files:automaticSelection?null:[target,...extraTargets.split(/[,\n]/)].map(s=>s.trim()).filter((s,i,all)=>!!s&&all.indexOf(s)===i),instruction:rsiGoal,test_command:testCommand,score_command:scoreCommand||null});setSelectedId(trial.id)},'개선 후보를 생성했습니다. 승인 후 독립 평가를 실행합니다.')}><BrainCircuit size={14}/> 개선 후보 생성</button>
   </section>
   <div className="mt-4 grid gap-4 lg:grid-cols-[280px_minmax(0,1fr)]">
    <section className={panel}><h2 className="mb-3 text-[13px] font-bold">실험 목록</h2>{improvements.map(i=><button type="button" key={i.id} onClick={()=>setSelectedId(i.id)} className={'mb-2 w-full rounded-lg border p-3 text-left text-[12px] '+(improvement?.id===i.id?'border-[#D7A78F] bg-[#FFF6F0]':'border-[#E8E1D7]')}>{i.instruction}<small className="mt-1 block text-[#8B8076]">{i.status} · {i.target_files?.length?`${i.target_files.length}개 파일`:'소스 자동 탐색 대기'}</small></button>)}</section>
    <section className={panel}>{improvement?<><h2 className="text-[14px] font-bold">{improvement.instruction}</h2><p className="mt-2 text-[11px] text-[#8B8076]">{improvement.repository} / {improvement.target_files?.length?`${improvement.target_files.length}개 파일`:'파일 선택 전'} · {improvement.status} · {improvement.attempts||0}회 시도 · {money(improvement.cost_krw)}</p>
     {improvement.selection_reason&&<p className="mt-3 rounded-lg bg-[#F8F5F1] p-3 text-[12px]">자동 탐색: {improvement.selection_reason}<span className="mt-1 block text-[11px] text-[#807469]">{improvement.target_files?.join(' · ')}</span></p>}
     {improvement.baseline&&<p className="mt-3 text-[12px]">기준: {improvement.baseline.tests_passed?'테스트 통과':'테스트 실패'} · 점수 {improvement.baseline.score??'없음'}</p>}
     {improvement.candidate&&<p className="mt-2 text-[12px]">후보: {improvement.candidate.tests_passed?'테스트 통과':'테스트 실패'} · 점수 {improvement.candidate.score??'없음'}</p>}
     {improvement.reflection&&<p className="mt-3 whitespace-pre-wrap rounded-lg bg-[#F8F5F1] p-3 text-[12px]">{improvement.reflection}</p>}
     {improvement.error&&<p role="alert" className="mt-3 text-[12px] text-rose-700">{improvement.error}</p>}
     <div className="mt-4 flex flex-wrap gap-2">
      {improvement.status==='awaiting_approval'&&<button type="button" className={primary} disabled={busy} onClick={()=>void perform(()=>post('/api/v1/improvements/'+improvement.id+'/approve',{}))}><Check size={14}/> 평가 명령 승인·실행</button>}
      {(improvement.status==='evaluated'||improvement.status==='adopting')&&<button type="button" className={primary} disabled={busy} onClick={()=>void perform(()=>post('/api/v1/improvements/'+improvement.id+'/adopt',{}))}>{improvement.status==='adopting'?'중단된 채택 확인·복구':'개선 결과 채택'}</button>}
      {improvement.status==='adopted'&&<button type="button" className={btn} disabled={busy} onClick={()=>void perform(async()=>{const next=await post<Improvement>('/api/v1/improvements/'+improvement.id+'/descendant',{});setSelectedId(next.id)})}>후손 개선 실행</button>}
     </div>
    </>:<p className="text-[12px]">개선 실험을 선택하세요.</p>}</section>
   </div>
  </>}

  {mode==='memory'&&<>
   <section className={panel}>
    <h2 className="mb-3 text-[14px] font-bold">장기 기억 · 비가중치 학습</h2>
    <p className="mb-3 text-[12px] text-[#7A7065]">메모는 실행을 넘어 유지되며, 같은 저장소의 다음 작업에서 검색해 재사용합니다. 모델 가중치는 변경하지 않습니다.</p>
    <div className="grid gap-3 sm:grid-cols-2"><label className="text-[11px]">범위 (global 또는 작업 저장소 경로)<input className={field} value={noteScope} maxLength={512} onChange={e=>setNoteScope(e.target.value)}/></label>
     <label className="text-[11px]">기억 제목<input className={field} value={noteTitle} maxLength={120} onChange={e=>setNoteTitle(e.target.value)}/></label></div>
    <textarea className={field+' mt-3'} rows={3} value={noteContent} maxLength={4000} onChange={e=>setNoteContent(e.target.value)} placeholder="재사용할 사실, 제약, 확인된 교훈 또는 절차"/>
    <button type="button" className={primary+' mt-3'} disabled={busy||!noteTitle.trim()||!noteContent.trim()||!noteScope.trim()} onClick={()=>void perform(async()=>{
     await post('/api/v1/memory',{scope:noteScope,title:noteTitle,content:noteContent});
     setNoteTitle('');setNoteContent('')
    },'기억을 저장했습니다.')}>기억 저장</button>
   </section>
   <section className={panel+' mt-4'}>
    <h2 className="mb-3 text-[14px] font-bold">저장된 기억 · {notes.length}개</h2>
    <div className="mb-3 flex gap-2"><input className={field} value={noteQuery} onChange={e=>setNoteQuery(e.target.value)} placeholder="검색어"/><button type="button" className={btn} disabled={busy} onClick={()=>void perform(async()=>{
     const result=await get<MemoryNote[]>('/api/v1/memory?scope='+encodeURIComponent(noteScope)+'&query='+encodeURIComponent(noteQuery))
     setNotes(result)
    })}>검색</button></div>
    <div className="grid gap-3">{notes.map(note=><article key={note.id} className="rounded-lg border border-[#ECE5DC] bg-[#FCFAF7] p-3"><h3 className="text-[12px] font-bold">{note.title}</h3>
     <p className="mt-1 text-[11px] text-[#8C8175]">{note.scope} · {new Date(note.updated_at_ms).toLocaleString('ko-KR')}</p>
     <p className="mt-2 whitespace-pre-wrap text-[12px]">{note.content}</p>
    </article>)}</div>
   </section>
  </>}
  {mode==='skills'&&<section className={panel}>
   <h2 className="mb-3 text-[14px] font-bold">프로젝트 스킬 · 지연 로딩</h2>
   <p className="mb-3 text-[12px] text-[#7A7065]">실행 작업의 Git 작업공간에서 SKILL.md를 검색합니다. 필요한 절차만 모델이 선택적으로 읽습니다.</p>
   <label className="text-[11px]">스킬을 확인할 작업<select className={field} value={selectedId} onChange={e=>{setSelectedId(e.target.value);setSkills([]);setSkillText('')}}><option value="">실행을 선택하세요</option>{runs.map(r=><option key={r.id} value={r.id}>{r.instruction.slice(0,80)} · {r.id.slice(0,8)}</option>)}</select></label>
   <button type="button" className={btn+' mt-3'} disabled={busy||!selectedId} onClick={()=>void perform(async()=>{setSkills(await get<ProjectSkill[]>('/api/v1/runs/'+selectedId+'/skills'));setSkillText('')})}>스킬 목록 조회</button>
   <div className="mt-3 flex flex-wrap gap-2">{skills.map(skill=><button type="button" key={skill.path} className={btn} onClick={()=>void perform(async()=>{
    const result=await get<{content:string}>('/api/v1/runs/'+selectedId+'/skills/'+encodeURIComponent(skill.name))
    setSkillText(result.content)
   })}>{skill.name}</button>)}</div>
   {skillText&&<pre className="mt-4 max-h-96 overflow-auto whitespace-pre-wrap rounded-lg bg-[#F8F5F1] p-3 text-[12px]">{skillText}</pre>}
  </section>}
  {mode==='settings'&&<section className={panel}><h2 className="mb-3 flex items-center gap-2 text-[14px] font-bold"><Cpu size={17}/> Rust 실행 엔진 연결</h2>
    <p className="mb-3 text-[12px] text-[#796E62]">이 화면은 메인 프런트엔드의 UI를 사용하면서 독립 Rust SDK의 실제 설정을 읽습니다.</p>
    <label className="block text-[11px]">서버 접근 토큰 (설정된 경우)
     <div className="mt-1 flex gap-2"><input type="password" autoComplete="off" className={field} value={token} onChange={e=>setToken(e.target.value)} placeholder="DOMINION_ACCESS_TOKEN"/><button type="button" className={primary} onClick={()=>{setAuthToken(token);void refresh().catch(e=>setError((e as Error).message))}}>연결</button></div></label>
    {config?<div className="mt-5 grid gap-3 sm:grid-cols-2">{([['메인 모델',config.model],['경제 모델',config.economy_model],['모델 인증',config.configured?'연결됨':'미설정'],['셸 사용',config.shell_enabled?'허용':'비활성'],['최대 실행 턴',String(config.max_steps)],['MCP 서버',String(config.mcp_server_count)],['컨텍스트 상한',config.context_max_chars.toLocaleString()+'자'],['병렬 조회 상한',String(config.max_parallel_reads)+'개'],['차단 도구',config.denied_tools.length?config.denied_tools.join(', '):'없음'],['도구 결과 상한',config.tool_result_max_chars.toLocaleString()+'자']] as [string,string][]).map(([k,v])=><div key={k} className="rounded-lg bg-[#FAF7F2] p-3"><div className="text-[11px] text-[#8B8175]">{k}</div><strong className="mt-1 block text-[13px]">{v}</strong></div>)}</div>:<p className="mt-4 text-[12px] text-[#8B8175]">설정 정보를 읽는 중이거나 서버 인증이 필요합니다.</p>}
    <p className="mt-4 text-[11px] text-[#82776B]">서버의 모델·요금·실행 정책은 Rust 환경설정(.env.example)을 기준으로 변경합니다. 이 화면에서 바뀌지 않은 설정을 변경한 것처럼 표시하지 않습니다.</p>
   </section>}
 </PageContainer>
}
function ChildList({parent}:{parent:string}){
 const [items,setItems]=useState<Run[]>([])
 useEffect(()=>{let live=true;const update=()=>{void get<Run[]>('/api/v1/runs/'+parent+'/subagents').then(data=>{if(live)setItems(data)}).catch(()=>{})};update();const timer=setInterval(update,4000);return()=>{live=false;clearInterval(timer)}},[parent])
 return <div className="mt-2 space-y-2">{items.map(child=><div key={child.id} className="rounded-lg bg-[#F8F5F1] p-3 text-[11px]"><strong>{child.status}{child.read_only?" · 읽기 전용":""}</strong> · {child.instruction}<p className="mt-1">사용 {money(child.cost_krw)} / {money(child.budget_krw)}</p>{child.answer&&<p className="mt-2 whitespace-pre-wrap">{child.answer}</p>}</div>)}</div>
}

function AuthenticatedVideo({id}:{id:string}){
 const [src,setSrc]=useState('')
 const [error,setError]=useState('')
 useEffect(()=>{
  let alive=true
  let url=''
  const controller=new AbortController()
  void getBlob('/api/v1/videos/'+id+'/download',controller.signal).then(blob=>{
   if(!alive)return
   url=URL.createObjectURL(blob)
   setSrc(url)
  }).catch(e=>{if(alive)setError((e as Error).message)})
  return()=>{alive=false;controller.abort();if(url)URL.revokeObjectURL(url)}
 },[id])
 if(error)return <p role="alert" className="mt-3 text-[12px] text-rose-700">{error}</p>
 if(!src)return <p className="mt-3 text-[12px] text-[#807469]">영상 파일 불러오는 중…</p>
 return <video controls preload="metadata" className="mt-4 w-full max-w-[640px] rounded-xl" src={src}/>
}
