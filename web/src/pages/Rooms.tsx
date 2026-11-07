import {useTranscriptScroll} from '../hooks/useTranscriptScroll'
import {useCallback,useEffect,useMemo,useRef,useState,type FormEvent} from 'react'
import {Activity,ArrowDown,ArrowRight,ArrowUp,AtSign,BookOpenText,Bot,Check,ChevronDown,ChevronLeft,ChevronRight,Clock3,Copy,Hash,LoaderCircle,MessageCircle,MoreHorizontal,Plus,RefreshCw,Settings2,ShieldCheck,Square,TerminalSquare,Trash2,UserRound,UsersRound,X} from 'lucide-react'
import {get,post,del,resolveApproval,koreanReasoningEnabled,type Agent,type ChatMessage} from '../api'
import {Button,TextArea,TextInput,SelectInput} from '../components/Primitives'
import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {FormField,Notice,refresh,useRemote} from '../components/UI'
import './Rooms.css'
import {t,useLocale,getLocale,type LocaleKey} from '../i18n'
import {translateError} from '../i18n/errors'

const MAX_ROOM_PARTICIPANTS=32

type RoutingMode='discussion'|'round_robin'|'mentioned'
type RoomConfig={id:string;name:string;topic?:string;description?:string;ownerName?:string;agentIds:string[];routingMode?:RoutingMode;rounds?:number;maxTurns?:number;toolsEnabled?:boolean;autoLearning?:boolean;autoModels?:boolean;sessionId?:string;status?:string;activeAgentId?:string;pauseReason?:string;updatedAt?:string}
type RoomDraft={id?:string;name:string;topic:string;ownerName:string;agentIds:string[];routingMode:RoutingMode;rounds:number;maxTurns:number;autoModels:boolean;toolsEnabled:boolean;autoLearning:boolean}
type Job={id:string;kind:string;status:string;stepIndex?:number;stepTotal?:number;error?:string;createdAt?:string;input?:{roomId?:string;sessionId?:string};result?:{memories?:number;status?:string};roomState?:{activeAgentId?:string;inFlight?:boolean}}
type Execution={id:string;status:string;agentId?:string;sessionId?:string;toolCalls?:number;tokens?:number;reason?:string}
type Operation={id:string;runId?:string;tool?:string;toolName?:string;name?:string;status?:string;result?:unknown;error?:string;arguments?:unknown}
type Approval={id:string;tool:string;args?:unknown;status:string}
type FeedState={room?:RoomConfig;sessionId?:string;jobs:Job[];executions:Execution[];learningJobs:Job[];operations:Operation[];approvals:Approval[];activeAgentId?:string;status?:string;queueDepth?:number}
type LiveDraft={turnId:string;agentId:string;text:string;reasoning:string;tool:string;error:string;done:boolean;messageId?:string}
type RoomEvent={roomId:string;agentId:string;turnId?:string;event:{type:string;text?:string;name?:string;message?:string;messageId?:string;terminal?:boolean;memories?:number;skill?:boolean;inserted?:number;updated?:number;promoted?:number}}
const emptyLive=(turnId:string,agentId:string):LiveDraft=>({turnId,agentId,text:'',reasoning:'',tool:'',error:'',done:false})
const initialFeed=():FeedState=>({jobs:[],executions:[],learningJobs:[],operations:[],approvals:[]})
const labels:Record<string,LocaleKey>={queued:'rooms.st.waiting',running:'rooms.st.running',completed:'rooms.st.completed',idle:'rooms.st.idle',paused:'rooms.st.stopped',stopped:'rooms.st.stopped',cancelled:'rooms.st.stopped',failed:'rooms.st.failed',needs_review:'rooms.st.needsReview',interrupted:'rooms.st.interrupted'}
const modes:Record<RoutingMode,{label:LocaleKey;description:LocaleKey}>={discussion:{label:'rooms.mode.discussion.label',description:'rooms.mode.discussion.desc'},round_robin:{label:'rooms.mode.round_robin.label',description:'rooms.mode.round_robin.desc'},mentioned:{label:'rooms.mode.mentioned.label',description:'rooms.mode.mentioned.desc'}}
const palette=['#668373','#9A745F','#6D7FA4','#967AA0','#B18C47','#6C9297','#AB7180','#7A8456','#8B7B69']
const active=(status?:string)=>status==='queued'||status==='running'
const stamp=(value?:string)=>{if(!value)return '';const normalized=value.includes('T')?value:value.replace(' ','T')+'Z';const date=new Date(normalized);return Number.isNaN(date.getTime())?'':date.toLocaleTimeString(getLocale()==='en'?'en-US':'ko-KR',{hour:'2-digit',minute:'2-digit'})}
const draftOf=(room?:RoomConfig):RoomDraft=>({id:room?.id,name:room?.name||'',topic:room?.topic||room?.description||'',ownerName:room?.ownerName||t('rooms.me'),agentIds:room?.agentIds||[],routingMode:room?.routingMode||'discussion',rounds:room?.rounds||3,maxTurns:room?.maxTurns??0,toolsEnabled:room?.toolsEnabled===true,autoLearning:room?.autoLearning!==false,autoModels:room?.autoModels!==false})
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
  {showList&&<button className="room-list-scrim" aria-label={t('rooms.listClose')} onClick={()=>setShowList(false)}/>}
  <aside className={'room-list '+(showList?'room-list-open':'room-list-closed')} aria-label={t('rooms.listAria')}>
   <div className="room-list-title"><div><span className="room-eyebrow">{t('ui.label.yourCircle')}</span><h2>{t('rooms.listTitle')}</h2></div><Button size="icon" variant="ghost" title={t('rooms.new')} aria-label={t('rooms.newAria')} onClick={()=>{setCreating(draftOf());setError('')}}><Plus size={19}/></Button></div>
   <div className="room-list-search"><TextInput aria-label={t('rooms.searchAria')} placeholder={t('rooms.search')} value={query} onChange={e=>setQuery(e.target.value)}/></div>
   <div className="room-list-items">{filtered.map(room=><button key={room.id} className={'room-list-item '+(selected===room.id?'is-selected':'')} onClick={()=>selectRoom(room.id)} aria-current={selected===room.id?'true':undefined}><span className="room-list-icon"><Hash size={18}/></span><span className="room-list-copy"><strong>{room.name}</strong><small>{room.topic||t('rooms.space')}</small><span className="room-list-meta">{t('rooms.me')} + {room.agentIds?.length||0}{t('rooms.people')}{room.status==='running'&&<i title={t('rooms.st.running')}/>}</span></span></button>)}
    {!rooms.loading&&!filtered.length&&<div className="room-list-empty">{query?t('rooms.noSearch'):t('rooms.empty')}</div>}
   </div>
   <div className="room-list-footer"><UsersRound size={16}/><span>{t('rooms.footer')}</span></div>
  </aside>
  {current?<RoomConversation key={current.id} room={current} agents={agents.value} openList={()=>setShowList(v=>!v)} showList={showList} openSession={openSession} onSaved={async()=>{await rooms.reload();refresh()}} onRemoved={async()=>{setSelected('');await rooms.reload()}}/>:<div className="room-welcome"><div className="room-welcome-mark"><UsersRound size={40}/></div><span className="room-eyebrow">{t('ui.label.teamPlace')}</span><h1>{t('rooms.welcome.title')}</h1><p>{t('rooms.welcome.line1')}<br/>{t('rooms.welcome.line2')}<br/>{t('rooms.welcome.line3')}</p><Button variant="primary" onClick={()=>{setCreating(draftOf());setError('')}}><Plus size={16}/> {t('rooms.create')}</Button><button className="room-mobile-list-button" onClick={()=>setShowList(true)}>{t('rooms.viewSaved')}</button></div>}
  {(error||rooms.error||agents.error)&&!creating&&<div className="room-global-error"><Notice message={error||rooms.error||agents.error}/></div>}
  {creating&&<DialogBackdrop onDismiss={()=>{if(!saving)setCreating(null)}}><DialogPanel size="wide" aria-label={t('rooms.newTitle')}><div className="room-dialog-header"><div><span className="room-eyebrow">{t('ui.label.startConversation')}</span><h2>{t('rooms.who')}</h2></div><Button size="icon" variant="ghost" aria-label={t('rooms.close')} onClick={()=>setCreating(null)} disabled={saving}><X size={18}/></Button></div><div className="room-dialog-body"><RoomSettings value={creating} onChange={setCreating} agents={agents.value} disabled={saving}/><Notice message={error}/></div><div className="room-dialog-footer"><span>{t('rooms.createHint')}</span><Button variant="primary" onClick={()=>void createRoom()} disabled={saving||!creating.name.trim()||!creating.agentIds.length}>{saving?<LoaderCircle size={15} className="animate-spin"/>:<Plus size={15}/>} {t('rooms.create')}</Button></div></DialogPanel></DialogBackdrop>}
 </div>
}

function RoomSettings({value,onChange,agents,disabled=false}:{value:RoomDraft;onChange:(value:RoomDraft)=>void;agents:Agent[];disabled?:boolean}){
 useLocale()
 const toggle=(id:string)=>onChange({...value,agentIds:value.agentIds.includes(id)?value.agentIds.filter(item=>item!==id):[...value.agentIds,id]})
 return <div className="room-settings-form">
  <FormField label={t('rooms.name')}><TextInput value={value.name} onChange={e=>onChange({...value,name:e.target.value})} placeholder={t('rooms.namePlaceholder')} disabled={disabled}/></FormField>
  <FormField label={t('rooms.topic')}><TextArea rows={2} value={value.topic} onChange={e=>onChange({...value,topic:e.target.value})} placeholder={t('rooms.topicPlaceholder')} disabled={disabled}/></FormField>
  <FormField label={t('rooms.ownerName')}><TextInput value={value.ownerName} onChange={e=>onChange({...value,ownerName:e.target.value})} placeholder={t('rooms.me')} disabled={disabled}/></FormField>
  <div className="room-roster-heading"><strong>{t('rooms.roster')}</strong><span>{t('rooms.rosterCount',{count:value.agentIds.length,max:MAX_ROOM_PARTICIPANTS})}</span></div>
  <div className="room-roster-options">{agents.map((agent,index)=>{const checked=value.agentIds.includes(agent.id);const unavailable=disabled||(!checked&&value.agentIds.length>=MAX_ROOM_PARTICIPANTS);return <label key={agent.id} className={'room-roster-option '+(checked?'checked':'')}><input type="checkbox" checked={checked} disabled={unavailable} onChange={()=>toggle(agent.id)}/><Avatar agent={agent} size="small" index={index}/><span><strong>{agent.name}</strong><small>{agent.model||t('rooms.defaultModel')}</small></span>{checked&&<Check size={14}/>}</label>})}{!agents.length&&<p className="room-note">{t('rooms.noAgents')}</p>}</div>
  <FormField label={t('rooms.mode')}><SelectInput value={value.routingMode} onChange={e=>onChange({...value,routingMode:e.target.value as RoutingMode})} disabled={disabled}>{Object.entries(modes).map(([key,item])=><option key={key} value={key}>{t(item.label)}</option>)}</SelectInput><small className="room-note">{t(modes[value.routingMode].description)}</small></FormField>
  <div className="room-settings-numbers"><FormField label={t('rooms.rounds')} hint={t('rooms.roundsHint')}><TextInput type="number" min={1} max={100000} value={value.rounds} onChange={e=>onChange({...value,rounds:Math.min(100000,Math.max(1,Number(e.target.value)||1))})} disabled={disabled||value.routingMode==='round_robin'}/></FormField><FormField label={t('rooms.maxTurns')} hint={t('rooms.maxTurnsHint')}><TextInput type="number" min={0} max={100000} value={value.maxTurns} onChange={e=>onChange({...value,maxTurns:Math.min(100000,Math.max(0,Number(e.target.value)||0))})} disabled={disabled}/></FormField></div>
  <label className="room-setting-check"><input type="checkbox" checked={value.autoLearning} onChange={e=>onChange({...value,autoLearning:e.target.checked})} disabled={disabled}/><span><strong>{t('rooms.autoLearn')}</strong><small>{t('rooms.autoLearnDesc')}</small></span></label>
  <label className="room-setting-check"><input type="checkbox" checked={value.autoModels} onChange={e=>onChange({...value,autoModels:e.target.checked})} disabled={disabled}/><span><strong>{t('rooms.autoModels')}</strong><small>{t('rooms.autoModelsDesc')}</small></span></label>
  <label className="room-setting-check"><input type="checkbox" checked={value.toolsEnabled} onChange={e=>onChange({...value,toolsEnabled:e.target.checked})} disabled={disabled}/><span><strong>{t('rooms.tools')}</strong><small>{t('rooms.toolsDesc')}</small></span></label>
 </div>
}

function RoomConversation({room,agents,openList,showList,openSession,onSaved,onRemoved}:{room:RoomConfig;agents:Agent[];openList:()=>void;showList:boolean;openSession:(id:string)=>void;onSaved:()=>Promise<void>;onRemoved:()=>Promise<void>}){
 useLocale()
 const [messages,setMessages]=useState<ChatMessage[]>([]),[feed,setFeed]=useState<FeedState>(initialFeed)
 const [loading,setLoading]=useState(true),[loadError,setLoadError]=useState(''),[error,setError]=useState('')
 const [input,setInput]=useState(''),[targets,setTargets]=useState<string[]>([]),[sending,setSending]=useState(false),[stopping,setStopping]=useState(false)
 const [inspector,setInspector]=useState(()=>window.innerWidth>=1450),[draft,setDraft]=useState<RoomDraft>(()=>draftOf(room)),[dirty,setDirty]=useState(false),[saving,setSaving]=useState(false),[saved,setSaved]=useState(false)
 const [knowledgeOpen,setKnowledgeOpen]=useState(false),[knowledge,setKnowledge]=useState<any[]>([]),[knowledgeBusy,setKnowledgeBusy]=useState(false),[knowledgeNotice,setKnowledgeNotice]=useState('')
 const [approvalBusy,setApprovalBusy]=useState('')
 const [learningNotice,setLearningNotice]=useState('')
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
     return {...base,text:item.type==='text'?base.text+(item.text||''):base.text,reasoning:item.type==='reasoning'?base.reasoning+(item.text||''):base.reasoning,tool:item.type==='tool_start'?(item.name||t('rooms.toolFallback')):['tool_end','tool_error'].includes(item.type)?'':base.tool}
    })
   }
   if(item.type==='learning'){
    setLearningNotice(t('rooms.event.memory',{count:Number(item.memories||0)}))
    window.setTimeout(()=>setLearningNotice(''),5000)
    return
   }
   if(item.type==='knowledge'){
    const gained=Number(item.inserted||0)+Number(item.updated||0)
    if(gained>0){
     setLearningNotice(t('rooms.event.knowledge',{count:gained}))
     window.setTimeout(()=>setLearningNotice(''),5000)
    }
    return
   }
   if(item.type==='error'){
    terminalTurnIds.current.add(turnId)
    setLive(previous=>({...previous?.turnId===turnId?previous:emptyLive(turnId,detail.agentId),done:true,tool:'',error:item.message||t('chat.tool.error',{error:''})}));update()
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
 const remove=async()=>{if(!confirm(t('rooms.deleteConfirm',{name:current.name})))return;setError('');try{await del('/api/rooms/'+room.id);await onRemoved()}catch(e){setError((e as Error).message)}}
 const loadKnowledge=async()=>{setKnowledgeBusy(true);try{setKnowledge(await get<any[]>('/api/rooms/'+room.id+'/knowledge'))}catch(e){setError((e as Error).message)}finally{setKnowledgeBusy(false)}}
 const openKnowledge=async()=>{setKnowledgeOpen(true);setKnowledgeNotice('');await loadKnowledge()}
 const learnKnowledge=async()=>{if(knowledgeBusy)return;setKnowledgeBusy(true);setKnowledgeNotice('');try{await post('/api/rooms/'+room.id+'/knowledge/learn',{});setKnowledgeNotice(t('rooms.knowledgeQueued'));setTimeout(()=>void loadKnowledge(),2500)}catch(e){setError((e as Error).message)}finally{setKnowledgeBusy(false)}}
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
  <section className="room-main" aria-label={t('rooms.headerAria',{name:current.name})}>
   <header className="room-header"><button className="room-list-toggle" aria-label={showList?t('rooms.listCollapse'):t('rooms.listExpand')} onClick={openList}><MessageCircle size={18}/></button><div className="room-header-icon"><Hash size={22}/></div><div className="room-header-copy"><h1>{current.name}</h1><p>{current.topic||t('rooms.topicFallback')}</p></div><Button size="sm" variant="ghost" className="room-roster-toggle" onClick={()=>void openKnowledge()} aria-haspopup="dialog"><BookOpenText size={16}/><span>{t('rooms.sharedKnowledge')}</span><b>{knowledge.length||0}</b></Button><Button size="sm" variant="ghost" className="room-roster-toggle" onClick={()=>setInspector(v=>!v)} aria-expanded={inspector}><UsersRound size={16}/><span>{t('rooms.participants')}</span><b>{current.agentIds.length+1}</b></Button></header>
   <div className="room-presence"><div className="room-avatar-stack"><Avatar owner size="small"/>{roster.slice(0,6).map((agent,index)=><Avatar key={agent.id} agent={agent} size="small" index={index}/>)}</div><span><strong>{current.ownerName||t('rooms.me')}</strong>{t('rooms.withAgents',{count:roster.length})}</span><span className="room-presence-dot">·</span><span>{t(modes[mode].label)}</span><span className={'room-status '+(running?'is-running':'')}>{running&&<i/>}{labels[status]?t(labels[status]):status}</span></div>
   {(current.toolsEnabled||latestOperation||pendingApprovals.length>0)&&<div className="room-work-status"><span><ShieldCheck size={12}/>{current.toolsEnabled?t('rooms.toolsOn'):t('rooms.toolsOff')}</span>{pendingApprovals.length>0?<button onClick={()=>approvalRef.current?.focus({preventScroll:true})}>{t('rooms.approvalsWaiting',{count:pendingApprovals.length})}</button>:latestOperation?<button onClick={()=>setShowActivity(true)}><TerminalSquare size={11}/>{latestOperation.tool||latestOperation.toolName||t('rooms.toolFallback')} · {labels[latestOperation.status||'']?t(labels[latestOperation.status||'']):latestOperation.status||t('rooms.recordFallback')}</button>:<span>{t('rooms.toolsNote')}</span>}</div>}
   <div className="room-scroll" ref={scrollRef} onScroll={onScroll} role="log" aria-label={t('rooms.logAria')} aria-busy={loading}>
    {loading?<div className="room-feed-loading"><LoaderCircle size={20} className="animate-spin"/> {t('rooms.loading')}</div>:!visible.length?<div className="room-first-message"><div className="room-empty-avatars"><Avatar owner/>{roster.slice(0,3).map((agent,index)=><Avatar key={agent.id} agent={agent} index={index}/>)}</div><h2>{t('rooms.firstTitle')}</h2><p>{t('rooms.firstDesc')}<br/>{t('rooms.firstDesc2')}</p><div><span><AtSign size={14}/> {t('rooms.mentionOne')}</span><span><BookOpenText size={14}/> {t('rooms.roomContinues')}</span></div></div>:<><div className="room-transcript-start"><span>{t('rooms.records')}</span></div>{visible.map(message=>{const owner=message.role==='user';const agent=agents.find(item=>item.id===message.agent_id);return <TranscriptMessage key={message.id} message={message} agent={agent} name={owner?(current.ownerName||t('rooms.me')):(agent?.name||message.agent_id||t('rooms.agentFallback'))} owner={owner} index={Math.max(0,current.agentIds.indexOf(message.agent_id||''))}/>})}</>}
    {live&&<div className="room-message room-message-agent room-message-live"><Avatar agent={agents.find(agent=>agent.id===live.agentId)}/><div className="room-message-main"><div className="room-message-byline"><strong>{agents.find(agent=>agent.id===live.agentId)?.name||live.agentId}</strong><span className="room-live-label">{!live.done&&<i/>}{live.error?t('rooms.live.stopped'):live.done?t('rooms.live.saving'):t('rooms.live.responding')}</span></div>{live.reasoning&&<details className="room-reasoning"><summary>{t('rooms.live.showReasoning')}</summary><pre>{live.reasoning}</pre></details>}{live.text?<MessageBody text={live.text}/>:!live.error&&<span className="room-typing"><i/><i/><i/></span>}{live.tool&&<div className="room-tool-live"><TerminalSquare size={13}/>{t('rooms.live.toolRunning',{tool:live.tool})}</div>}{live.error&&<p className="room-message-error">{translateError(live.error)}</p>}</div></div>}
    {running&&!live&&<div className="room-message room-message-agent room-message-live"><Avatar agent={speaker}/><div className="room-message-main"><div className="room-message-byline"><strong>{speaker?.name||t('rooms.waitingSpeaker')}</strong><span className="room-live-label">{speaker?t('rooms.preparing'):t('rooms.queuedWait')}</span></div>{speaker&&<span className="room-typing"><i/><i/><i/></span>}</div></div>}
    {loadError&&<Notice message={loadError}/>}
   </div>
   {moreBelow&&<button className="room-new-messages" onClick={jumpDown}><ArrowDown size={14}/> {t('rooms.newMessages')}</button>}
   <div className="room-composer-wrap">
    {(latestJob?.error||current.pauseReason)&&<div className="room-run-notice"><Activity size={14}/><span>{translateError(latestJob?.error||current.pauseReason||'')}</span><button onClick={()=>setShowActivity(true)}>{t('rooms.runLog')} <ChevronRight size={13}/></button></div>}
    {pendingApprovals.length>0&&<div className="room-approvals-stack" ref={approvalRef} tabIndex={-1}>{pendingApprovals.map(approval=><div key={approval.id} className="room-approval" data-room-approval><div><ShieldCheck size={15}/><strong>{approval.tool}</strong><span>{t('rooms.approvalRequest')}</span></div><details><summary>{t('rooms.approvalShow')}</summary><pre>{JSON.stringify(approval.args,null,2)}</pre></details><div className="room-approval-actions"><Button size="sm" variant="ghost" disabled={Boolean(approvalBusy)} onClick={()=>void approve(approval.id,false)}>{t('rooms.deny')}</Button><Button size="sm" variant="primary" disabled={Boolean(approvalBusy)} onClick={()=>void approve(approval.id,true)}>{approvalBusy===approval.id?t('rooms.processing'):t('rooms.approveOnce')}</Button></div></div>)}</div>}
    {error&&<Notice message={error}/>}
    <form className="room-composer" onSubmit={send}>
     <textarea ref={inputRef} aria-label={t('rooms.messageAria')} placeholder={mode==='mentioned'?t('rooms.mentionPlaceholder'):t('rooms.messagePlaceholder')} value={input} onChange={e=>setInput(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'&&!e.shiftKey&&!e.nativeEvent.isComposing){e.preventDefault();void send()}}} rows={2} disabled={sending}/>
     <div className="room-composer-bottom"><span><AtSign size={14}/>{targets.length?t('rooms.responders',{count:targets.length}):mode==='mentioned'?t('rooms.pickResponders'):t('rooms.autoResponders')}</span><div>{running&&<Button size="sm" variant="ghost" className="room-stop" disabled={stopping} onClick={()=>void stop()}><Square size={12} fill="currentColor"/>{stopping?t('rooms.stopping'):t('rooms.stop')}</Button>}<Button type="submit" variant="primary" size="icon" className="room-send" aria-label={t('rooms.sendAria')} disabled={sending||!input.trim()||!current.agentIds.length}>{sending?<LoaderCircle size={18} className="animate-spin"/>:<ArrowUp size={19}/>}</Button></div></div>
    </form>
    <div className="room-mention-options" aria-label={t('rooms.mentionOptions')}>{roster.map((agent,index)=><button key={agent.id} className={'room-mention-chip '+(targets.includes(agent.id)?'is-selected':'')} aria-pressed={targets.includes(agent.id)} onClick={()=>mention(agent)} title={t('rooms.mentionTitle',{name:agent.name})}><span style={{backgroundColor:agentColor(agent,index)}}/>@{agent.name}{targets.includes(agent.id)&&<Check size={11}/>}</button>)}</div>
    <div className="room-composer-foot"><span>{running?t('rooms.continuesNotice'):t('rooms.enterHint')}</span><div>{learningNotice&&<span className="rounded bg-[#F7E8E1] px-1.5 py-0.5 font-semibold text-[#A94E30]">✦ {learningNotice}</span>}{learningActive>0&&<span><LoaderCircle size={11} className="animate-spin"/>{t('rooms.learning')} {learningActive}</span>}{memoryCount>0&&<span title={t('rooms.memoryTitle')}><BookOpenText size={11}/>{t('rooms.memory',{count:memoryCount})}</span>}<button onClick={()=>setShowActivity(v=>!v)} aria-expanded={showActivity}><Activity size={12}/>{t('rooms.activity')}{feed.jobs.length>0&&' '+feed.jobs.length}</button></div></div>
   </div>
   {showActivity&&<RoomActivity jobs={feed.jobs} executions={feed.executions} learningJobs={feed.learningJobs} recordedOperations={feed.operations} sessionId={sessionId} openSession={openSession} onClose={()=>setShowActivity(false)}/>}
  </section>
  {inspector&&<><button className="room-inspector-scrim" aria-label={t('rooms.inspectorClose')} onClick={()=>setInspector(false)}/><aside className="room-inspector" aria-label={t('rooms.inspectorAria')}><div className="room-inspector-title"><div><span className="room-eyebrow">{t('ui.label.inThisRoom')}</span><h2>{t('rooms.inspectorTitle')}</h2></div><Button size="icon" variant="ghost" aria-label={t('rooms.settingsClose')} onClick={()=>setInspector(false)}><X size={17}/></Button></div><div className="room-owner-card"><Avatar owner/><div><strong>{current.ownerName||t('rooms.me')}</strong><small>{t('rooms.ownerTag')}</small></div><span>{t('rooms.me')}</span></div><div className="room-inspector-scroll"><RoomSettings value={draft} onChange={updateDraft} agents={agents} disabled={saving}/>{saved&&<p className="room-saved"><Check size={13}/> {t('rooms.settingsSaved')}</p>}<button className="room-delete" onClick={()=>void remove()}><Trash2 size={13}/> {t('rooms.deleteRoom')}</button></div><div className="room-inspector-footer"><Button variant="primary" disabled={!dirty||saving||!draft.name.trim()||!draft.agentIds.length} onClick={()=>void save()}>{saving?<LoaderCircle size={14} className="animate-spin"/>:<Check size={14}/>} {t('rooms.saveChanges')}</Button></div></aside></>}
  {knowledgeOpen&&<DialogBackdrop onDismiss={()=>setKnowledgeOpen(false)}><DialogPanel size="wide" aria-label={t('rooms.sharedKnowledge')}><div className="room-dialog-header"><div><span className="room-eyebrow">{t('ui.label.sharedKnowledge')}</span><h2>{t('rooms.knowledgeOf',{name:current.name})}</h2></div><Button size="icon" variant="ghost" aria-label={t('rooms.close')} onClick={()=>setKnowledgeOpen(false)}><X size={18}/></Button></div><div className="room-dialog-body"><p className="room-note">{t('rooms.knowledgeDesc')}</p><div className="room-knowledge-list">{knowledgeBusy&&!knowledge.length&&<p className="room-note"><LoaderCircle size={14} className="animate-spin"/> {t('rooms.knowledgeLoading')}</p>}{!knowledgeBusy&&!knowledge.length&&<p className="room-note">{t('rooms.knowledgeEmpty')}</p>}{knowledge.map(item=>{const sources=(item.sources||[]).map((source:any)=>[source.agentName||source.agentId||'?',source.model||t('rooms.modelUnrecorded')].filter(Boolean).join(' · ')).filter(Boolean).join(', ');return <div className={'room-knowledge-item '+(item.status==='superseded'?'is-superseded':'')} key={item.id}><div className="room-knowledge-head"><strong>{item.type}</strong><span>{item.verification||'unverified'}</span><span>{item.status==='active'?t('rooms.knowledgeActive'):t('rooms.knowledgeSuperseded')}</span></div><p>{item.text}</p><small>{t('rooms.source')}: {sources||t('rooms.noRecord')}</small></div>})}</div>{knowledgeNotice&&<p className="room-saved"><Check size={13}/>{knowledgeNotice}</p>}</div><div className="room-dialog-footer"><span>{t('rooms.selectedKnowledge')}</span><div className="flex gap-2"><Button variant="secondary" disabled={knowledgeBusy} onClick={()=>void learnKnowledge()}>{knowledgeBusy?<LoaderCircle size={14} className="animate-spin"/>:<RefreshCw size={14}/>} {t('rooms.learnNow')}</Button><Button onClick={()=>setKnowledgeOpen(false)}>{t('rooms.close')}</Button></div></div></DialogPanel></DialogBackdrop>}
 </div>
}

function TranscriptMessage({message,agent,name,owner,index}:{message:ChatMessage;agent?:Agent;name:string;owner:boolean;index:number}){
 useLocale()
 const [copied,setCopied]=useState(false)
 return <article className={'room-message '+(owner?'room-message-owner':'room-message-agent')}><Avatar agent={agent} owner={owner} index={index}/><div className="room-message-main"><div className="room-message-byline"><strong>{name}</strong>{owner?<span className="room-author-tag">{t('rooms.authorMe')}</span>:<span className="room-agent-tag">{t('rooms.authorAgent')}</span>}<time>{stamp(message.created_at)}</time><button className="room-copy" aria-label={t('rooms.copyAria')} title={t('rooms.copyAria')} onClick={()=>{void navigator.clipboard.writeText(message.content).then(()=>{setCopied(true);setTimeout(()=>setCopied(false),1600)})}}>{copied?<Check size={12}/>:<Copy size={12}/>}</button></div>{message.reasoning&&<details className="room-reasoning"><summary>{t('rooms.live.showReasoning')}</summary><pre>{message.reasoning}</pre></details>}<MessageBody text={message.content}/>{message.tool_calls?.length?<span className="room-message-tools"><TerminalSquare size={12}/>{t('rooms.toolCallsSummary',{count:message.tool_calls.length})}</span>:null}</div></article>
}
function MessageBody({text}:{text:string}){
 const blocks=text.split(/```(?:[^\n]*)\n([\s\S]*?)```/g)
 return <div className="room-message-text">{blocks.map((block,index)=>index%2===1?<pre key={index}><code>{block}</code></pre>:<span key={index}>{block}</span>)}</div>
}

function RoomActivity({jobs,executions,learningJobs,recordedOperations,sessionId,openSession,onClose}:{jobs:Job[];executions:Execution[];learningJobs:Job[];recordedOperations:Operation[];sessionId?:string;openSession:(id:string)=>void;onClose:()=>void}){
 useLocale()
 const [selected,setSelected]=useState(''),[operations,setOperations]=useState<Operation[]>([]),[error,setError]=useState(''),[loading,setLoading]=useState(false)
 useEffect(()=>{let valid=true;setOperations([]);setError('');if(!selected)return;setLoading(true);get<Operation[]>('/api/executions/'+selected+'/operations').then(rows=>{if(valid)setOperations(rows)}).catch(e=>{if(valid)setError(e.message)}).finally(()=>{if(valid)setLoading(false)});return()=>{valid=false}},[selected])
 return <div className="room-activity"><div className="room-activity-title"><span><Activity size={15}/> {t('rooms.activityTitle')}</span><div>{sessionId&&<Button size="sm" variant="ghost" onClick={()=>openSession(sessionId)}>{t('rooms.originalSession')} <ArrowRight size={12}/></Button>}<Button size="icon" variant="ghost" aria-label={t('rooms.activityClose')} onClick={onClose}><X size={15}/></Button></div></div><div className="room-activity-scroll">{!jobs.length&&!executions.length&&<p className="room-note">{t('rooms.noActivity')}</p>}{jobs.slice(0,8).map(job=><div key={job.id} className="room-job-row"><span className={'room-job-status '+(active(job.status)?'is-running':'')}>{labels[job.status]?t(labels[job.status]):job.status}</span><span>{t('rooms.turns',{current:job.stepIndex||0,total:job.stepTotal||0})}</span><time>{stamp(job.createdAt)}</time>{job.error&&<p>{translateError(job.error)}</p>}</div>)}{executions.length>0&&<div className="room-execution-buttons">{executions.slice(0,12).map(execution=><button key={execution.id} className={selected===execution.id?'is-selected':''} onClick={()=>setSelected(selected===execution.id?'':execution.id)}><TerminalSquare size={12}/>{t('rooms.executionButton',{status:labels[execution.status]?t(labels[execution.status]):execution.status,tools:execution.toolCalls||0,tokens:execution.tokens||0})}</button>)}</div>}{loading&&<span className="room-note">{t('rooms.loadingEvidence')}</span>}{selected&&!loading&&!operations.length&&!error&&<p className="room-note">{t('rooms.noToolCalls')}</p>}{(selected?operations:recordedOperations.slice(0,10)).map(operation=><details className="room-operation" key={operation.id}><summary><TerminalSquare size={12}/>{operation.tool||operation.toolName||operation.name||t('rooms.toolFallback')}<span>{operation.status?t(labels[operation.status]||'rooms.recordFallback'):t('rooms.recordFallback')}</span></summary><pre>{JSON.stringify(operation,null,2)}</pre></details>)}{learningJobs.length>0&&<div className="room-learning-log"><BookOpenText size={13}/><span>{t('rooms.learningJobs',{total:learningJobs.length,done:learningJobs.filter(job=>job.status==='completed').length})}</span></div>}<Notice message={error}/></div></div>
}
