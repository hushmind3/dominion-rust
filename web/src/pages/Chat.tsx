import {createPortal} from 'react-dom'
import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {FormEvent,useEffect,useRef,useState,type KeyboardEvent} from 'react'
import {ArrowRight,ArrowUp,BrainCircuit,Check,ChevronDown,ChevronRight,CircleHelp,Clock3,Code2,Command,Compass,Copy,Cpu,CornerDownLeft,HardDrive,List,LoaderCircle,MessageCircle,MoreHorizontal,Plus,Power,RefreshCw,ShieldAlert,ShieldCheck,SlidersHorizontal,Sparkles,TerminalSquare,Trash2,X} from 'lucide-react'
import {Agent,Approval,ChatMessage,ChatSession,Provider,get,post,resolveApproval,streamChat,koreanReasoningEnabled,koreanReasoningKey} from '../api'
import {Action,Empty,Notice,StateTag,refresh,useRemote} from '../components/UI'
import {SlashMenu,slashCandidates,slashCommandLabel,slashCommands,type SlashChoice} from '../components/SlashMenu'
import {ModelPicker} from '../components/ModelPicker'
import {useProviderModels} from '../hooks/useProviderModels'
import {useTranscriptScroll} from '../hooks/useTranscriptScroll'
import {useAnchoredPanel} from '../hooks/useAnchoredPanel'
import {useJob,jobFinished,type JobState} from '../hooks/useJob'
import {t,useLocale,type LocaleKey} from '../i18n'
import {translateError} from '../i18n/errors'

type LearningResult={memories:number;skill:boolean}
type LearningJob=JobState & {result?:LearningResult}
type ModelOverride={scope?:string;providerId:string;model:string}
type InstalledModel={id:string;repo:string;label:string;format:string;bytes:number;source?:string}
type Runtime={status:string;id?:string;name?:string;model?:string;error?:string}
type PermissionMode='request'|'workspace'|'full'
const permissionCopy:Record<PermissionMode,{label:LocaleKey;detail:LocaleKey}>={
 request:{label:'chat.perm.request.label',detail:'chat.perm.request.detail'},
 workspace:{label:'chat.perm.workspace.label',detail:'chat.perm.workspace.detail'},
 full:{label:'chat.perm.full.label',detail:'chat.perm.full.detail'}
}
const formatBytes=(bytes:number)=>{
 const gb=Number(bytes||0)/1024/1024/1024
 return gb>=1?gb.toFixed(1)+' GB':Math.max(0,Math.round(Number(bytes||0)/1024/1024))+' MB'
}
export function ChatPage({sessionId,initialAgent,initialProvider,initialModel,onSession,openModels}:{sessionId?:string;initialAgent?:string;initialProvider?:string;initialModel?:string;onSession:(x:string|undefined)=>void;openModels:()=>void}){
 const loc=useLocale()
 const sessions=useRemote<ChatSession[]>('/api/sessions',[]);
 const agents=useRemote<Agent[]>('/api/agents',[]);
 const providers=useRemote<Provider[]>('/api/providers',[]);
 const current=sessionId;
 const selectedSession=sessions.value.find(session=>session.id===current);
 const transcript=useRemote<ChatMessage[]>(current?'/api/sessions/'+encodeURIComponent(current)+'/messages':null,[]);
 const messages=transcript.value;
 const approvals=useRemote<Approval[]>('/api/approvals',[]);
 const [input,setInput]=useState('');
 const [agentChoice,setAgentChoice]=useState<{scope?:string;id:string}>();
 const [override,setOverride]=useState<ModelOverride>();
 const defaultAgent=agents.value.find(agent=>agent.id==='general')||agents.value[0];
 const agentId=(agentChoice?.scope===current?agentChoice?.id:undefined)??selectedSession?.agentId??(!current?initialAgent:undefined)??defaultAgent?.id??'';
 const activeAgent=agents.value.find(agent=>agent.id===agentId);
 const manual=override&&override.scope===current?override:
  !current&&initialProvider?{providerId:initialProvider,model:initialModel??''}:undefined;
 const enabledProviders=providers.value.filter(provider=>provider.enabled!==false);
 const sessionProvider=enabledProviders.find(provider=>provider.id===selectedSession?.providerId);
 const providerId=manual?.providerId??(activeAgent?.providerId||sessionProvider?.id||enabledProviders[0]?.id||'');
 const selectedProvider=enabledProviders.find(provider=>provider.id===providerId);
 const model=manual?.model||((activeAgent?.providerId===providerId?activeAgent.model:'')||
  (selectedSession?.providerId===providerId&&selectedSession.agentId===agentId?selectedSession.model:'')||selectedProvider?.model||'');
 const catalogue=useProviderModels(providerId);
 const {value:models,error:modelError}=catalogue;
 const setRoute=(providerId:string,model:string)=>setOverride({scope:current,providerId,model});
 useEffect(()=>{if(!current){setOverride(undefined);setAgentChoice(undefined)}},[initialAgent,initialProvider,initialModel]);
 const [turnSession,setTurnSession]=useState<string>();
 const [savedMessageId,setSavedMessageId]=useState<string>();
 const [approvalBusy,setApprovalBusy]=useState(false);
 const [liveApproval,setLiveApproval]=useState<Approval|null>(null);
 const [learningJob,setLearningJob]=useState<{id:string;scope:string}>();
 const job=useJob<LearningJob>(learningJob?'/api/jobs/'+encodeURIComponent(learningJob.id):null);
 const approval=(liveApproval?.sessionId===current?liveApproval:null)||
  approvals.value.find(item=>item.sessionId===current&&item.status==='pending');
 const [error,setError]=useState(''),[busy,setBusy]=useState(false),[live,setLive]=useState(''),[thought,setThought]=useState(''),[thinking,setThinking]=useState(false),[reasoningKo,setReasoningKo]=useState(koreanReasoningEnabled),[showReasoning,setShowReasoning]=useState(true),[showSettings,setShowSettings]=useState(false),[toolsEnabled,setToolsEnabled]=useState(true),[thinkingEnabled,setThinkingEnabled]=useState(true),[reasoningEffort,setReasoningEffort]=useState(''),[autoLearn,setAutoLearn]=useState(true),[showHistory,setShowHistory]=useState(()=>{try{if(window.innerWidth<768)return false;return localStorage.getItem('dominion.chat.sidebar')!=='0'}catch{return true}}),[toolStatus,setToolStatus]=useState('');
 const [permissionMode,setPermissionMode]=useState<PermissionMode>('request'),[permissionOpen,setPermissionOpen]=useState(false),[permissionBusy,setPermissionBusy]=useState(false);
 const [modelMenu,setModelMenu]=useState(false),[installed,setInstalled]=useState<InstalledModel[]>([]),[modelsLoading,setModelsLoading]=useState(false),[runtime,setRuntime]=useState<Runtime>({status:'idle'}),[modelBusy,setModelBusy]=useState(''),[unloading,setUnloading]=useState(false),[learning,setLearning]=useState(false),[menuError,setMenuError]=useState('');
 const [slashOpen,setSlashOpen]=useState(false),[slashActive,setSlashActive]=useState(0),[commandInfo,setCommandInfo]=useState('');
 const inputRef=useRef<HTMLTextAreaElement>(null);
 const stream=useRef<AbortController|null>(null);
 const view=useRef(current);
 view.current=current;
 const viewingTurn=turnSession===current&&(!savedMessageId||!messages.some(message=>message.id===savedMessageId));
 const slashItems=slashOpen?slashCandidates(input,agents.value,models):[];
 const transcriptRevision=JSON.stringify([messages.at(-1)?.id,messages.length,viewingTurn?live.length:0,viewingTurn?thought.length:0,viewingTurn?toolStatus:'',approval?.id]);
 const {scrollRef,onScroll,jumpDown}=useTranscriptScroll(current,transcriptRevision);
 const modelPanel=useAnchoredPanel(modelMenu,()=>setModelMenu(false),380);
 const permissionPanel=useAnchoredPanel(permissionOpen,()=>setPermissionOpen(false),300,'top');
 const settingsPanel=useAnchoredPanel(showSettings,()=>setShowSettings(false),250,'top');
 useEffect(()=>()=>stream.current?.abort(),[]);
 useEffect(()=>{if(window.innerWidth<768)return;try{localStorage.setItem('dominion.chat.sidebar',showHistory?'1':'0')}catch{}},[showHistory]);
 useEffect(()=>{get<{autoApprove?:string[];autoApproveAll?:boolean}>('/api/policy').then(p=>{
  setPermissionMode(p.autoApproveAll?'full':(p.autoApprove||[]).includes('workspace_write')?'workspace':'request')
 }).catch(()=>{})},[]);
 useEffect(()=>{
  const controller=new AbortController();
  let timer:ReturnType<typeof setTimeout>;
  const check=async()=>{
   try{const state=await get<Runtime>('/api/model-library/runtime',controller.signal);if(!controller.signal.aborted)setRuntime(state)}
   catch{if(!controller.signal.aborted)setRuntime({status:'unknown'})}
   if(!controller.signal.aborted&&providerId==='dominion-native')timer=setTimeout(check,4000)
  };
  void check();
  return()=>{controller.abort();clearTimeout(timer)};
 },[providerId]);
 const changePermission=async(mode:PermissionMode)=>{
  if(permissionBusy)return
  if(mode==='full'&&!window.confirm(t('chat.perm.fullConfirm')))return
  setPermissionBusy(true);setError('')
  try{
   await post('/api/policy',mode==='full'?{id:'default',autoApprove:[],autoApproveAll:true,autoApproveMcp:true}
    :mode==='workspace'?{id:'default',autoApprove:['workspace_write'],autoApproveAll:false,autoApproveMcp:false}
    :{id:'default',autoApprove:[],autoApproveAll:false,autoApproveMcp:false})
   setPermissionMode(mode);setPermissionOpen(false)
   setCommandInfo(t('chat.perm.changed',{label:t(permissionCopy[mode].label)}))
  }catch(e){setError((e as Error).message)}finally{setPermissionBusy(false)}
 }
 const loadModelMenu=async()=>{
  setMenuError('');setModelsLoading(true)
  try{
   const [list,state]=await Promise.all([get<InstalledModel[]>('/api/model-library/installed'),get<Runtime>('/api/model-library/runtime')])
   setInstalled(list);setRuntime(state)
  }catch(e){setMenuError((e as Error).message)}finally{setModelsLoading(false)}
 }
 const openModelMenu=()=>{
  const next=!modelMenu;setModelMenu(next);setPermissionOpen(false);setShowSettings(false)
  if(next)void loadModelMenu()
 }
 const switchLocalModel=async(m:InstalledModel)=>{
  if(modelBusy)return
  setModelBusy(m.id);setMenuError('')
  try{
   const state=await get<Runtime>('/api/model-library/runtime');
   const ready=state.status==='ready'&&state.id===m.id?state:
    await post<Runtime>('/api/model-library/runtime/load',{id:m.id,context:4096});
   setRuntime(ready);
   if(ready.status!=='ready'||ready.id!==m.id||!ready.model)throw Error(ready.error||t('chat.model.loadFailed'));
   await providers.reload();
   if(providerId==='dominion-native')void catalogue.reload();
   setRoute('dominion-native',ready.model);
   setCommandInfo(t('chat.model.switched',{repo:m.repo,label:m.label}))
   setModelMenu(false)
  }catch(e){setMenuError((e as Error).message)}finally{setModelBusy('')}
 }
 const unloadModel=async()=>{
  if(unloading)return
  if(!window.confirm(t('chat.model.unloadConfirm')))return
  setUnloading(true);setError('')
  try{
   await post('/api/model-library/runtime/unload',{})
   setRuntime({status:'idle'});if(manual?.providerId==='dominion-native')setOverride(undefined)
   await providers.reload();setCommandInfo(t('chat.model.unloaded'))
  }catch(e){setError((e as Error).message)}finally{setUnloading(false)}
 }
 const reportLearning=(result:LearningResult)=>setCommandInfo(
  t('chat.learn.done',{memories:result.memories,skills:result.skill?t('chat.learn.skillSuggested'):t('chat.learn.none')})+
  '\n'+t('chat.learn.hint'));
 useEffect(()=>{
  if(!learningJob||!jobFinished(job.value?.status))return;
  if(learningJob.scope===current){
   if(job.value?.status==='completed'&&job.value.result)reportLearning(job.value.result);
   else setError(job.value?.error||t('chat.learn.incomplete'));
  }
  setLearningJob(undefined);setLearning(false);refresh();
 },[job.value,learningJob,current]);
 const learnSession=async()=>{
  if(!current||learning||selectedSession?.roomId)return;
  setLearning(true);setError('');setCommandInfo('');
  let queued=false;
  try{
   const response=await post<{jobId?:string;result?:LearningResult;message?:string}>('/api/sessions/'+current+'/learn',{});
   if(response.jobId){queued=true;setLearningJob({id:response.jobId,scope:current});return;}
   if(view.current===current){
    if(response.message)setCommandInfo(response.message);
    else if(response.result)reportLearning(response.result);
   }
  }catch(e){if(view.current===current)setError((e as Error).message)}
  finally{if(!queued)setLearning(false)}
 };
 const chooseAgent=(id:string)=>{setAgentChoice({scope:current,id});setOverride(undefined)};
 const pickSession=(s:ChatSession)=>{
  onSession(s.id);setAgentChoice(undefined);setOverride(undefined);
  setError('');setCommandInfo('');setSlashOpen(false);
  if(window.innerWidth<768)setShowHistory(false);
 };
 const resetLocal=()=>{
  onSession(undefined);setAgentChoice(undefined);setOverride(undefined);setInput('');
  setError('');setCommandInfo('');setSlashOpen(false);
  if(window.innerWidth<768)setShowHistory(false);
  inputRef.current?.focus({preventScroll:true});
 };
 const executeCommand=async(text:string)=>{
  const found=text.trim().match(/^\/([a-zA-Z]+)(?:\s+([\s\S]*))?$/);
  if(!found){setError(t('chat.cmd.invalid'));return;}
  const name=found[1].toLowerCase(),arg=(found[2]||'').trim();
  if(!slashCommands.some(x=>x.key===name)){setError(t('chat.cmd.unknown',{name}));return;}
  if(['name','goal','model','agent'].includes(name)&&!arg){
   setInput('/'+name+' ');setSlashOpen(name==='model'||name==='agent');inputRef.current?.focus({preventScroll:true});return;
  }
  setError('');setCommandInfo('');setInput('');setSlashOpen(false);
  try{
   if(name==='new'){resetLocal();setCommandInfo(t('chat.cmd.new'));return;}
   if(name==='history'){setShowHistory(true);await sessions.reload();setCommandInfo(t('chat.cmd.history'));return;}
   if(name==='model'){
    const found=models.find(x=>x.id===arg);if(!found)throw Error(t('chat.cmd.modelMissing',{model:arg}));
    setRoute(providerId,found.id);setCommandInfo(t('chat.cmd.modelSwitched',{model:found.id}));return;
   }
   if(name==='agent'){
    const found=agents.value.find(x=>x.id===arg);if(!found)throw Error(t('chat.cmd.agentMissing',{agent:arg}));
    chooseAgent(found.id);setCommandInfo(t('chat.cmd.agentSwitched',{agent:found.name}));return;
   }
   if(name==='think'){
    const enabled=arg==='on'?true:arg==='off'?false:!thinking;
    setThinking(enabled);setCommandInfo(t('chat.cmd.thinking',{state:enabled?t('chat.cmd.on'):t('chat.cmd.off')}));return;
   }
   if(name==='help'){setCommandInfo(slashCommands.map(x=>x.command+' · '+slashCommandLabel(x,loc).name).join('\n'));return;}
   if(name==='stop'){
    if(!current)throw Error(t('chat.cmd.noSessionStop'));
    const answer=await post<{cancelled:boolean}>('/api/sessions/'+current+'/cancel',{});
    setCommandInfo(answer.cancelled?t('chat.cmd.stopRequested'):t('chat.cmd.nothingRunning'));return;
   }
   if(name==='status'){
    const state=await get<{agents:number;providers:number;jobs:number;executions:number}>('/api/status');
    setCommandInfo(t('chat.cmd.status',{agents:state.agents,providers:state.providers,executions:state.executions,jobs:state.jobs}));return;
   }
   if(name==='tools'){
    const found=await get<{name:string}[]>('/api/tools');
    setCommandInfo(t('chat.cmd.tools',{count:found.filter(x=>x.name).length,list:found.filter(x=>x.name).map(x=>x.name).join(' · ')}));return;
   }
   if(name==='goal'){
    const goal=await post<{id:string}>('/api/goals',{goal:arg,executor:{agentId,providerId,model}});
    setCommandInfo(t('chat.cmd.goal',{id:goal.id.slice(0,8)}));return;
   }
   if(['mcp','skills','memory','workflows','agents','settings'].includes(name)){
    const route=name==='mcp'||name==='skills'?'extensions':name;
    window.location.hash='#/'+route;return;
   }
   if(!current)throw Error(t('chat.cmd.noSession'));
   if(name==='name'){
    await post('/api/sessions/'+current+'/rename',{title:arg});await sessions.reload();
    setCommandInfo(t('chat.cmd.renamed',{name:arg}));return;
   }
   if(name==='clear'){
    if(!window.confirm(t('chat.cmd.confirmClear')))return;
    await post('/api/sessions/'+current+'/clear',{});await transcript.reload();
    setCommandInfo(t('chat.cmd.cleared'));return;
   }
   if(name==='fork'){
    const copied=await post<ChatSession>('/api/sessions/'+current+'/fork',{});
    pickSession(copied);await sessions.reload();
    setCommandInfo(t('chat.cmd.forked',{title:copied.title}));return;
   }
   if(name==='compact'){
    if(!model||!providerId)throw Error(t('chat.cmd.needModel'));
    const result=await post<{summarized:number;remaining:number}>('/api/sessions/'+current+'/compact',{providerId,model});
    await transcript.reload();
    setCommandInfo(t('chat.cmd.compacted',{summarized:result.summarized,remaining:result.remaining}));return;
   }
   if(name==='export'){
    const exported=await get<ChatMessage[]>('/api/sessions/'+current+'/messages');
    const blob=new Blob([JSON.stringify({sessionId:current,messages:exported},null,2)],{type:'application/json'});
    const url=URL.createObjectURL(blob),link=document.createElement('a');link.href=url;link.download='dominion-session-'+current+'.json';
    link.click();URL.revokeObjectURL(url);setCommandInfo(t('chat.cmd.exported'));return;
   }
  }catch(e){setError((e as Error).message);}
 };
 const chooseSlash=(item:SlashChoice)=>{
  setSlashActive(0);
  if(item.submenu||item.requiresArg){
   setInput(item.command+' ');setSlashOpen(Boolean(item.submenu));inputRef.current?.focus({preventScroll:true});return;
  }
  setSlashOpen(false);void executeCommand(item.command);
 };
 const send=async(e?:FormEvent)=>{
  e?.preventDefault();const text=input.trim();if(!text)return;
  if(text.startsWith('/')){
   if(busy&&text!=='/stop')return;
   await executeCommand(text);return;
  }
  if(stream.current||selectedSession?.roomId||(current&&sessions.loading))return;
  if(providerId&&!selectedProvider){setError(t('chat.err.agentApi'));return;}
  if(!providerId){setError(t('chat.err.connectApi'));return;}
  if(!model){setError(t('chat.err.pickModel'));return;}
  const controller=new AbortController();
  stream.current=controller;
  setTurnSession(current);setSavedMessageId(undefined);
  setBusy(true);setError('');setCommandInfo('');setLive('');setThought('');setSlashOpen(false);setToolStatus('');
  jumpDown();
  let id=current;
  let saved=false;
  try{
   if(!id){
    const session=await post<ChatSession>('/api/sessions',{title:t('chat.sessionTitle'),agentId,providerId:manual?.providerId,model:manual?.model});
    if(controller.signal.aborted)return;
    id=session.id;
    if(view.current===current){
     onSession(id);setTurnSession(id);
     if(agentId)setAgentChoice({id:agentId,scope:id});
     if(manual)setOverride({...manual,scope:id});
    }
    void sessions.reload();
   }
   setInput(previous=>previous.trim()===text?'':previous);
   if(inputRef.current)inputRef.current.style.height='';
   await streamChat({
    sessionId:id,agentId,providerId:manual?.providerId,model:manual?.model,input:text,
    toolsEnabled,toolsExecutionEnabled:toolsEnabled,autoLearning:autoLearn,
    reasoningLanguage:reasoningKo?'ko':undefined,reasoningEffort:reasoningEffort||undefined,thinking:thinkingEnabled,
   },event=>{
    if(event.type==='assistant_saved'){saved=true;setSavedMessageId(event.messageId);refresh();}
    if(event.type==='text')setLive(old=>old+event.text);
    if(event.type==='reasoning')setThought(old=>old+event.text);
    if(event.type==='approval'){setLiveApproval({id:event.id,tool:event.tool,args:event.args,sessionId:id!,status:'pending'});void approvals.reload();}
    if(event.type==='tool_start'){setLiveApproval(null);setToolStatus(t('chat.tool.running',{name:event.name}));}
    if(event.type==='tool_end')setToolStatus(t('chat.tool.done',{name:event.name}));
    if(event.type==='tool_error')setToolStatus(t('chat.tool.error',{error:event.error}));
    if(event.type==='goal_progress')setToolStatus(t('chat.tool.goal',{event:event.event,status:event.status}));
    if(event.type==='error')throw Error(event.message);
   },controller.signal);
  }catch(e){
   if(!controller.signal.aborted&&view.current===id)setError((e as Error).message);
  }finally{
   stream.current=null;
   if(!controller.signal.aborted){
    refresh();
    setBusy(false);setLiveApproval(null);setToolStatus('');
    if(!saved){setLive('');setThought('');}
   }
  }
 };
 const approve=async(ok:boolean)=>{
  if(!approval||approvalBusy)return;
  setApprovalBusy(true);
  try{await resolveApproval(approval.id,ok);setLiveApproval(null)}
  catch(e){setError((e as Error).message)}
  finally{await approvals.reload();setApprovalBusy(false)}
 };
 const appendKey=(e:KeyboardEvent<HTMLTextAreaElement>)=>{
  if(e.nativeEvent.isComposing)return;
  if(slashOpen){
   if(e.key==='Escape'){e.preventDefault();setSlashOpen(false);return;}
   if(e.key==='ArrowDown'){e.preventDefault();setSlashActive(x=>slashItems.length?(x+1)%slashItems.length:0);return;}
   if(e.key==='ArrowUp'){e.preventDefault();setSlashActive(x=>slashItems.length?(x+slashItems.length-1)%slashItems.length:0);return;}
   if((e.key==='Enter'||e.key==='Tab')&&slashItems.length){
    e.preventDefault();chooseSlash(slashItems[Math.min(slashActive,slashItems.length-1)]);return;
   }
  }
  if(e.key==='Enter'&&!e.shiftKey){e.preventDefault();void send();}
 };
 const modelState=(()=>{
  if(busy&&viewingTurn)return {label:t('chat.state.processing'),confirmed:false};
  if(!providerId||!model)return {label:t('chat.state.modelNeeded'),confirmed:false};
  if(!selectedProvider)return {label:t('chat.state.noApi'),confirmed:false};
  if(providerId==='dominion-native'){
   if(runtime.status==='ready'&&runtime.model===model&&!modelError&&models.some(item=>item.id===model))return {label:t('chat.state.localRunning'),confirmed:true};
   return {label:runtime.status==='error'?t('chat.state.localError'):t('chat.state.localUnknown'),confirmed:false};
  }
  if(catalogue.loading)return {label:t('chat.state.checkingList'),confirmed:false};
  if(modelError)return {label:t('chat.state.listError'),confirmed:false};
  return {label:models.some(item=>item.id===model)?t('chat.state.listed'):t('chat.state.idRegistered'),confirmed:false};
 })();
 const activePermission=permissionCopy[permissionMode];
 return <div className={'relative grid h-[calc(100dvh-64px)] min-h-0 grid-cols-1 grid-rows-[minmax(0,1fr)] bg-[#F6F3EC] '+(showHistory?'md:grid-cols-[232px_minmax(0,1fr)]':'')}>
  {showHistory&&<Button type="button" aria-label={t('chat.listCollapse')} className="absolute inset-0 z-20 !h-auto !w-auto cursor-default !rounded-none !border-0 !bg-slate-900/30 md:hidden" onClick={()=>setShowHistory(false)}/>}
  <aside className={'flex min-h-0 flex-col overflow-hidden border-r border-[#E7E0D5] bg-[#FAF7F1] '+(showHistory?'max-md:absolute max-md:inset-y-0 max-md:left-0 max-md:z-30 max-md:w-[232px] max-md:shadow-2xl':'hidden')}>
   <div className="flex items-center justify-between gap-3 px-4 pb-3 pt-5"><div className="min-w-0"><span className="block text-[10px] font-extrabold uppercase tracking-widest text-[#C96442]">CONVERSATIONS</span><strong className="mt-1 block text-sm text-[#3A342D]">{t('chat.recent')}</strong></div><Button size="icon" variant="ghost" onClick={resetLocal} title={t('chat.newChat')}><Plus size={19}/></Button></div>
   <Button className="mx-3 mb-4 inline-flex items-center justify-center gap-2 rounded-xl border border-[#EED9CC] bg-[#F7E8E1] px-4 py-2.5 text-xs font-bold text-[#A94E30] hover:bg-[#F1DCCF]" onClick={resetLocal}><Plus size={17}/> {t('chat.newChat')}</Button>
   <div className="min-h-0 flex-1 overflow-y-auto px-2">{sessions.value.filter(s=>!s.roomId).map(s=><Button onClick={()=>pickSession(s)} key={s.id} title={s.title} className={'flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left text-xs text-[#7C7469] hover:bg-[#F7E8E1] '+(current===s.id?'bg-[#F7E8E1] font-bold text-[#A94E30]':'')}><MessageCircle size={15} className="shrink-0"/><span className="truncate">{s.title}</span></Button>)}{!sessions.value.length&&<div className="p-5 text-center text-xs text-[#9C9388]">{t('chat.first')}</div>}</div>
   <div className="border-t border-[#E7E0D5] p-4 text-[11px] text-[#7C7469]"><span><Clock3 size={14}/> {t('chat.storedLocally')}</span></div>
  </aside>
  <div className="flex min-h-0 min-w-0 flex-col">
   <header className="relative flex h-16 shrink-0 items-center justify-between gap-2 border-b border-[#E7E0D5] bg-[#FFFDF9] px-4">
    <div className="flex min-w-0 flex-1 items-center gap-1.5">
      <Button size="icon" variant="ghost" onClick={()=>setShowHistory(x=>!x)} title={showHistory?t('chat.listCollapse'):t('chat.listExpand')} aria-label={showHistory?t('chat.listCollapse'):t('chat.listExpand')}><List size={18}/></Button>
     <div className="relative flex min-w-0 items-center gap-1">
      <SelectInput aria-label={t('chat.selectProvider')} title={selectedProvider?.name||t('chat.autoProvider')} value={manual?.providerId??''} onChange={e=>{if(e.target.value)setRoute(e.target.value,'');else{setOverride(undefined);if(initialProvider)onSession(undefined)}}} className="!w-[124px] !border-0 !bg-transparent !py-1 !pr-6 !text-[12px]"><option value="">{t('chat.autoProvider')}</option>{providers.value.filter(p=>p.enabled).map(p=><option value={p.id} key={p.id}>{p.name}</option>)}</SelectInput>
      <span className="text-[#B8AFA2]">/</span>
      <ModelPicker providerId={providerId} value={model} catalogue={catalogue} onChange={value=>setRoute(providerId,value)} placeholder={providerId?t('chat.modelPick'):t('chat.connectApi')} className="!h-7 !w-[220px] !border-0 !bg-transparent !py-1 !pl-1.5 !pr-2 !text-[11px] !font-semibold" ariaLabel={t('chat.modelPick')}/>
      <Button variant="ghost" size="sm" ref={modelPanel.trigger} onClick={openModelMenu} title={t('chat.localModel')} className="ml-auto !border !border-[#E7E0D5] !bg-white !text-[#5C544A] hover:!bg-[#FAF7F1]" aria-expanded={modelMenu}><HardDrive size={14}/><span className="hidden lg:inline">{t('chat.localModel')}</span><ChevronDown size={12}/></Button>
      <Button variant="ghost" size="sm" title={t('chat.unload')} onClick={()=>void unloadModel()} disabled={unloading||runtime.status!=='ready'} className="!border !border-[#E7E0D5] !bg-white !text-[#5C544A] hover:!bg-[#FAF7F1] disabled:opacity-40"><Power size={14}/><span className="hidden xl:inline">{unloading?t('chat.unloading'):t('chat.unload')}</span></Button>
      {modelMenu&&modelPanel.position&&createPortal(
       <div ref={modelPanel.panel} style={modelPanel.position} className="z-[80] overflow-y-auto overscroll-contain rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] shadow-[0_18px_50px_rgba(36,33,29,.14)]">
        <div className="flex items-center justify-between gap-2 border-b border-[#EFE9DF] px-3 py-2.5"><div className="min-w-0"><strong className="text-[12px] text-[#3A342D]">{t('chat.localModel')}</strong><p className="truncate text-[10px] text-[#9C9388]">{runtime.status==='ready'?t('chat.running')+' · '+(runtime.name||''):runtime.status==='loading'?t('chat.loadingModel'):t('chat.noLocal')}</p></div><Button size="icon" variant="ghost" title={t('models.refresh')} onClick={()=>void loadModelMenu()}><RefreshCw size={15}/></Button></div>
        {menuError&&<div className="border-b border-rose-100 bg-rose-50 px-3 py-2 text-[11px] text-rose-700">{menuError}</div>}
        <div className="max-h-[320px] overflow-y-auto p-1.5">{modelsLoading&&<div className="flex items-center justify-center gap-2 p-5 text-[11px] text-[#9C9388]"><LoaderCircle size={14} className="animate-spin"/> {t('chat.loadingModel')}</div>}{!modelsLoading&&installed.map(m=><Button type="button" variant="ghost" key={m.id} disabled={Boolean(modelBusy)} onClick={()=>void switchLocalModel(m)} className={'flex w-full items-center gap-3 !justify-start !border-0 rounded-lg px-2.5 py-2.5 text-left hover:bg-[#FAF7F1] disabled:opacity-60 '+(runtime.status==='ready'&&runtime.id===m.id?'bg-emerald-50/60':'')}>
         <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-[#F3EEE5] text-[#7C7469]"><HardDrive size={15}/></span>
         <span className="min-w-0 flex-1"><strong className="block truncate text-[12px] text-[#3A342D]">{m.repo}</strong><small className="text-[10px] text-[#9C9388]">{m.label} · {m.format.toUpperCase()} · {formatBytes(m.bytes)}</small></span>
         {runtime.status==='ready'&&runtime.id===m.id?<span className="shrink-0 rounded bg-emerald-100 px-2 py-1 text-[10px] font-semibold text-emerald-700">{t('chat.inUse')}</span>:modelBusy===m.id?<span className="shrink-0 text-[10px] font-semibold text-[#C96442]">{t('chat.loadingShort')}</span>:<span className="shrink-0 text-[10px] font-semibold text-[#C96442]">{t('chat.runShort')}</span>}
        </Button>)}{!modelsLoading&&!installed.length&&<div className="p-5 text-center text-[11px] text-[#9C9388]">{t('chat.noInstalled')}</div>}</div>
        <Button type="button" variant="ghost" onClick={()=>{setModelMenu(false);openModels()}} className="flex w-full items-center justify-between !rounded-none border-t border-[#EFE9DF] px-3 py-2.5 text-[11px] font-semibold text-[#5C544A] hover:bg-[#FAF7F1]">{t('chat.openModels')} <ArrowRight size={13}/></Button>
       </div>
      ,document.body)}
     </div>
    </div>
    <div className="flex shrink-0 items-center gap-1">
     <span role="status" className={'hidden items-center gap-2 text-[11px] font-semibold sm:inline-flex '+(modelState.confirmed?'text-emerald-700':'text-[#7C7469]')}><i className={'size-1.5 rounded-full '+(modelState.confirmed?'bg-emerald-500':'bg-slate-400')}/>{modelState.label}</span>
     
     <Button size="icon" variant="ghost" title={t('chat.modelManage')} onClick={openModels}><Cpu size={18}/></Button>
    </div>
   </header>
   <div ref={scrollRef} onScroll={onScroll} data-chat-scroll className="min-h-0 flex-1 overflow-y-auto overscroll-contain">{!messages.length&&!(busy&&viewingTurn)?<div className="mx-auto flex min-h-full max-w-xl flex-col justify-center px-5 py-8">
    <div className="mb-6 flex items-center gap-3"><span className="grid size-11 place-items-center rounded-2xl bg-[#F7E8E1] text-[#C96442]"><Sparkles size={22}/></span><div><h1 className="text-[24px] font-bold tracking-[-.03em] text-[#24211D]">{t('chat.newChatHeading')}</h1><p className="mt-1 text-[13px] text-[#8A8176]">{t('chat.newChatDesc')}</p></div></div>
    <div className="rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] p-5"><div className="mb-2 text-[11px] font-semibold text-[#8A8176]">{t('chat.examples')}</div>{[t('chat.example1'),t('chat.example2'),t('chat.example3')].map(example=><Button type="button" key={example} onClick={()=>{setInput(example);inputRef.current?.focus({preventScroll:true})}} className="flex w-full items-center justify-between border-t border-[#EFE9DF] px-2 py-3 text-left text-[12px] text-[#5C544A] hover:bg-[#FFFDF9]">{example}<ArrowRight size={14} className="shrink-0 text-[#9C9388]"/></Button>)}</div>
    {!model&&<Button type="button" onClick={openModels} className="mt-4 inline-flex items-center gap-2 self-start text-[12px] font-semibold text-[#C96442]"><Compass size={15}/> {t('chat.connectFirst')} <ArrowRight size={14}/></Button>}
   </div>:<div className="mx-auto flex min-h-full w-full max-w-[860px] flex-col gap-6 px-6 py-7">{messages.filter(m=>m.role!=='tool').map(m=><Message key={m.id} message={m} agentName={agents.value.find(a=>a.id===m.agent_id)?.name} showReasoning={showReasoning} />)}{viewingTurn&&(live||thought||busy)&&<div className="flex min-w-0 items-start gap-3 assistant"><div className="grid size-9 shrink-0 place-items-center rounded-xl bg-[#F3EEE5] text-[#4A443C] ai"><Sparkles size={17}/></div><div className="relative min-w-0 flex-1"><div className="mb-2 flex items-center gap-2 text-xs font-bold text-[#4A443C]">DOMINION <span>LIVE</span>{busy&&thought&&!showReasoning&&<span className="inline-flex items-center gap-1 rounded bg-[#F3EEE5] px-1.5 py-0.5 text-[10px] font-medium text-[#7C7469]"><BrainCircuit size={11}/> {t('chat.thinkingNow')}</span>}</div>{showReasoning&&thought&&<details className="mb-3 max-h-56 overflow-y-auto rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-[11px] leading-5 text-[#7C7469]" open={thinking}><summary className="cursor-pointer select-none" onClick={e=>{e.preventDefault();setThinking(x=>!x)}}><BrainCircuit size={14} className="inline"/> {t('chat.reasoningProcess',{count:thought.length.toLocaleString()} )} <ChevronRight size={13} className="inline"/></summary><div className="mt-2 whitespace-pre-wrap">{thought}</div></details>}{live&&<div className="min-w-0 break-words text-sm leading-7 text-[#3A342D] [&_p]:my-2 [&_pre]:overflow-x-auto [&_pre]:rounded-xl [&_pre]:bg-slate-950 [&_pre]:p-4 [&_pre]:text-slate-100 [&_code]:break-words [&_ul]:list-disc [&_ul]:pl-6">{live}</div>}{busy&&!live&&<div className="flex items-center gap-2 text-xs text-[#9C9388]"><i/><i/><i/></div>}</div></div>}{viewingTurn&&toolStatus&&<div className="inline-flex items-center gap-2 rounded-lg bg-[#FAF7F1] px-3 py-1.5 text-xs text-[#7C7469]"><TerminalSquare size={15}/>{toolStatus}</div>}</div>}</div>
   <div className="sticky bottom-0 z-10 border-t border-[#EFE9DF] bg-white/95 px-4 py-3 backdrop-blur-sm"><div className="mx-auto w-full max-w-[860px]">{approval&&<div className="mb-3 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-amber-200 bg-amber-50 p-3 text-xs text-amber-900"><div><ShieldAlert size={20}/><span><strong>{t('chat.approval')}</strong><small>{approval.tool} · {JSON.stringify(approval.args??{}).slice(0,150)}</small></span></div><div><Action variant="secondary" disabled={approvalBusy} onClick={()=>approve(false)}>{t('chat.deny')}</Action><Action disabled={approvalBusy} onClick={()=>approve(true)}><Check size={15}/> {t('chat.approve')}</Action></div></div>}
    {(error||transcript.error||job.error)&&<div className="mb-3 rounded-xl border border-rose-200 bg-rose-50 p-3 text-xs text-rose-700"><CircleHelp size={16}/>{error||transcript.error||translateError(job.error||'')}<Button onClick={()=>setError('')}><X size={14}/></Button></div>}
    {modelError&&!model&&<div className="mb-3 text-xs text-[#7C7469]">{t('chat.noModel')}: {modelError} <Button onClick={openModels}>{t('chat.manage')} <ArrowRight size={13}/></Button></div>}
    {commandInfo&&<div role="status" className="mb-3 max-h-36 overflow-auto whitespace-pre-wrap rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-3 text-xs leading-6 text-[#4A443C]">{commandInfo}</div>}
    <div className="relative">
    {slashOpen&&<SlashMenu items={slashItems} active={slashActive} onChoose={chooseSlash}/>}
    <form onSubmit={send} className="overflow-visible rounded-2xl border border-[#E1D9CC] bg-[#FFFDF9] shadow-[0_8px_30px_rgba(36,33,29,.08)] focus-within:border-[#C96442] focus-within:ring-4 focus-within:ring-[#C96442]/10">
     <TextArea ref={inputRef} rows={1} placeholder={t("chat.inputPlaceholder")} value={input} onChange={e=>{const next=e.target.value;setInput(next);setSlashOpen(next.startsWith('/')&&!next.includes('\n'));setSlashActive(0);const el=e.currentTarget;el.style.height='auto';el.style.height=Math.min(el.scrollHeight,180)+'px'}} onKeyDown={appendKey} aria-controls="dominion-slash-menu" aria-expanded={slashOpen} className="!min-h-[64px] !resize-none !rounded-2xl !border-0 !bg-[#FFFDF9] !px-5 !pb-0 !pt-4 !text-[15px] !leading-7 !shadow-none !ring-0 focus:!ring-0"/>
     <div className="flex flex-wrap items-center justify-between gap-2 px-2 pb-2 pt-1">
      <div className="flex flex-wrap items-center gap-1">
       <div className="relative">
        <Button type="button" variant="ghost" ref={permissionPanel.trigger} onClick={()=>{setPermissionOpen(x=>!x);setShowSettings(false);setModelMenu(false)}} className={'inline-flex !h-7 items-center gap-1.5 !rounded-md !border-0 !px-2 !py-0 text-[11px] font-semibold '+(permissionMode==='full'?'!bg-amber-50 text-amber-700 hover:!bg-amber-100':'!bg-[#FAF7F1] text-[#5C544A] hover:!bg-[#F3EEE5]')} title={t(activePermission.detail)} aria-expanded={permissionOpen}>{permissionMode==='full'?<ShieldAlert size={13}/>:<ShieldCheck size={13}/>} {t(activePermission.label)} <ChevronDown size={11}/></Button>
        {permissionOpen&&permissionPanel.position&&createPortal(<div ref={permissionPanel.panel} style={permissionPanel.position} className="z-[80] max-w-full overflow-y-auto overscroll-contain rounded-xl border border-[#E7E0D5] bg-white p-1.5 shadow-xl"><div className="px-2 py-1.5"><strong className="text-[11px] text-[#4A443C]">{t('chat.permissions')}</strong><p className="text-[10px] text-[#9C9388]">{t('chat.permissionsDesc')}</p></div>{(['request','workspace','full'] as PermissionMode[]).map(mode=><Button type="button" variant="ghost" key={mode} disabled={permissionBusy} onClick={()=>void changePermission(mode)} className={'flex w-full items-start gap-2 !justify-start !border-0 rounded-lg px-2 py-2 text-left hover:bg-[#FAF7F1] '+(permissionMode===mode?'bg-[#FAF7F1]':'')}><span className={'mt-0.5 grid size-4 shrink-0 place-items-center rounded-full border '+(permissionMode===mode?'border-[#C96442] bg-[#C96442] text-white':'border-[#D8CFC2]')}>{permissionMode===mode&&<Check size={11}/>}</span><span className="min-w-0 flex-1"><strong className={'block text-[11px] '+(mode==='full'?'text-amber-700':'text-[#4A443C]')}>{t(permissionCopy[mode].label)}</strong><small className="block text-[10px] leading-4 text-[#9C9388]">{t(permissionCopy[mode].detail)}</small></span></Button>)}</div>,document.body)}
       </div>
       <SelectInput aria-label={t('chat.reasoningEffort')} value={reasoningEffort} onChange={e=>setReasoningEffort(e.target.value)} title={t('chat.reasoningEffortTitle')} className="!h-7 !w-auto !border-0 !bg-transparent !py-1 !pl-1.5 !pr-6 !text-[11px] !font-semibold !text-[#5C544A]"><option value="">{t('chat.reasoningAuto')}</option><option value="low">{t('chat.reasoningLow')}</option><option value="medium">{t('chat.reasoningMedium')}</option><option value="high">{t('chat.reasoningHigh')}</option></SelectInput>
       <SelectInput aria-label={t('chat.agentAria')} value={agentId} onChange={e=>chooseAgent(e.target.value)} className="!h-7 !w-auto !max-w-[140px] !border-0 !bg-transparent !py-1 !pl-1.5 !pr-6 !text-[11px] !font-semibold !text-[#5C544A]">{agents.value.map(a=><option key={a.id} value={a.id}>{a.name}</option>)}</SelectInput>
       <div className="relative">
        <Button type="button" variant="ghost" ref={settingsPanel.trigger} onClick={()=>{setShowSettings(x=>!x);setPermissionOpen(false);setModelMenu(false)}} className="inline-flex !h-7 items-center gap-1.5 !rounded-md !border-0 !px-2 !py-0 text-[11px] font-medium text-[#7C7469] hover:bg-[#F3EEE5]" title={t('chat.settings')} aria-expanded={showSettings}><SlidersHorizontal size={13}/><span className="hidden sm:inline">{t('chat.settings')}</span></Button>
        {showSettings&&settingsPanel.position&&createPortal(<div ref={settingsPanel.panel} style={settingsPanel.position} className="z-[80] max-w-full overflow-y-auto overscroll-contain rounded-xl border border-[#E7E0D5] bg-white p-2 shadow-xl"><label className="flex cursor-pointer items-center justify-between gap-2 rounded-lg px-2 py-2 text-[11px] text-[#5C544A] hover:bg-[#FAF7F1]"><span className="inline-flex items-center gap-1.5"><BrainCircuit size={13}/> {t('chat.showReasoning')}</span><TextInput type="checkbox" checked={showReasoning} onChange={e=>setShowReasoning(e.target.checked)}/></label><label className="flex cursor-pointer items-center justify-between gap-2 rounded-lg px-2 py-2 text-[11px] text-[#5C544A] hover:bg-[#FAF7F1]"><span>{t('chat.koreanReasoning')}</span><TextInput type="checkbox" checked={reasoningKo} onChange={e=>{const enabled=e.target.checked;setReasoningKo(enabled);try{localStorage.setItem(koreanReasoningKey,enabled?'1':'0')}catch{}}}/></label><label className="flex cursor-pointer items-center justify-between gap-2 rounded-lg px-2 py-2 text-[11px] text-[#5C544A] hover:bg-[#FAF7F1]"><span>{t('chat.useThinking')}</span><TextInput type="checkbox" checked={thinkingEnabled} onChange={e=>setThinkingEnabled(e.target.checked)}/></label><label className="flex cursor-pointer items-center justify-between gap-2 rounded-lg px-2 py-2 text-[11px] text-[#5C544A] hover:bg-[#FAF7F1]"><span>{t('chat.useTools')}</span><TextInput type="checkbox" checked={toolsEnabled} onChange={e=>setToolsEnabled(e.target.checked)}/></label><label className="flex cursor-pointer items-center justify-between gap-2 rounded-lg px-2 py-2 text-[11px] text-[#5C544A] hover:bg-[#FAF7F1]"><span>{t('chat.autoLearn')} <small className="text-[10px] text-[#9C9388]">{t('chat.autoLearnDesc')}</small></span><TextInput type="checkbox" checked={autoLearn} onChange={e=>setAutoLearn(e.target.checked)}/></label><Button type="button" variant="ghost" disabled={!current||Boolean(selectedSession?.roomId)||learning||busy} onClick={()=>void learnSession()} className="mt-1 flex w-full !justify-start gap-2 !border-0 rounded-lg px-2 py-2 text-[11px] text-[#5C544A] hover:bg-[#FAF7F1] disabled:opacity-40"><BrainCircuit size={13}/>{learning?t('chat.learning'):t('chat.learnNow')}</Button></div>,document.body)}
       </div>
      </div>
      <div className="ml-auto flex items-center gap-2">
       <span className="hidden text-[10px] text-[#9C9388] sm:inline">{t('chat.enterHint')}</span>
       <Button className="grid size-9 shrink-0 place-items-center rounded-full !border-[#C96442] !bg-[#C96442] !text-white hover:!bg-[#A94E30] disabled:opacity-40" disabled={busy||Boolean(selectedSession?.roomId)||(Boolean(current)&&sessions.loading)||!input.trim()} type="submit" title={t('chat.send')}>{busy?<LoaderCircle size={18} className="animate-spin"/>:<ArrowUp size={19}/>}</Button>
      </div>
     </div>
    </form></div><div className="mt-2 text-center text-[10px] text-[#9C9388]">{t('chat.reasoningNote')}</div>
   </div></div>
  </div>
 </div>
}
function Message({message,agentName,showReasoning}:{message:ChatMessage;agentName?:string;showReasoning?:boolean}){
 const [open,setOpen]=useState(true);const human=message.role==='user';
 return <div className={'flex min-w-0 items-start gap-3 '+(human?'':'')} data-message>
  <div className={'grid size-9 shrink-0 place-items-center rounded-xl '+(human?'bg-[#E9E2D7] text-[#4A443C]':'bg-[#F7E8E1] text-[#A94E30]')}>{human?'U':<Sparkles size={17}/>}</div>
  <div className="relative min-w-0 flex-1">
   <div className="mb-2 flex items-center gap-2 text-xs font-bold text-[#4A443C]">{human?'You':agentName||'DOMINION'}</div>
   {showReasoning&&message.reasoning&&<details className="mb-3 max-h-56 overflow-y-auto rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-[11px] leading-5 text-[#7C7469]" open={open}><summary className="cursor-pointer select-none" onClick={e=>{e.preventDefault();setOpen(v=>!v)}}><BrainCircuit size={14} className="inline"/> {t('chat.reasoningProcess',{count:message.reasoning.length.toLocaleString()})} <ChevronRight size={13} className="inline"/></summary><div className="mt-2 whitespace-pre-wrap">{message.reasoning}</div></details>}
   <div className={"min-w-0 break-words text-sm leading-7 text-[#3A342D] [&_p]:my-2 [&_pre]:overflow-x-auto [&_pre]:rounded-xl [&_pre]:bg-slate-950 [&_pre]:p-4 [&_pre]:text-slate-100 [&_code]:break-words [&_ul]:list-disc [&_ul]:pl-6 "+(human?"rounded-xl bg-[#F3EEE5] px-4 py-3":"")}>{message.content||(!human&&message.tool_calls?.length?t('chat.toolRequest'):'' )}</div>
   {!human&&message.content&&<Button className="inline-flex items-center gap-1.5 rounded-lg px-2 py-1 text-[11px] text-[#7C7469] hover:bg-[#F3EEE5]" onClick={()=>navigator.clipboard.writeText(message.content)} title={t('chat.copy')}><Copy size={14}/></Button>}
  </div>
 </div>
}
