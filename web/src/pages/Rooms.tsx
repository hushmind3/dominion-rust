import {useTranscriptScroll} from '../hooks/useTranscriptScroll'
import {useCallback,useEffect,useMemo,useRef,useState,type FormEvent} from 'react'
import {Activity,ArrowDown,ArrowRight,ArrowUp,AtSign,BookOpenText,Bot,Check,ChevronDown,ChevronLeft,ChevronRight,Clock3,Copy,Hash,LoaderCircle,MessageCircle,MoreHorizontal,Plus,RefreshCw,Settings2,ShieldCheck,Square,TerminalSquare,Trash2,UserRound,UsersRound,X} from 'lucide-react'
import {get,post,del,resolveApproval,koreanReasoningEnabled,type Agent,type ChatMessage} from '../api'
import {Button,TextArea,TextInput,SelectInput} from '../components/Primitives'
import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {FormField,Notice,refresh,useRemote} from '../components/UI'
import './Rooms.css'
import {t,useLocale} from '../i18n'

const MAX_ROOM_PARTICIPANTS=32

type RoutingMode='discussion'|'round_robin'|'mentioned'
type RoomConfig={id:string;name:string;topic?:string;description?:string;ownerName?:string;agentIds:string[];routingMode?:RoutingMode;rounds?:number;maxTurns?:number;toolsEnabled?:boolean;autoLearning?:boolean;sessionId?:string;status?:string;activeAgentId?:string;pauseReason?:string;updatedAt?:string}
type RoomDraft={id?:string;name:string;topic:string;ownerName:string;agentIds:string[];routingMode:RoutingMode;rounds:number;maxTurns:number;toolsEnabled:boolean;autoLearning:boolean}
type Job={id:string;kind:string;status:string;stepIndex?:number;stepTotal?:number;error?:string;createdAt?:string;input?:{roomId?:string;sessionId?:string};result?:{memories?:number;status?:string};roomState?:{activeAgentId?:string;inFlight?:boolean}}
type Execution={id:string;status:string;agentId?:string;sessionId?:string;toolCalls?:number;tokens?:number;reason?:string}
type Operation={id:string;runId?:string;tool?:string;toolName?:string;name?:string;status?:string;result?:unknown;error?:string;arguments?:unknown}
type Approval={id:string;tool:string;args?:unknown;status:string}
type FeedState={room?:RoomConfig;sessionId?:string;jobs:Job[];executions:Execution[];learningJobs:Job[];operations:Operation[];approvals:Approval[];activeAgentId?:string;status?:string;queueDepth?:number}
type LiveDraft={turnId:string;agentId:string;text:string;reasoning:string;tool:string;error:string;done:boolean;messageId?:string}
type RoomEvent={roomId:string;agentId:string;turnId?:string;event:{type:string;text?:string;name?:string;message?:string;messageId?:string;terminal?:boolean}}
const emptyLive=(turnId:string,agentId:string):LiveDraft=>({turnId,agentId,text:'',reasoning:'',tool:'',error:'',done:false})
const initialFeed=():FeedState=>({jobs:[],executions:[],learningJobs:[],operations:[],approvals:[]})
const labels:Record<string,string>={queued:'응답 대기',running:'대화 중',completed:'완료',idle:'대기',paused:'중지됨',stopped:'중지됨',cancelled:'중지됨',failed:'실패',needs_review:'확인 필요',interrupted:'중단됨'}
const modes:Record<RoutingMode,{label:string;description:string}>={discussion:{label:'함께 대화',description:'선택한 참가자들이 앞선 대화를 읽고 차례로 답합니다.'},round_robin:{label:'한 명씩',description:'메시지마다 다음 참가자 한 명이 답합니다.'},mentioned:{label:'멘션한 사람만',description:'@이름 또는 아래 발언자 선택으로 응답할 사람을 지정합니다.'}}
const palette=['#668373','#9A745F','#6D7FA4','#967AA0','#B18C47','#6C9297','#AB7180','#7A8456','#8B7B69']
const active=(status?:string)=>status==='queued'||status==='running'
const stamp=(value?:string)=>{if(!value)return '';const normalized=value.includes('T')?value:value.replace(' ','T')+'Z';const date=new Date(normalized);return Number.isNaN(date.getTime())?'':date.toLocaleTimeString('ko-KR',{hour:'2-digit',minute:'2-digit'})}
const draftOf=(room?:RoomConfig):RoomDraft=>({id:room?.id,name:room?.name||'',topic:room?.topic||room?.description||'',ownerName:room?.ownerName||'나',agentIds:room?.agentIds||[],routingMode:room?.routingMode||'discussion',rounds:room?.rounds||1,maxTurns:room?.maxTurns||12,toolsEnabled:room?.toolsEnabled===true,autoLearning:room?.autoLearning!==false})
const agentColor=(agent?:Agent,index=0)=>/^#[0-9a-f]{3,8}$/i.test(agent?.color||'')?agent!.color!:palette[index%palette.length]

function Avatar({agent,name,owner=false,size='normal',index=0}:{agent?:Agent;name?:string;owner?:boolean;size?:'small'|'normal';index?:number}){
 return <span className={'room-avatar '+(size==='small'?'room-avatar-small ':'')+(owner?'room-avatar-owner':'')} style={owner?undefined:{backgroundColor:agentColor(agent,index)}}>{(agent?.avatar||agent?.avatarUrl)?<img src={String(agent.avatar||agent.avatarUrl)} alt=""/>:owner?<UserRound size={size==='small'?14:18}/>:<span>{(agent?.name||name||'?').slice(0,1)}</span>}</span>
}

export function RoomsPage({openSession}:{openSession:(id:string)=>void}){
 useLocale()
 const rooms=useRemote<RoomConfig[]>('/api/rooms',[])
 const agents=useRemote<Agent[]>('/api/agents',[])
 const [selected,setSelected]=useState(()=>{try{return localStorage.getItem('dominion.room.selected')||''}catch{return ''}})
 const [showList,setShowList]=useState(()=>window.innerWidth>=840)
 const [creating,setCreating]=useState<RoomDraft|null>(null)
 const [saving,setSaving]=useState(false),[error,setError]=useState(''),[query,setQuery]=useState('')
 const current=rooms.value.find(room=>room.id===selected)
 useEffect(()=>{if(rooms.loading)return;if(!rooms.value.some(room=>room.id===selected))setSelected(rooms.value[0]?.id||'')},[rooms.value,rooms.loading,selected])
 useEffect(()=>{try{if(selected)localStorage.setItem('dominion.room.selected',selected)}catch{}},[selected])
 const selectRoom=(id:string)=>{setSelected(id);setError('');if(window.innerWidth<840)setShowList(false)}
 const createRoom=async()=>{
  if(!creating||saving)return;setSaving(true);setError('')
  try{const created=await post<RoomConfig>('/api/rooms',creating);await rooms.reload();setSelected(created.id);setCreating(null);if(window.innerWidth<840)setShowList(false);refresh()}
  catch(e){setError((e as Error).message)}finally{setSaving(false)}
 }
 const filtered=rooms.value.filter(room=>(room.name+' '+(room.topic||'')).toLowerCase().includes(query.toLowerCase()))
 return <div className="rooms-app">
  {showList&&<button className="room-list-scrim" aria-label="대화방 목록 닫기" onClick={()=>setShowList(false)}/>}
  <aside className={'room-list '+(showList?'room-list-open':'room-list-closed')} aria-label="그룹 대화방">
   <div className="room-list-title"><div><span className="room-eyebrow">YOUR CIRCLE</span><h2>{t('rooms.listTitle')}</h2></div><Button size="icon" variant="ghost" title={t('rooms.new')} aria-label="새 대화방" onClick={()=>{setCreating(draftOf());setError('')}}><Plus size={19}/></Button></div>
   <div className="room-list-search"><TextInput aria-label="대화방 검색" placeholder={t('rooms.search')} value={query} onChange={e=>setQuery(e.target.value)}/></div>
   <div className="room-list-items">{filtered.map(room=><button key={room.id} className={'room-list-item '+(selected===room.id?'is-selected':'')} onClick={()=>selectRoom(room.id)} aria-current={selected===room.id?'true':undefined}><span className="room-list-icon"><Hash size={18}/></span><span className="room-list-copy"><strong>{room.name}</strong><small>{room.topic||'나와 에이전트가 함께하는 공간'}</small><span className="room-list-meta">{t('rooms.me')} + {room.agentIds?.length||0}{t('rooms.people')}{room.status==='running'&&<i title="대화 중"/>}</span></span></button>)}
    {!rooms.loading&&!filtered.length&&<div className="room-list-empty">{query?t('rooms.noSearch'):t('rooms.empty')}</div>}
   </div>
   <div className="room-list-footer"><UsersRound size={16}/><span>잡담도, 깊은 논의도.<br/>같은 대화방에서 이어가세요.</span></div>
  </aside>
  {current?<RoomConversation key={current.id} room={current} agents={agents.value} openList={()=>setShowList(v=>!v)} showList={showList} openSession={openSession} onSaved={async()=>{await rooms.reload();refresh()}} onRemoved={async()=>{setSelected('');await rooms.reload()}}/>:<div className="room-welcome"><div className="room-welcome-mark"><UsersRound size={40}/></div><span className="room-eyebrow">A PLACE FOR YOUR TEAM</span><h1>나도 대화의 한가운데.</h1><p>에이전트들을 한 공간으로 초대하세요.<br/>서로의 이야기를 읽고, 내 말에 반응하고,<br/>다음 이야기를 함께 이어갑니다.</p><Button variant="primary" onClick={()=>{setCreating(draftOf());setError('')}}><Plus size={16}/> 대화방 만들기</Button><button className="room-mobile-list-button" onClick={()=>setShowList(true)}>저장된 대화방 보기</button></div>}
  {(error||rooms.error||agents.error)&&!creating&&<div className="room-global-error"><Notice message={error||rooms.error||agents.error}/></div>}
  {creating&&<DialogBackdrop onDismiss={()=>{if(!saving)setCreating(null)}}><DialogPanel size="wide" aria-label="새 그룹 대화방"><div className="room-dialog-header"><div><span className="room-eyebrow">START A CONVERSATION</span><h2>누구와 함께할까요?</h2></div><Button size="icon" variant="ghost" aria-label="닫기" onClick={()=>setCreating(null)} disabled={saving}><X size={18}/></Button></div><div className="room-dialog-body"><RoomSettings value={creating} onChange={setCreating} agents={agents.value} disabled={saving}/><Notice message={error}/></div><div className="room-dialog-footer"><span>방을 만들면 내가 먼저 이야기를 시작할 수 있습니다.</span><Button variant="primary" onClick={()=>void createRoom()} disabled={saving||!creating.name.trim()||!creating.agentIds.length}>{saving?<LoaderCircle size={15} className="animate-spin"/>:<Plus size={15}/>} 대화방 만들기</Button></div></DialogPanel></DialogBackdrop>}
 </div>
}

function RoomSettings({value,onChange,agents,disabled=false}:{value:RoomDraft;onChange:(value:RoomDraft)=>void;agents:Agent[];disabled?:boolean}){
 const toggle=(id:string)=>onChange({...value,agentIds:value.agentIds.includes(id)?value.agentIds.filter(item=>item!==id):[...value.agentIds,id]})
 return <div className="room-settings-form">
  <FormField label="대화방 이름"><TextInput value={value.name} onChange={e=>onChange({...value,name:e.target.value})} placeholder="예: 우리들의 라운지" disabled={disabled}/></FormField>
  <FormField label="대화방 주제 · 선택"><TextArea rows={2} value={value.topic} onChange={e=>onChange({...value,topic:e.target.value})} placeholder="편하게 이야기할 공간, 또는 함께 풀어볼 의제" disabled={disabled}/></FormField>
  <FormField label="내 표시 이름"><TextInput value={value.ownerName} onChange={e=>onChange({...value,ownerName:e.target.value})} placeholder="나" disabled={disabled}/></FormField>
  <div className="room-roster-heading"><strong>함께할 에이전트</strong><span>{value.agentIds.length}/{MAX_ROOM_PARTICIPANTS}명</span></div>
  <div className="room-roster-options">{agents.map((agent,index)=>{const checked=value.agentIds.includes(agent.id);const unavailable=disabled||(!checked&&value.agentIds.length>=MAX_ROOM_PARTICIPANTS);return <label key={agent.id} className={'room-roster-option '+(checked?'checked':'')}><input type="checkbox" checked={checked} disabled={unavailable} onChange={()=>toggle(agent.id)}/><Avatar agent={agent} size="small" index={index}/><span><strong>{agent.name}</strong><small>{agent.model||'기본 모델 사용'}</small></span>{checked&&<Check size={14}/>}</label>})}{!agents.length&&<p className="room-note">에이전트 화면에서 먼저 참가자를 만들어주세요.</p>}</div>
  <FormField label="대화 방식"><SelectInput value={value.routingMode} onChange={e=>onChange({...value,routingMode:e.target.value as RoutingMode})} disabled={disabled}>{Object.entries(modes).map(([key,item])=><option key={key} value={key}>{item.label}</option>)}</SelectInput><small className="room-note">{modes[value.routingMode].description}</small></FormField>
  <div className="room-settings-numbers"><FormField label="대화 라운드"><SelectInput value={value.rounds} onChange={e=>onChange({...value,rounds:Number(e.target.value)})} disabled={disabled||value.routingMode==='round_robin'}>{[1,2,3,4].map(n=><option value={n} key={n}>{n}라운드</option>)}</SelectInput></FormField><FormField label="최대 AI 발언"><TextInput type="number" min={1} max={60} value={value.maxTurns} onChange={e=>onChange({...value,maxTurns:Math.min(60,Math.max(1,Number(e.target.value)||1))})} disabled={disabled}/></FormField></div>
  <label className="room-setting-check"><input type="checkbox" checked={value.autoLearning} onChange={e=>onChange({...value,autoLearning:e.target.checked})} disabled={disabled}/><span><strong>대화 후 기억 후보 추출</strong><small>근거가 있는 내용만 참가자별 기억으로 남깁니다.</small></span></label>
  <label className="room-setting-check"><input type="checkbox" checked={value.toolsEnabled} onChange={e=>onChange({...value,toolsEnabled:e.target.checked})} disabled={disabled}/><span><strong>이 대화방에서 도구 사용 허용</strong><small>새 요청부터 적용됩니다. 파일·명령 실행은 기존 승인 정책을 따릅니다.</small></span></label>
 </div>
}

function RoomConversation({room,agents,openList,showList,openSession,onSaved,onRemoved}:{room:RoomConfig;agents:Agent[];openList:()=>void;showList:boolean;openSession:(id:string)=>void;onSaved:()=>Promise<void>;onRemoved:()=>Promise<void>}){
 const [messages,setMessages]=useState<ChatMessage[]>([]),[feed,setFeed]=useState<FeedState>(initialFeed)
 const [loading,setLoading]=useState(true),[loadError,setLoadError]=useState(''),[error,setError]=useState('')
 const [input,setInput]=useState(''),[targets,setTargets]=useState<string[]>([]),[sending,setSending]=useState(false),[stopping,setStopping]=useState(false)
 const [inspector,setInspector]=useState(()=>window.innerWidth>=1450),[draft,setDraft]=useState<RoomDraft>(()=>draftOf(room)),[dirty,setDirty]=useState(false),[saving,setSaving]=useState(false),[saved,setSaved]=useState(false)
 const [knowledgeOpen,setKnowledgeOpen]=useState(false),[knowledge,setKnowledge]=useState<any[]>([]),[knowledgeBusy,setKnowledgeBusy]=useState(false),[knowledgeNotice,setKnowledgeNotice]=useState('')
 const [approvalBusy,setApprovalBusy]=useState('')
 const [showActivity,setShowActivity]=useState(false),[live,setLive]=useState<LiveDraft|null>(null),[moreBelow,setMoreBelow]=useState(false)
 const inputRef=useRef<HTMLTextAreaElement>(null),approvalRef=useRef<HTMLDivElement>(null),requestInFlight=useRef(false),reloadRequested=useRef(false),dataVersion=useRef(0),mounted=useRef(true),sendAttempt=useRef<{input:string;targets:string[];id:string;reasoningLanguage:'ko'|'default'}|null>(null)
 const activeTurnId=useRef<string|null>(null),retiredTurnIds=useRef(new Set<string>()),terminalTurnIds=useRef(new Set<string>())
 const current=feed.room||room
 const roster=useMemo(()=>current.agentIds.map(id=>agents.find(agent=>agent.id===id)).filter((a):a is Agent=>Boolean(a)),[current.agentIds,agents])
 const runningJobs=feed.jobs.filter(job=>active(job.status))
 const running=runningJobs.length>0||current.status==='running'||feed.status==='running'
 const activeAgentId=live?.agentId||feed.activeAgentId||current.activeAgentId||runningJobs.find(job=>job.roomState?.activeAgentId)?.roomState?.activeAgentId
 const speaker=agents.find(agent=>agent.id===activeAgentId)
 const latestJob=feed.jobs[0]
 const pendingApprovals=feed.approvals.filter(approval=>approval.status==='pending')
 const latestOperation=feed.operations[0]
 const mode=current.routingMode||'discussion'
 const sessionId=feed.sessionId||current.sessionId
 const visible=messages.filter(message=>(message.role==='user'&&!message.agent_id&&(!('metadata' in message)||(message as any).metadata?.authorKind!=='agent_context'))||(message.role==='assistant'&&Boolean(message.content.trim())))
 const learningActive=feed.learningJobs.filter(job=>active(job.status)).length
 const memoryCount=feed.learningJobs.filter(job=>job.status==='completed').reduce((sum,job)=>sum+(typeof job.result?.memories==='number'?job.result.memories:0),0)
 const load=useCallback(async():Promise<void>=>{
  if(requestInFlight.current){reloadRequested.current=true;return}
  requestInFlight.current=true
  const version=dataVersion.current
  try{
   const [rows,state]=await Promise.all([get<ChatMessage[]>('/api/rooms/'+room.id+'/messages'),get<FeedState>('/api/rooms/'+room.id+'/state')])
   if(!mounted.current)return
   // A GET begun before a confirmed write must not erase its owner message,
   // queued job or saved settings. Schedule one fresh read after it completes.
   if(version!==dataVersion.current){reloadRequested.current=true;return}
   const actualRows=Array.isArray(rows)?rows:[]
   setMessages(actualRows);setFeed({...initialFeed(),...state});setLoadError('')
   setLive(previous=>previous?.messageId&&actualRows.some(message=>message.role==='assistant'&&message.id===previous.messageId)?null:previous)
  }catch(e){if(mounted.current&&version===dataVersion.current)setLoadError((e as Error).message)}finally{
   requestInFlight.current=false
   if(mounted.current){setLoading(false);if(reloadRequested.current){reloadRequested.current=false;void load()}}
  }
 },[room.id])
 useEffect(()=>{
  mounted.current=true;void load();let debounce:ReturnType<typeof setTimeout>|undefined
  const update=()=>{clearTimeout(debounce);debounce=setTimeout(()=>void load(),160)}
  const stream=(event:Event)=>{
   const detail=(event as CustomEvent<RoomEvent>).detail;if(detail.roomId!==room.id)return
   const item=detail.event,turnId=detail.turnId||'legacy:'+detail.agentId
   if(retiredTurnIds.current.has(turnId))return
   if(item.type==='turn_start'){
    if(activeTurnId.current===turnId||terminalTurnIds.current.has(turnId))return
    if(activeTurnId.current){retiredTurnIds.current.add(activeTurnId.current);if(retiredTurnIds.current.size>100)retiredTurnIds.current.delete(retiredTurnIds.current.values().next().value!)}
    activeTurnId.current=turnId;setLive(emptyLive(turnId,detail.agentId));return
   }
   // Opening a tab halfway through a turn can miss turn_start. The first
   // observed invocation becomes current; other turns require their own start.
   if(activeTurnId.current===null)activeTurnId.current=turnId
   if(activeTurnId.current!==turnId)return
   if(['text','reasoning','tool_start','tool_end','tool_error'].includes(item.type)){
    if(terminalTurnIds.current.has(turnId))return
    setLive(previous=>{
     const base=previous?.turnId===turnId?previous:emptyLive(turnId,detail.agentId)
     if(base.done)return base
     return {...base,text:item.type==='text'?base.text+(item.text||''):base.text,reasoning:item.type==='reasoning'?base.reasoning+(item.text||''):base.reasoning,tool:item.type==='tool_start'?(item.name||'도구 실행'):['tool_end','tool_error'].includes(item.type)?'':base.tool}
    })
   }
   if(item.type==='error'){
    terminalTurnIds.current.add(turnId)
    setLive(previous=>({...previous?.turnId===turnId?previous:emptyLive(turnId,detail.agentId),done:true,tool:'',error:item.message||'응답 실행에 실패했습니다.'}));update()
   }
   if(item.type==='assistant_saved'){
    terminalTurnIds.current.add(turnId)
    setLive(previous=>previous?.turnId===turnId?{...previous,done:true,tool:'',messageId:item.messageId}:previous);update()
   }
   if(item.type==='done'){
    terminalTurnIds.current.add(turnId)
    setLive(previous=>previous?.turnId===turnId?(previous.error||previous.messageId?{...previous,done:true,tool:''}:null):previous);update()
   }
  }
  window.addEventListener('dominion-refresh',update);window.addEventListener('dominion-room-event',stream)
  const timer=setInterval(()=>void load(),2500)
  return()=>{mounted.current=false;clearTimeout(debounce);clearInterval(timer);window.removeEventListener('dominion-refresh',update);window.removeEventListener('dominion-room-event',stream)}
 },[room.id,load])
 useEffect(()=>{if(!dirty)setDraft(draftOf(current))},[current,dirty])
 useEffect(()=>{setTargets(previous=>previous.filter(id=>current.agentIds.includes(id)))},[current.agentIds])
 const transcriptRevision=JSON.stringify([messages.at(-1)?.id,messages.length,live?.turnId,live?.text.length,live?.reasoning.length,live?.tool,live?.done]);
 const {scrollRef,onScroll,jumpDown}=useTranscriptScroll(room.id,transcriptRevision,setMoreBelow);
 const updateDraft=(value:RoomDraft)=>{setDraft(value);setDirty(true);setSaved(false)}
 const save=async()=>{if(saving)return;setSaving(true);setError('');try{const savedRoom=await post<RoomConfig>('/api/rooms',draft);dataVersion.current+=1;setFeed(previous=>({...previous,room:savedRoom}));setDirty(false);setSaved(true);await load();await onSaved()}catch(e){setError((e as Error).message)}finally{setSaving(false)}}
 const remove=async()=>{if(!confirm('“'+current.name+'” 대화방을 삭제하시겠습니까?'))return;setError('');try{await del('/api/rooms/'+room.id);await onRemoved()}catch(e){setError((e as Error).message)}}
 const loadKnowledge=async()=>{setKnowledgeBusy(true);try{setKnowledge(await get<any[]>('/api/rooms/'+room.id+'/knowledge'))}catch(e){setError((e as Error).message)}finally{setKnowledgeBusy(false)}}
 const openKnowledge=async()=>{setKnowledgeOpen(true);setKnowledgeNotice('');await loadKnowledge()}
 const learnKnowledge=async()=>{if(knowledgeBusy)return;setKnowledgeBusy(true);setKnowledgeNotice('');try{await post('/api/rooms/'+room.id+'/knowledge/learn',{});setKnowledgeNotice('학습 작업을 등록했습니다. 완료 후 공유 지식이 갱신됩니다.');setTimeout(()=>void loadKnowledge(),2500)}catch(e){setError((e as Error).message)}finally{setKnowledgeBusy(false)}}
 const send=async(event?:FormEvent)=>{
  event?.preventDefault();if(!input.trim()||sending)return;setSending(true);setError('');jumpDown()
  const contents=input.trim(),recipients=[...targets]
  if(!sendAttempt.current||sendAttempt.current.input!==contents||JSON.stringify(sendAttempt.current.targets)!==JSON.stringify(recipients))sendAttempt.current={input:contents,targets:recipients,id:crypto.randomUUID(),reasoningLanguage:koreanReasoningEnabled()?'ko':'default'}
  try{const result=await post<{room:RoomConfig;message:ChatMessage;job:Job;sessionId:string}>('/api/rooms/'+room.id+'/messages',{input:contents,...(recipients.length?{agentIds:recipients}:{}),clientMessageId:sendAttempt.current.id,reasoningLanguage:sendAttempt.current.reasoningLanguage});dataVersion.current+=1;setMessages(previous=>previous.some(message=>message.id===result.message.id)?previous:[...previous,result.message]);setFeed(previous=>({...previous,room:result.room,sessionId:result.sessionId,jobs:[result.job,...previous.jobs.filter(job=>job.id!==result.job.id)]}));setInput('');setTargets([]);sendAttempt.current=null;await load();await onSaved();inputRef.current?.focus({preventScroll:true})}
  catch(e){setError((e as Error).message)}finally{setSending(false)}
 }
 const approve=async(id:string,allow:boolean)=>{if(approvalBusy)return;setApprovalBusy(id);setError('');try{await resolveApproval(id,allow);dataVersion.current+=1;setFeed(previous=>({...previous,approvals:previous.approvals.filter(approval=>approval.id!==id)}));refresh()}catch(e){setError((e as Error).message)}finally{await load();setApprovalBusy('')}}
 const stop=async()=>{if(stopping)return;setStopping(true);setError('');try{await post('/api/rooms/'+room.id+'/stop',{});dataVersion.current+=1;if(activeTurnId.current)terminalTurnIds.current.add(activeTurnId.current);setLive(null);await load();await onSaved()}catch(e){setError((e as Error).message)}finally{setStopping(false)}}
 const mention=(agent:Agent)=>{setTargets(previous=>previous.includes(agent.id)?previous.filter(id=>id!==agent.id):[...previous,agent.id]);inputRef.current?.focus({preventScroll:true})}
 const status=running?'running':feed.status||current.status||'idle'
 return <div className={'room-conversation '+(inspector?'has-inspector':'')}>
  <section className="room-main" aria-label={current.name+' 그룹 대화'}>
   <header className="room-header"><button className="room-list-toggle" aria-label={showList?'대화방 목록 접기':'대화방 목록 보기'} onClick={openList}><MessageCircle size={18}/></button><div className="room-header-icon"><Hash size={22}/></div><div className="room-header-copy"><h1>{current.name}</h1><p>{current.topic||'함께 이야기하고, 서로의 생각을 이어가는 공간'}</p></div><Button size="sm" variant="ghost" className="room-roster-toggle" onClick={()=>void openKnowledge()} aria-haspopup="dialog"><BookOpenText size={16}/><span>{t('rooms.sharedKnowledge')}</span><b>{knowledge.length||0}</b></Button><Button size="sm" variant="ghost" className="room-roster-toggle" onClick={()=>setInspector(v=>!v)} aria-expanded={inspector}><UsersRound size={16}/><span>{t('rooms.participants')}</span><b>{current.agentIds.length+1}</b></Button></header>
   <div className="room-presence"><div className="room-avatar-stack"><Avatar owner size="small"/>{roster.slice(0,6).map((agent,index)=><Avatar key={agent.id} agent={agent} size="small" index={index}/>)}</div><span><strong>{current.ownerName||'나'}</strong>{t('rooms.withAgents',{count:roster.length})}</span><span className="room-presence-dot">·</span><span>{modes[mode].label}</span><span className={'room-status '+(running?'is-running':'')}>{running&&<i/>}{labels[status]||status}</span></div>
   {(current.toolsEnabled||latestOperation||pendingApprovals.length>0)&&<div className="room-work-status"><span><ShieldCheck size={12}/>{current.toolsEnabled?'컴퓨터 도구 허용':'도구 사용 꺼짐'}</span>{pendingApprovals.length>0?<button onClick={()=>approvalRef.current?.focus({preventScroll:true})}>승인 대기 {pendingApprovals.length}건</button>:latestOperation?<button onClick={()=>setShowActivity(true)}><TerminalSquare size={11}/>{latestOperation.tool||latestOperation.toolName||'도구'} · {labels[latestOperation.status||'']||latestOperation.status||'기록'}</button>:<span>실제 실행 결과는 기록에서 확인할 수 있습니다.</span>}</div>}
   <div className="room-scroll" ref={scrollRef} onScroll={onScroll} role="log" aria-label="공유 대화 기록" aria-busy={loading}>
    {loading?<div className="room-feed-loading"><LoaderCircle size={20} className="animate-spin"/> {t('rooms.loading')}</div>:!visible.length?<div className="room-first-message"><div className="room-empty-avatars"><Avatar owner/>{roster.slice(0,3).map((agent,index)=><Avatar key={agent.id} agent={agent} index={index}/>)}</div><h2>{t('rooms.firstTitle')}</h2><p>{t('rooms.firstDesc')}<br/>{t('rooms.firstDesc2')}</p><div><span><AtSign size={14}/> 특정 참가자에게 말 걸기</span><span><BookOpenText size={14}/> 이 방의 기록은 계속 이어집니다</span></div></div>:<><div className="room-transcript-start"><span>{t('rooms.records')}</span></div>{visible.map(message=>{const owner=message.role==='user';const agent=agents.find(item=>item.id===message.agent_id);return <TranscriptMessage key={message.id} message={message} agent={agent} name={owner?(current.ownerName||'나'):(agent?.name||message.agent_id||'에이전트')} owner={owner} index={Math.max(0,current.agentIds.indexOf(message.agent_id||''))}/>})}</>}
    {live&&<div className="room-message room-message-agent room-message-live"><Avatar agent={agents.find(agent=>agent.id===live.agentId)}/><div className="room-message-main"><div className="room-message-byline"><strong>{agents.find(agent=>agent.id===live.agentId)?.name||live.agentId}</strong><span className="room-live-label">{!live.done&&<i/>}{live.error?'응답 중단':live.done?'저장 중':'응답 중'}</span></div>{live.reasoning&&<details className="room-reasoning"><summary>추론 보기</summary><pre>{live.reasoning}</pre></details>}{live.text?<MessageBody text={live.text}/>:!live.error&&<span className="room-typing"><i/><i/><i/></span>}{live.tool&&<div className="room-tool-live"><TerminalSquare size={13}/>{live.tool} 실행 중</div>}{live.error&&<p className="room-message-error">{live.error}</p>}</div></div>}
    {running&&!live&&<div className="room-message room-message-agent room-message-live"><Avatar agent={speaker}/><div className="room-message-main"><div className="room-message-byline"><strong>{speaker?.name||'참가자 응답 대기'}</strong><span className="room-live-label">{speaker?'응답을 준비하고 있습니다':'작업 대기열에서 기다리고 있습니다'}</span></div>{speaker&&<span className="room-typing"><i/><i/><i/></span>}</div></div>}
    {loadError&&<Notice message={loadError}/>}
   </div>
   {moreBelow&&<button className="room-new-messages" onClick={jumpDown}><ArrowDown size={14}/> {t('rooms.newMessages')}</button>}
   <div className="room-composer-wrap">
    {(latestJob?.error||current.pauseReason)&&<div className="room-run-notice"><Activity size={14}/><span>{latestJob?.error||current.pauseReason}</span><button onClick={()=>setShowActivity(true)}>실행 기록 <ChevronRight size={13}/></button></div>}
    {pendingApprovals.length>0&&<div className="room-approvals-stack" ref={approvalRef} tabIndex={-1}>{pendingApprovals.map(approval=><div key={approval.id} className="room-approval" data-room-approval><div><ShieldCheck size={15}/><strong>{approval.tool}</strong><span>실행 승인 요청</span></div><details><summary>실행할 내용 확인</summary><pre>{JSON.stringify(approval.args,null,2)}</pre></details><div className="room-approval-actions"><Button size="sm" variant="ghost" disabled={Boolean(approvalBusy)} onClick={()=>void approve(approval.id,false)}>거절</Button><Button size="sm" variant="primary" disabled={Boolean(approvalBusy)} onClick={()=>void approve(approval.id,true)}>{approvalBusy===approval.id?'처리 중':'이번 실행 허용'}</Button></div></div>)}</div>}
    {error&&<Notice message={error}/>}
    <form className="room-composer" onSubmit={send}>
     <textarea ref={inputRef} aria-label="대화방 메시지" placeholder={mode==='mentioned'?t('rooms.mentionPlaceholder'):t('rooms.messagePlaceholder')} value={input} onChange={e=>setInput(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'&&!e.shiftKey&&!e.nativeEvent.isComposing){e.preventDefault();void send()}}} rows={2} disabled={sending}/>
     <div className="room-composer-bottom"><span><AtSign size={14}/>{targets.length?t('rooms.responders',{count:targets.length}):mode==='mentioned'?t('rooms.pickResponders'):t('rooms.autoResponders')}</span><div>{running&&<Button size="sm" variant="ghost" className="room-stop" disabled={stopping} onClick={()=>void stop()}><Square size={12} fill="currentColor"/>{stopping?t('rooms.stopping'):t('rooms.stop')}</Button>}<Button type="submit" variant="primary" size="icon" className="room-send" aria-label="메시지 보내기" disabled={sending||!input.trim()||!current.agentIds.length}>{sending?<LoaderCircle size={18} className="animate-spin"/>:<ArrowUp size={19}/>}</Button></div></div>
    </form>
    <div className="room-mention-options" aria-label="이번 메시지에 응답할 참가자">{roster.map((agent,index)=><button key={agent.id} className={'room-mention-chip '+(targets.includes(agent.id)?'is-selected':'')} aria-pressed={targets.includes(agent.id)} onClick={()=>mention(agent)} title={agent.name+' 응답 선택'}><span style={{backgroundColor:agentColor(agent,index)}}/>@{agent.name}{targets.includes(agent.id)&&<Check size={11}/>}</button>)}</div>
    <div className="room-composer-foot"><span>{running?'창을 닫아도 Rust 서버에서 계속 실행합니다.':'Enter 보내기 · Shift + Enter 줄바꿈'}</span><div>{learningActive>0&&<span><LoaderCircle size={11} className="animate-spin"/>{t('rooms.learning')} {learningActive}</span>}{memoryCount>0&&<span title="완료된 학습 작업에 기록된 생성 수"><BookOpenText size={11}/>{t('rooms.memory',{count:memoryCount})}</span>}<button onClick={()=>setShowActivity(v=>!v)} aria-expanded={showActivity}><Activity size={12}/>{t('rooms.activity')}{feed.jobs.length>0&&' '+feed.jobs.length}</button></div></div>
   </div>
   {showActivity&&<RoomActivity jobs={feed.jobs} executions={feed.executions} learningJobs={feed.learningJobs} recordedOperations={feed.operations} sessionId={sessionId} openSession={openSession} onClose={()=>setShowActivity(false)}/>}
  </section>
  {inspector&&<><button className="room-inspector-scrim" aria-label="참여자 설정 닫기" onClick={()=>setInspector(false)}/><aside className="room-inspector" aria-label="대화방 참여자와 설정"><div className="room-inspector-title"><div><span className="room-eyebrow">IN THIS ROOM</span><h2>우리의 대화방</h2></div><Button size="icon" variant="ghost" aria-label="설정 닫기" onClick={()=>setInspector(false)}><X size={17}/></Button></div><div className="room-owner-card"><Avatar owner/><div><strong>{current.ownerName||'나'}</strong><small>방의 주인 · 대화에 함께 참여</small></div><span>나</span></div><div className="room-inspector-scroll"><RoomSettings value={draft} onChange={updateDraft} agents={agents} disabled={saving}/>{saved&&<p className="room-saved"><Check size={13}/> 설정을 저장했습니다.</p>}<button className="room-delete" onClick={()=>void remove()}><Trash2 size={13}/> 대화방 삭제</button></div><div className="room-inspector-footer"><Button variant="primary" disabled={!dirty||saving||!draft.name.trim()||!draft.agentIds.length} onClick={()=>void save()}>{saving?<LoaderCircle size={14} className="animate-spin"/>:<Check size={14}/>} 변경 저장</Button></div></aside></>}
  {knowledgeOpen&&<DialogBackdrop onDismiss={()=>setKnowledgeOpen(false)}><DialogPanel size="wide" aria-label="공유 지식"><div className="room-dialog-header"><div><span className="room-eyebrow">SHARED ROOM KNOWLEDGE</span><h2>{current.name}의 공유 지식</h2></div><Button size="icon" variant="ghost" aria-label="닫기" onClick={()=>setKnowledgeOpen(false)}><X size={18}/></Button></div><div className="room-dialog-body"><p className="room-note">{t('rooms.knowledgeDesc')}</p><div className="room-knowledge-list">{knowledgeBusy&&!knowledge.length&&<p className="room-note"><LoaderCircle size={14} className="animate-spin"/> 공유 지식을 불러오는 중…</p>}{!knowledgeBusy&&!knowledge.length&&<p className="room-note">{t('rooms.knowledgeEmpty')}</p>}{knowledge.map(item=>{const sources=(item.sources||[]).map((source:any)=>[source.agentName||source.agentId||'?',source.model||'모델 미기록'].filter(Boolean).join(' · ')).filter(Boolean).join(', ');return <div className={'room-knowledge-item '+(item.status==='superseded'?'is-superseded':'')} key={item.id}><div className="room-knowledge-head"><strong>{item.type}</strong><span>{item.verification||'unverified'}</span><span>{item.status==='active'?'활성':'대체됨'}</span></div><p>{item.text}</p><small>출처: {sources||'기록 없음'}</small></div>})}</div>{knowledgeNotice&&<p className="room-saved"><Check size={13}/>{knowledgeNotice}</p>}</div><div className="room-dialog-footer"><span>{t('rooms.selectedKnowledge')}</span><div className="flex gap-2"><Button variant="secondary" disabled={knowledgeBusy} onClick={()=>void learnKnowledge()}>{knowledgeBusy?<LoaderCircle size={14} className="animate-spin"/>:<RefreshCw size={14}/>} 지금 학습</Button><Button onClick={()=>setKnowledgeOpen(false)}>{t('rooms.close')}</Button></div></div></DialogPanel></DialogBackdrop>}
 </div>
}

function TranscriptMessage({message,agent,name,owner,index}:{message:ChatMessage;agent?:Agent;name:string;owner:boolean;index:number}){
 const [copied,setCopied]=useState(false)
 return <article className={'room-message '+(owner?'room-message-owner':'room-message-agent')}><Avatar agent={agent} owner={owner} index={index}/><div className="room-message-main"><div className="room-message-byline"><strong>{name}</strong>{owner?<span className="room-author-tag">나</span>:<span className="room-agent-tag">에이전트</span>}<time>{stamp(message.created_at)}</time><button className="room-copy" aria-label="발언 복사" title="발언 복사" onClick={()=>{void navigator.clipboard.writeText(message.content).then(()=>{setCopied(true);setTimeout(()=>setCopied(false),1600)})}}>{copied?<Check size={12}/>:<Copy size={12}/>}</button></div>{message.reasoning&&<details className="room-reasoning"><summary>추론 보기</summary><pre>{message.reasoning}</pre></details>}<MessageBody text={message.content}/>{message.tool_calls?.length?<span className="room-message-tools"><TerminalSquare size={12}/>도구 호출 {message.tool_calls.length}회 · 실행 기록에서 확인</span>:null}</div></article>
}
function MessageBody({text}:{text:string}){
 const blocks=text.split(/```(?:[^\n]*)\n([\s\S]*?)```/g)
 return <div className="room-message-text">{blocks.map((block,index)=>index%2===1?<pre key={index}><code>{block}</code></pre>:<span key={index}>{block}</span>)}</div>
}

function RoomActivity({jobs,executions,learningJobs,recordedOperations,sessionId,openSession,onClose}:{jobs:Job[];executions:Execution[];learningJobs:Job[];recordedOperations:Operation[];sessionId?:string;openSession:(id:string)=>void;onClose:()=>void}){
 const [selected,setSelected]=useState(''),[operations,setOperations]=useState<Operation[]>([]),[error,setError]=useState(''),[loading,setLoading]=useState(false)
 useEffect(()=>{let valid=true;setOperations([]);setError('');if(!selected)return;setLoading(true);get<Operation[]>('/api/executions/'+selected+'/operations').then(rows=>{if(valid)setOperations(rows)}).catch(e=>{if(valid)setError(e.message)}).finally(()=>{if(valid)setLoading(false)});return()=>{valid=false}},[selected])
 return <div className="room-activity"><div className="room-activity-title"><span><Activity size={15}/> 이 대화방의 실행 기록</span><div>{sessionId&&<Button size="sm" variant="ghost" onClick={()=>openSession(sessionId)}>원본 세션 <ArrowRight size={12}/></Button>}<Button size="icon" variant="ghost" aria-label="실행 기록 닫기" onClick={onClose}><X size={15}/></Button></div></div><div className="room-activity-scroll">{!jobs.length&&!executions.length&&<p className="room-note">아직 실행 기록이 없습니다.</p>}{jobs.slice(0,8).map(job=><div key={job.id} className="room-job-row"><span className={'room-job-status '+(active(job.status)?'is-running':'')}>{labels[job.status]||job.status}</span><span>{job.stepIndex||0}/{job.stepTotal||0} 발언</span><time>{stamp(job.createdAt)}</time>{job.error&&<p>{job.error}</p>}</div>)}{executions.length>0&&<div className="room-execution-buttons">{executions.slice(0,12).map(execution=><button key={execution.id} className={selected===execution.id?'is-selected':''} onClick={()=>setSelected(selected===execution.id?'':execution.id)}><TerminalSquare size={12}/>{labels[execution.status]||execution.status} · 도구 {execution.toolCalls||0}회 · {execution.tokens||0}토큰</button>)}</div>}{loading&&<span className="room-note">도구 실행 근거를 불러오는 중…</span>}{selected&&!loading&&!operations.length&&!error&&<p className="room-note">이 실행에서 기록된 도구 호출이 없습니다.</p>}{(selected?operations:recordedOperations.slice(0,10)).map(operation=><details className="room-operation" key={operation.id}><summary><TerminalSquare size={12}/>{operation.tool||operation.toolName||operation.name||'도구'}<span>{operation.status||'기록'}</span></summary><pre>{JSON.stringify(operation,null,2)}</pre></details>)}{learningJobs.length>0&&<div className="room-learning-log"><BookOpenText size={13}/><span>학습 작업 {learningJobs.length}개 · 완료 {learningJobs.filter(job=>job.status==='completed').length}개</span></div>}<Notice message={error}/></div></div>
}
