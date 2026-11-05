import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {FormEvent,useEffect,useRef,useState,type KeyboardEvent} from 'react'
import {Archive,ArrowRight,ArrowUp,BrainCircuit,Check,ChevronDown,ChevronRight,CircleHelp,Clock3,Code2,Command,Compass,Copy,Cpu,CornerDownLeft,HardDrive,List,LoaderCircle,MessageCircle,MoreHorizontal,Plus,Power,RefreshCw,ShieldAlert,ShieldCheck,SlidersHorizontal,Sparkles,TerminalSquare,Trash2,X} from 'lucide-react'
import {Agent,ChatMessage,ChatSession,Provider,get,post,streamChat} from '../api'
import {Action,Empty,Notice,StateTag,refresh,useRemote} from '../components/UI'
import {SlashMenu,slashCandidates,slashCommands,type SlashChoice} from '../components/SlashMenu'
import {PageToolbar} from '../components/PageToolbar'
import {ConversationComposer,ResponseControls,AttachmentList,type Attachment,type AttachmentBinding,type ReasoningEffort} from '../components/ConversationComposer'
type Model={id:string;name:string}
type LearningResult={memories?:number|unknown[];skill?:boolean;reason?:string}
type LearningJob={id:string;status:string;error?:string;result?:LearningResult}
const learningSummary=(result?:LearningResult)=>{if(!result)return '기억 추출 작업이 완료되었습니다. 메모리 화면에서 저장 결과를 확인하세요.';const count=Array.isArray(result.memories)?result.memories.length:typeof result.memories==='number'?result.memories:null;return '기억 추출 완료'+(count===null?'':' · 새 기억 '+count+'개')+(result.skill?' · 스킬 초안 1개 제안':'')}

type InstalledModel={id:string;repo:string;label:string;format:string;bytes:number;source?:string}
type Runtime={status:string;id?:string;name?:string;error?:string}
type PermissionMode='request'|'workspace'|'full'
const permissionCopy:Record<PermissionMode,{label:string;detail:string}>={
 request:{label:'승인 요청',detail:'파일 수정·명령 실행 전에 항상 확인합니다.'},
 workspace:{label:'파일 자동 승인',detail:'작업공간 파일 쓰기만 자동 승인하고 명령 실행은 확인합니다.'},
 full:{label:'전체 액세스',detail:'파일·명령·MCP 도구를 확인 없이 실행합니다.'}
}
const formatBytes=(bytes:number)=>{
 const gb=Number(bytes||0)/1024/1024/1024
 return gb>=1?gb.toFixed(1)+' GB':Math.max(0,Math.round(Number(bytes||0)/1024/1024))+' MB'
}
const storedChatPreferences=()=>{try{return JSON.parse(localStorage.getItem('dominion.chat.preferences')||'{}')}catch{return {}}}
export function ChatPage({sessionId,onSession,openModels}:{sessionId?:string;onSession:(x:string|undefined)=>void;openModels:()=>void}){
 const sessions=useRemote<ChatSession[]>('/api/sessions',[]);
 const agents=useRemote<Agent[]>('/api/agents',[]);
 const providers=useRemote<Provider[]>('/api/providers',[]);
 const [preferences]=useState(storedChatPreferences)
 const [attachments,setAttachments]=useState<Attachment[]>([]),[showArchived,setShowArchived]=useState(false)
 const [current,setCurrent]=useState<string|undefined>(sessionId);
 const [messages,setMessages]=useState<ChatMessage[]>([]);
 const [input,setInput]=useState(''),[providerId,setProviderId]=useState<string>(preferences.providerId||''),[model,setModel]=useState<string>(preferences.model||''),[agentId,setAgentId]=useState<string>(preferences.agentId||'general'),[models,setModels]=useState<Model[]>([]);
 const [modelError,setModelError]=useState(''),[error,setError]=useState(''),[busy,setBusy]=useState(false),[live,setLive]=useState(''),[thought,setThought]=useState(''),[thinking,setThinking]=useState(false),[reasoningKo,setReasoningKo]=useState(preferences.reasoningKo!==false),[showReasoning,setShowReasoning]=useState(preferences.showReasoning!==false),[showSettings,setShowSettings]=useState(false),[toolsEnabled,setToolsEnabled]=useState(preferences.toolsEnabled!==false),[thinkingEnabled,setThinkingEnabled]=useState(preferences.thinkingEnabled!==false),[reasoningEffort,setReasoningEffort]=useState<ReasoningEffort>(preferences.reasoningEffort||''),[autoLearn,setAutoLearn]=useState(preferences.autoLearn!==false),[showHistory,setShowHistory]=useState(()=>{try{if(window.innerWidth<768)return false;return localStorage.getItem('dominion.chat.sidebar')!=='0'}catch{return true}}),[approval,setApproval]=useState<{id:string;tool:string;args:any}|null>(null),[toolStatus,setToolStatus]=useState('');
 const [permissionMode,setPermissionMode]=useState<PermissionMode>('request'),[permissionOpen,setPermissionOpen]=useState(false),[permissionBusy,setPermissionBusy]=useState(false);
 const [modelMenu,setModelMenu]=useState(false),[installed,setInstalled]=useState<InstalledModel[]>([]),[modelsLoading,setModelsLoading]=useState(false),[runtime,setRuntime]=useState<Runtime>({status:'idle'}),[modelBusy,setModelBusy]=useState(''),[unloading,setUnloading]=useState(false),[learning,setLearning]=useState(false),[menuError,setMenuError]=useState('');
 const [learningJob,setLearningJob]=useState<{id:string;sessionId:string;generation:number}|null>(null)
 const [slashOpen,setSlashOpen]=useState(false),[slashActive,setSlashActive]=useState(0),[commandInfo,setCommandInfo]=useState(''),[learnedNotice,setLearnedNotice]=useState(false);
 const bottom=useRef<HTMLDivElement>(null),inputRef=useRef<HTMLTextAreaElement>(null),currentRef=useRef(current),viewGeneration=useRef(0);
 currentRef.current=current
 useEffect(()=>{try{localStorage.setItem('dominion.chat.preferences',JSON.stringify({providerId,model,agentId,reasoningEffort,thinkingEnabled,reasoningKo,showReasoning,toolsEnabled,autoLearn}))}catch{}},[providerId,model,agentId,reasoningEffort,thinkingEnabled,reasoningKo,showReasoning,toolsEnabled,autoLearn])
 useEffect(()=>{if(agents.loading||agents.value.some(agent=>agent.id===agentId))return;const fallback=agents.value.find(agent=>agent.id==='general')||agents.value[0];if(fallback)setAgentId(fallback.id)},[agents.loading,agents.value,agentId])
 useEffect(()=>{
  if(!learningJob)return
  let mounted=true,inFlight=false
  const sameView=()=>currentRef.current===learningJob.sessionId&&viewGeneration.current===learningJob.generation
  const update=async()=>{
   if(inFlight)return
   inFlight=true
   try{
    const job=await get<LearningJob>('/api/jobs/'+learningJob.id)
    if(!mounted||!sameView())return
    if(['completed','failed','cancelled','needs_review','interrupted'].includes(job.status)){
     if(job.status==='completed')setCommandInfo(learningSummary(job.result))
     else{setCommandInfo('');setError(job.error||'기억 추출 작업이 '+job.status+' 상태입니다. 실행 기록을 확인하세요.')}
     setLearning(false);setLearningJob(null);refresh()
    }else setCommandInfo(job.status==='running'?'기억 추출 작업이 실행 중입니다. 실제 저장 결과를 기다립니다.':'기억 추출 작업이 대기 중입니다.')
   }catch(error){
    if(mounted&&sameView()){setCommandInfo('');setError('기억 추출 작업의 상태를 확인할 수 없습니다. 실행 기록에서 확인하세요. '+(error as Error).message);setLearning(false);setLearningJob(null)}
   }finally{inFlight=false}
  }
  void update();const timer=setInterval(()=>void update(),1500);return()=>{mounted=false;clearInterval(timer)}
 },[learningJob])
 useEffect(()=>{
  const handler=(event:Event)=>{
   const detail=(event as CustomEvent).detail as {sessionId?:string}|undefined;
   if(detail?.sessionId&&detail.sessionId===currentRef.current)setLearnedNotice(true);
  };
  window.addEventListener('dominion-learning-feedback',handler);
  return()=>window.removeEventListener('dominion-learning-feedback',handler);
 },[]);
 const slashItems=slashOpen?slashCandidates(input,agents.value,models):[];
 useEffect(()=>{if(providerId||providers.loading||!providers.value.some(p=>p.enabled))return;let valid=true;get<{providerId:string;model:string}>('/api/model-route').then(x=>{if(valid){setProviderId(x.providerId);setModel(x.model)}}).catch(()=>{});return()=>{valid=false}},[providerId,providers.loading,providers.value.length]);
 useEffect(()=>{if(currentRef.current===sessionId)return;viewGeneration.current+=1;currentRef.current=sessionId;setCurrent(sessionId);setInput('');setAttachments([]);setMessages([]);setLive('');setThought('');setBusy(false);setLearning(false);setLearningJob(null);setApproval(null);setError('');setLearnedNotice(false)},[sessionId]);
 useEffect(()=>{let valid=true;const load=()=>{if(current&&!busy)void get<ChatMessage[]>('/api/sessions/'+current+'/messages').then(rows=>{if(valid&&currentRef.current===current)setMessages(rows)}).catch(()=>{})};if(!current)setMessages([]);else load();let timer:ReturnType<typeof setTimeout>;const update=()=>{clearTimeout(timer);timer=setTimeout(load,220)};window.addEventListener('dominion-refresh',update);return()=>{valid=false;clearTimeout(timer);window.removeEventListener('dominion-refresh',update)}},[current,busy]);
 useEffect(()=>{let valid=true;setModels([]);setModelError('');if(!providerId)return;get<Model[]>('/api/providers/'+providerId+'/models').then(x=>{if(!valid)return;setModels(x);if(x.length)setModel(old=>x.some(m=>m.id===old)?old:x[0].id);else setModel('')}).catch(e=>{if(valid){setModelError(e.message);setModel('')}});return()=>{valid=false}},[providerId]);
 useEffect(()=>{
  const el=document.querySelector('[data-chat-scroll]') as HTMLElement|null;
  if(el){
    const nearBottom=el.scrollHeight-el.scrollTop-el.clientHeight<120;
    if(!nearBottom)return;
  }
  bottom.current?.scrollIntoView({behavior:'auto',block:'end'});
},[messages,live,thought,toolStatus]);
 useEffect(()=>{if(window.innerWidth<768)return;try{localStorage.setItem('dominion.chat.sidebar',showHistory?'1':'0')}catch{}},[showHistory]);
 useEffect(()=>{get<{autoApprove?:string[];autoApproveAll?:boolean}>('/api/policy').then(p=>{
  setPermissionMode(p.autoApproveAll?'full':(p.autoApprove||[]).includes('workspace_write')?'workspace':'request')
 }).catch(()=>{})},[]);
 useEffect(()=>{get<Runtime>('/api/model-library/runtime').then(setRuntime).catch(()=>{})},[]);
 useEffect(()=>{
  if(!permissionOpen&&!modelMenu&&!showSettings)return
  const h=(e:globalThis.KeyboardEvent)=>{if(e.key==='Escape'){setPermissionOpen(false);setModelMenu(false);setShowSettings(false)}}
  window.addEventListener('keydown',h);return()=>window.removeEventListener('keydown',h)
 },[permissionOpen,modelMenu,showSettings]);
 const changePermission=async(mode:PermissionMode)=>{
  if(permissionBusy)return
  if(mode==='full'&&!window.confirm('전체 액세스를 켜면 모델이 승인 없이 파일을 수정하고 명령을 실행합니다. 계속할까요?'))return
  setPermissionBusy(true);setError('')
  try{
   await post('/api/policy',mode==='full'?{id:'default',autoApprove:[],autoApproveAll:true,autoApproveMcp:true}
    :mode==='workspace'?{id:'default',autoApprove:['workspace_write'],autoApproveAll:false,autoApproveMcp:false}
    :{id:'default',autoApprove:[],autoApproveAll:false,autoApproveMcp:false})
   setPermissionMode(mode);setPermissionOpen(false)
   setCommandInfo('실행 권한: '+permissionCopy[mode].label)
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
   const state=await get<Runtime>('/api/model-library/runtime')
   if(!(state.status==='ready'&&state.id===m.id)){
    if(state.status==='ready')await post('/api/model-library/runtime/unload',{})
    await post('/api/model-library/runtime/load',{id:m.id,context:4096,gpu:true})
    let ready=false
    for(let i=0;i<240;i++){
     await new Promise(r=>setTimeout(r,1000))
     const next=await get<Runtime>('/api/model-library/runtime');setRuntime(next)
     if(next.status==='ready'){ready=true;break}
     if(next.status==='error')throw Error(next.error||'모델 실행에 실패했습니다')
    }
    if(!ready)throw Error('모델 로딩 시간이 초과되었습니다')
   }
   await providers.reload()
   const list=await get<Model[]>('/api/providers/dominion-native/models')
   setProviderId('dominion-native');setModels(list);setModel(list[0]?.id||'')
   setCommandInfo('모델 전환 완료: '+m.repo+' / '+m.label)
   setModelMenu(false)
  }catch(e){setMenuError((e as Error).message)}finally{setModelBusy('')}
 }
 const unloadModel=async()=>{
  if(unloading)return
  if(!window.confirm('실행 중인 로컬 모델을 내리고 VRAM을 해제할까요?'))return
  setUnloading(true);setError('')
  try{
   await post('/api/model-library/runtime/unload',{})
   setRuntime({status:'idle'});setProviderId('');setModel('');setModels([])
   await providers.reload();setCommandInfo('로컬 모델을 내렸습니다. VRAM이 해제됩니다.')
  }catch(e){setError((e as Error).message)}finally{setUnloading(false)}
 }
 const learnSession=async()=>{
  if(!current){setError('학습할 대화를 먼저 선택하세요.');return}
  if(learning)return
  const generation=viewGeneration.current,session=current
  setLearning(true);setError('');setCommandInfo('기억 추출 작업을 요청하고 있습니다.')
  try{
   const result=await post<{jobId?:string;status:string;job?:LearningJob;result?:LearningResult;message?:string}>('/api/sessions/'+session+'/learn',{providerId:providerId||undefined,model:model||undefined})
   if(viewGeneration.current!==generation||currentRef.current!==session)return
   if(result.status==='completed'){setCommandInfo(result.message||learningSummary(result.result||result.job?.result));setLearning(false);refresh();return}
   const id=result.jobId||result.job?.id;if(!id)throw Error(result.message||'기억 추출 작업 ID를 확인할 수 없습니다.')
   setLearningJob({id,sessionId:session,generation});setCommandInfo(result.status==='running'?'기억 추출 작업이 실행 중입니다.':'기억 추출 작업을 대기열에 등록했습니다. 저장 결과가 나오면 표시합니다.')
  }catch(error){if(viewGeneration.current===generation){setError((error as Error).message);setLearning(false)}}
 };
 const pickSession=(session:ChatSession)=>{viewGeneration.current+=1;currentRef.current=session.id;setCurrent(session.id);onSession(session.id);setAttachments([]);setInput('');setBusy(false);setLearning(false);setLearningJob(null);setApproval(null);if(session.providerId)setProviderId(session.providerId);if(session.model)setModel(session.model);if(session.agentId)setAgentId(session.agentId);setLive('');setThought('');setError('')};
 const resetLocal=()=>{viewGeneration.current+=1;currentRef.current=undefined;setCurrent(undefined);onSession(undefined);setMessages([]);setAttachments([]);setInput('');setLive('');setThought('');setBusy(false);setLearning(false);setLearningJob(null);setApproval(null);setToolStatus('');setError('');setCommandInfo('');setSlashOpen(false);inputRef.current?.focus()};
 const ensureSession=async()=>{if(currentRef.current)return currentRef.current;const generation=viewGeneration.current;const session=await post<ChatSession>('/api/sessions',{title:'새 대화',agentId,providerId,model});if(generation!==viewGeneration.current)throw Error('대화가 변경되었습니다. 새 대화에서 다시 첨부하세요.');currentRef.current=session.id;setCurrent(session.id);onSession(session.id);void sessions.reload();return session.id};
 const addAttachments=(items:Attachment[],binding:AttachmentBinding)=>{if(currentRef.current===binding.sessionId)setAttachments(previous=>[...previous,...items])};
 const archiveSession=async(session:ChatSession)=>{try{await post('/api/sessions/'+session.id+'/archive',{archived:!session.archived});if(currentRef.current===session.id&&!session.archived)resetLocal();await sessions.reload()}catch(error){setError((error as Error).message)}};
 const stop=async()=>{if(!currentRef.current)return;try{await post('/api/sessions/'+currentRef.current+'/cancel',{})}catch(error){setError((error as Error).message)}};
 const executeCommand=async(text:string)=>{
  const found=text.trim().match(/^\/([a-zA-Z]+)(?:\s+([\s\S]*))?$/);
  if(!found){setError('명령어 형식이 올바르지 않습니다. /help로 확인하세요.');return;}
  const name=found[1].toLowerCase(),arg=(found[2]||'').trim();
  if(!slashCommands.some(x=>x.key===name)){setError('알 수 없는 명령어: /'+name+' · /help로 확인하세요.');return;}
  if(['name','goal','model','agent'].includes(name)&&!arg){
   setInput('/'+name+' ');setSlashOpen(name==='model'||name==='agent');inputRef.current?.focus();return;
  }
  setError('');setCommandInfo('');setInput('');setSlashOpen(false);
  try{
   if(name==='new'){resetLocal();setCommandInfo('새 대화를 시작합니다.');return;}
   if(name==='history'){setShowHistory(true);await sessions.reload();setCommandInfo('왼쪽에 저장된 대화 기록을 표시했습니다.');return;}
   if(name==='model'){
    const found=models.find(x=>x.id===arg);if(!found)throw Error('현재 제공자에 등록되지 않은 모델입니다: '+arg);
    setModel(found.id);setCommandInfo('모델 전환: '+found.id);return;
   }
   if(name==='agent'){
    const found=agents.value.find(x=>x.id===arg);if(!found)throw Error('등록되지 않은 에이전트입니다: '+arg);
    setAgentId(found.id);setCommandInfo('에이전트 전환: '+found.name);return;
   }
   if(name==='think'){
    const enabled=arg==='on'?true:arg==='off'?false:!thinking;
    setThinking(enabled);setCommandInfo('추론 표시: '+(enabled?'켜짐':'꺼짐'));return;
   }
   if(name==='help'){setCommandInfo(slashCommands.map(x=>x.command+' · '+x.name).join('\n'));return;}
   if(name==='stop'){
    if(!current)throw Error('중지할 실행 세션이 없습니다.');
    const answer=await post<{cancelled:boolean}>('/api/sessions/'+current+'/cancel',{});
    setCommandInfo(answer.cancelled?'실행 중단 요청을 전달했습니다.':'현재 실행 중인 작업이 없습니다.');return;
   }
   if(name==='status'){
    const state=await get<{agents:number;providers:number;jobs:number;executions:number}>('/api/status');
    setCommandInfo('에이전트 '+state.agents+'개 · 제공자 '+state.providers+'개 · 실행 '+state.executions+'개 · 대기/진행 작업 '+state.jobs+'개');return;
   }
   if(name==='tools'){
    const found=await get<{name:string}[]>('/api/tools');
    setCommandInfo('사용 가능한 도구 '+found.filter(x=>x.name).length+'개\n'+found.filter(x=>x.name).map(x=>x.name).join(' · '));return;
   }
   if(name==='goal'){
    const goal=await post<{id:string}>('/api/goals',{goal:arg,executor:{agentId,providerId,model}});
    setCommandInfo('자율 작업 등록: '+goal.id.slice(0,8)+' · 실행 기록에서 상태를 확인할 수 있습니다.');return;
   }
   if(['mcp','skills','memory','workflows','agents','settings'].includes(name)){
    const route=name==='mcp'||name==='skills'?'extensions':name;
    window.location.hash='#/'+route;return;
   }
   if(!current)throw Error('현재 세션이 없습니다. /new로 대화를 시작하세요.');
   if(name==='name'){
    await post('/api/sessions/'+current+'/rename',{title:arg});await sessions.reload();
    setCommandInfo('대화 이름 변경: '+arg);return;
   }
   if(name==='clear'){
    if(!window.confirm('현재 세션의 대화 기록을 지우시겠습니까?'))return;
    await post('/api/sessions/'+current+'/clear',{});setMessages([]);setLive('');setThought('');
    setCommandInfo('현재 대화 기록을 초기화했습니다.');return;
   }
   if(name==='fork'){
    const copied=await post<ChatSession>('/api/sessions/'+current+'/fork',{});
    setCurrent(copied.id);onSession(copied.id);await sessions.reload();
    setCommandInfo('새 분기 세션을 만들었습니다: '+copied.title);return;
   }
   if(name==='compact'){
    if(!model||!providerId)throw Error('컨텍스트 압축에 사용할 모델을 연결하세요.');
    const result=await post<{summarized:number;remaining:number}>('/api/sessions/'+current+'/compact',{providerId,model});
    setCommandInfo('컨텍스트 압축 완료 · 요약된 메시지 '+result.summarized+'개 · 최근 메시지 '+result.remaining+'개 유지');return;
   }
   if(name==='export'){
    const exported=await get<ChatMessage[]>('/api/sessions/'+current+'/messages');
    const blob=new Blob([JSON.stringify({sessionId:current,messages:exported},null,2)],{type:'application/json'});
    const url=URL.createObjectURL(blob),link=document.createElement('a');link.href=url;link.download='dominion-session-'+current+'.json';
    link.click();URL.revokeObjectURL(url);setCommandInfo('세션 JSON 파일을 내보냈습니다.');return;
   }
  }catch(e){setError((e as Error).message);}
 };
 const chooseSlash=(item:SlashChoice)=>{
  setSlashActive(0);
  if(item.submenu||item.requiresArg){
   setInput(item.command+' ');setSlashOpen(Boolean(item.submenu));inputRef.current?.focus();return;
  }
  setSlashOpen(false);void executeCommand(item.command);
 };
 const send=async(event?:FormEvent)=>{
  event?.preventDefault();const text=input.trim()||(attachments.length?'첨부 파일을 확인해줘.':'');if(!text)return;
  if(text.startsWith('/')){if(busy&&text!=='/stop')return;await executeCommand(text);return;}
  if(busy)return;
  const generation=viewGeneration.current,submittedAttachments=[...attachments]
  const visible=()=>viewGeneration.current===generation
  setBusy(true);setError('');setCommandInfo('');setLive('');setThought('');setInput('');setAttachments([]);setSlashOpen(false);setToolStatus('');
  if(inputRef.current)inputRef.current.style.height='';
  try{
   const session=await ensureSession();if(!visible())return
   setMessages(old=>[...old,{id:'pending-'+Date.now(),session_id:session,role:'user',content:text,metadata:{attachments:submittedAttachments},created_at:new Date().toISOString()}]);
   await streamChat({sessionId:session,agentId,providerId,model,input:text,attachmentIds:submittedAttachments.map(item=>item.id),toolsEnabled,autoLearning:autoLearn,reasoningLanguage:reasoningKo?'ko':undefined,reasoningEffort:reasoningEffort||undefined,thinking:reasoningEffort==='none'?false:thinkingEnabled},item=>{
    if(!visible()||currentRef.current!==session)return
    if(item.type==='text')setLive(old=>old+item.text);
    if(item.type==='reasoning')setThought(old=>old+item.text);
    if(item.type==='approval')setApproval({id:item.id,tool:item.tool,args:item.args});
    if(item.type==='tool_start')setToolStatus('실행 중 · '+item.name);
    if(item.type==='tool_end')setToolStatus('완료 · '+item.name);
    if(item.type==='tool_error')setToolStatus('도구 오류 · '+item.error);
    if(item.type==='goal_progress')setToolStatus('자율 실행 · '+item.event+' · '+item.status);
    if(item.type==='assistant_saved'){void get<ChatMessage[]>('/api/sessions/'+session+'/messages').then(rows=>{if(visible()&&currentRef.current===session){setMessages(rows);setLive('');setThought('')}})}
    if(item.type==='error')setError(item.message);
   });
   const rows=await get<ChatMessage[]>('/api/sessions/'+session+'/messages');if(visible()&&currentRef.current===session){setMessages(rows);setLive('');setThought('')}void sessions.reload();
  }catch(error){if(visible()){setError((error as Error).message);setAttachments(submittedAttachments)}}finally{if(visible()){setBusy(false);setApproval(null)}}
 };
 const approve=async(allow:boolean)=>{if(!approval)return;try{const result=await post<{resolved:boolean;error?:string}>('/api/approvals',{id:approval.id,allow});if(!result.resolved)throw Error(result.error||'진행 중인 승인 요청이 아닙니다.');setApproval(null)}catch(error){setError((error as Error).message)}};
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
 const selectedProvider=providers.value.find(provider=>provider.id===providerId)
 const selectedSession=sessions.value.find(session=>session.id===current)
 const activePermission=permissionCopy[permissionMode]
 const listedSessions=sessions.value.filter(session=>!session.roomId&&Boolean(session.archived)===showArchived)
 const responseControls=<ResponseControls providers={providers.value} models={models} providerId={providerId} model={model} onProvider={id=>{setProviderId(id);setModel('')}} onModel={setModel} agents={agents.value} agentId={agentId} onAgent={id=>{setAgentId(id);const agent=agents.value.find(item=>item.id===id);if(agent?.providerId){setProviderId(agent.providerId);setModel(agent.model||'')}}} thinking={thinkingEnabled} onThinking={setThinkingEnabled} effort={reasoningEffort} onEffort={setReasoningEffort} disabled={busy}/>
 return <div className={'chat-workspace '+(showHistory?'has-chat-history':'')}>
  <PageToolbar><Button size="icon" variant="ghost" onClick={()=>setShowHistory(value=>!value)} aria-label="대화 목록 펼치기/접기"><List size={18}/></Button><strong className="page-toolbar-title">{selectedSession?.title||(current?'대화':'새 대화')}</strong><div className="page-toolbar-actions">{current&&<><Button size="icon" variant="ghost" aria-label="현재 대화 보관" title="대화 보관" onClick={()=>selectedSession&&void archiveSession(selectedSession)}><Archive size={17}/></Button><Button size="icon" variant="ghost" aria-label="현재 대화 선택 해제" title="대화 닫기 · 기록 유지" onClick={resetLocal}><X size={17}/></Button></>}<Button size="sm" variant="ghost" onClick={openModelMenu}><HardDrive size={16}/><span className="hide-on-small">로컬 모델</span></Button><Button size="icon" variant="ghost" aria-label="대화 설정" onClick={()=>{setShowSettings(value=>!value);setModelMenu(false)}}><SlidersHorizontal size={17}/></Button></div></PageToolbar>
  {showHistory&&<button className="chat-history-scrim" aria-label="대화 목록 닫기" onClick={()=>setShowHistory(false)}/>}
  <aside className={'chat-history '+(showHistory?'':'is-hidden')}>
   <div className="chat-history-heading"><strong>{showArchived?'보관한 대화':'최근 대화'}</strong><Button size="icon" variant="ghost" aria-label="새 대화" onClick={resetLocal}><Plus size={19}/></Button></div>
   <div className="chat-history-list">{listedSessions.map(session=><div className={'chat-history-row '+(current===session.id?'is-selected':'')} key={session.id}><button className="chat-history-select" onClick={()=>pickSession(session)}><MessageCircle size={15}/><span>{session.title||'대화'}</span></button><button className="chat-history-archive" onClick={()=>void archiveSession(session)} aria-label={(session.archived?'보관 해제: ':'보관: ')+session.title} title={session.archived?'보관 해제':'보관'}><Archive size={14}/></button></div>)}{!listedSessions.length&&<p>{showArchived?'보관한 대화가 없습니다.':'새 대화를 시작해보세요.'}</p>}</div>
   <button className="chat-archive-toggle" onClick={()=>setShowArchived(value=>!value)}><Archive size={15}/>{showArchived?'최근 대화로 돌아가기':'보관한 대화 보기'}</button>
  </aside>
  <div className="chat-content">
   {modelMenu&&<div className="chat-floating-panel"><div className="conversation-panel-heading"><strong>로컬 모델</strong><Button size="icon" variant="ghost" aria-label="닫기" onClick={()=>setModelMenu(false)}><X size={17}/></Button></div>{menuError&&<Notice message={menuError}/>}<div className="local-model-options">{installed.map(item=><button key={item.id} disabled={Boolean(modelBusy)} onClick={()=>void switchLocalModel(item)}><HardDrive size={17}/><span><strong>{item.repo}</strong><small>{item.label} · {formatBytes(item.bytes)}</small></span><b>{runtime.id===item.id?'사용 중':modelBusy===item.id?'불러오는 중':'실행'}</b></button>)}{modelsLoading&&<p>모델 목록을 불러오는 중…</p>}{!modelsLoading&&!installed.length&&<p>설치된 로컬 모델이 없습니다.</p>}</div><div className="conversation-panel-actions"><Button size="sm" onClick={openModels}>모델 관리</Button><Button size="sm" disabled={unloading||runtime.status!=='ready'} onClick={()=>void unloadModel()}><Power size={14}/>{unloading?'내리는 중':'모델 내리기'}</Button></div></div>}
   {showSettings&&<div className="chat-floating-panel"><div className="conversation-panel-heading"><strong>대화 설정</strong><Button size="icon" variant="ghost" aria-label="닫기" onClick={()=>setShowSettings(false)}><X size={17}/></Button></div><label className="conversation-setting"><span>추론 과정 펼쳐 보기</span><input type="checkbox" checked={showReasoning} onChange={event=>setShowReasoning(event.target.checked)}/></label><label className="conversation-setting"><span>한국어 추론 요청</span><input type="checkbox" checked={reasoningKo} onChange={event=>setReasoningKo(event.target.checked)}/></label><label className="conversation-setting"><span>도구 사용</span><input type="checkbox" checked={toolsEnabled} onChange={event=>setToolsEnabled(event.target.checked)}/></label><label className="conversation-setting"><span>대화 후 기억 추출</span><input type="checkbox" checked={autoLearn} onChange={event=>setAutoLearn(event.target.checked)}/></label><Button size="sm" disabled={!current||busy||learning} onClick={()=>void learnSession()}><BrainCircuit size={15}/>{learning?'학습 작업 확인 중':'지금 기억 추출'}</Button></div>}
   <div data-chat-scroll className="chat-transcript">{!messages.length&&!busy?<div className="chat-empty"><Sparkles size={25}/><h2>무엇을 함께할까요?</h2><p>질문하거나 컴퓨터에서 실행할 작업을 지시하세요.<br/>파일과 이미지는 아래에 첨부하거나 붙여넣을 수 있습니다.</p>{['현재 작업공간을 살펴보고 상태를 알려줘','프로젝트의 다음 작업을 함께 정리해줘'].map(text=><button key={text} onClick={()=>{setInput(text);inputRef.current?.focus()}}>{text}<ArrowRight size={15}/></button>)}</div>:<div className="chat-transcript-inner">{messages.filter(message=>message.role!=='tool').map(message=><Message key={message.id} message={message} agent={agents.value.find(agent=>agent.id===message.agent_id)} showReasoning={showReasoning}/>)}{learnedNotice&&<div className="chat-tool-status"><BrainCircuit size={15}/>학습 기록이 갱신되었습니다. 기억 화면에서 확인하세요.</div>}{(live||thought||busy)&&<div className="chat-message"><div className="chat-avatar"><Sparkles size={19}/></div><div className="chat-message-main"><div className="chat-message-author">{agents.value.find(agent=>agent.id===agentId)?.name||'DOMINION'} <span>{busy?'응답 중':'저장 중'}</span></div>{thought&&<details className="conversation-reasoning" open={showReasoning&&thinking}><summary onClick={event=>{event.preventDefault();setThinking(value=>!value)}}>추론 과정 · {thought.length.toLocaleString()}자</summary><pre>{thought}</pre></details>}{live&&<div className="chat-message-text">{live}</div>}{busy&&!live&&<div className="chat-working"><LoaderCircle size={15} className="animate-spin"/>{thought?'모델이 추론하고 있습니다.':'응답을 기다리고 있습니다.'}</div>}</div></div>}{toolStatus&&<div className="chat-tool-status"><TerminalSquare size={15}/>{toolStatus}</div>}<div ref={bottom}/></div>}</div>
   <div className="chat-composer-wrap">{selectedSession?.roomId?<div className="conversation-readonly">그룹 대화의 원본 기록입니다.<Button onClick={()=>{try{localStorage.setItem('dominion.room.selected',selectedSession.roomId!)}catch{}window.location.hash='#/rooms'}}>그룹 대화에서 이어가기</Button></div>:<><Notice message={error}/>{commandInfo&&<div role="status" className="conversation-info">{commandInfo}</div>}{modelError&&!model&&<div className="conversation-inline-note">모델 연결 확인: {modelError}</div>}{approval&&<div className="chat-approval"><strong>{approval.tool} 실행 승인 요청</strong><pre>{JSON.stringify(approval.args,null,2)}</pre><div><Button size="sm" onClick={()=>void approve(false)}>거절</Button><Button size="sm" variant="primary" onClick={()=>void approve(true)}>이번 실행 허용</Button></div></div>}<div className="relative">{slashOpen&&<SlashMenu items={slashItems} active={slashActive} onChoose={chooseSlash}/>}<ConversationComposer input={input} onInput={value=>{setInput(value);setSlashOpen(value.startsWith('/')&&!value.includes('\n'));setSlashActive(0)}} onSend={send} inputRef={inputRef} attachments={attachments} onAddAttachments={addAttachments} onRemoveAttachment={id=>setAttachments(previous=>previous.filter(item=>item.id!==id))} resolveBinding={async()=>({sessionId:await ensureSession()})} busy={busy} onStop={stop} onKeyDown={appendKey} placeholder="메시지 입력 · / 명령어 · 이미지 붙여넣기" controls={responseControls} actions={<select className="conversation-permission" aria-label="컴퓨터 실행 권한" title={activePermission.detail} value={permissionMode} disabled={permissionBusy} onChange={event=>void changePermission(event.target.value as PermissionMode)}>{Object.entries(permissionCopy).map(([key,item])=><option value={key} key={key}>{item.label}</option>)}</select>} footer={<span>Enter 보내기 · Shift+Enter 줄바꿈 · 모델이 제공한 추론만 표시</span>}/></div></>}</div>
  </div>
 </div>
}
function Message({message,agent,showReasoning}:{message:ChatMessage;agent?:Agent;showReasoning?:boolean}){
 const human=message.role==='user',attachments=Array.isArray(message.metadata?.attachments)?message.metadata.attachments as Attachment[]:[]
 return <article className={'chat-message '+(human?'is-owner':'')} data-message><div className="chat-avatar">{(agent?.avatar||agent?.avatarUrl)?<img src={String(agent.avatar||agent.avatarUrl)} alt=""/>:human?'나':<Sparkles size={18}/>}</div><div className="chat-message-main"><div className="chat-message-author">{human?'나':agent?.name||'DOMINION'}</div>{message.reasoning&&<details className="conversation-reasoning" open={showReasoning?undefined:false}><summary>추론 과정 · {message.reasoning.length.toLocaleString()}자</summary><pre>{message.reasoning}</pre></details>}<AttachmentList attachments={attachments}/>{message.content&&<div className="chat-message-text">{message.content}</div>}{!human&&message.content&&<button className="chat-message-copy" onClick={()=>void navigator.clipboard.writeText(message.content)} title="발언 복사"><Copy size={14}/>복사</button>}</div></article>
}
