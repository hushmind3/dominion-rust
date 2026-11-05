import {lazy,Suspense,useCallback,useEffect,useState} from 'react'
import {get} from './api'
import {WorkspaceSidebar,WorkspaceHeader,CommandPalette,navigation,type PageId} from './components/WorkspaceShell'
const CapabilitiesPage=lazy(()=>import('./pages/Capabilities').then(module=>({default:module.CapabilitiesPage})))
const MemoryPage=lazy(()=>import('./pages/Memory').then(module=>({default:module.MemoryPage})))
const WorkflowsPage=lazy(()=>import('./pages/Workflows').then(module=>({default:module.WorkflowsPage})))
const ChatPage=lazy(()=>import('./pages/Chat').then(module=>({default:module.ChatPage})))
const VideoPage=lazy(()=>import('./pages/Video').then(module=>({default:module.VideoPage})))
const ModelHub=lazy(()=>import('./pages/Models').then(module=>({default:module.ModelHub})))
const AgentsPage=lazy(()=>import('./pages/Agents').then(module=>({default:module.AgentsPage})))
const RoomsPage=lazy(()=>import('./pages/Rooms').then(module=>({default:module.RoomsPage})))
const ExtensionsPage=lazy(()=>import('./pages/Extensions').then(module=>({default:module.ExtensionsPage})))
const ConnectionsPage=lazy(()=>import('./pages/Connections').then(module=>({default:module.ConnectionsPage})))
const OperationsPage=lazy(()=>import('./pages/Operations').then(module=>({default:module.OperationsPage})))
const DeveloperPage=lazy(()=>import('./pages/Developer').then(module=>({default:module.DeveloperPage})))

function locationState(){
 const raw=window.location.hash.replace(/^#\/?/,'')
 const [name,query]=raw.split('?')
 const valid=name==='capabilities'||navigation.some(group=>group.items.some(item=>item.id===name))
 return {page:(valid?name:'chat') as PageId,session:new URLSearchParams(query||'').get('session')||undefined}
}
export default function App(){
 const [page,setPage]=useState<PageId>(()=>locationState().page)
 const [session,setSession]=useState<string|undefined>(()=>locationState().session)
 const [chatResetKey,setChatResetKey]=useState(0)
 const [learningNotice,setLearningNotice]=useState<{inserted:number;skill:boolean}|null>(null)
 const [sidebar,setSidebar]=useState(false)
 const [collapsed,setCollapsed]=useState(()=>{try{const saved=localStorage.getItem('dominion.nav.collapsed');return saved===null?window.innerWidth<1200:saved==='1'}catch{return false}})
 const [palette,setPalette]=useState(false)
 const [online,setOnline]=useState(false)
 const [running,setRunning]=useState(0)
 const [pending,setPending]=useState(0)
 const toggleNav=useCallback(()=>{
  try{
   if(window.innerWidth<520){setSidebar(open=>!open);return}
   const next=!collapsed
   setCollapsed(next)
   try{localStorage.setItem('dominion.nav.collapsed',next?'1':'0')}catch{}
  }catch{setSidebar(open=>!open)}
 },[collapsed])
 const navigate=useCallback((target:PageId,sessionId?:string)=>{
  setPage(target)
  if(sessionId!==undefined)setSession(sessionId)
  const next='#/'+target+(sessionId?'?session='+encodeURIComponent(sessionId):'')
  if(window.location.hash!==next)window.location.hash=next
  setSidebar(false)
  setPalette(false)
  window.scrollTo({top:0,behavior:'auto'})
 },[])
 const newChat=useCallback(()=>{setSession(undefined);setChatResetKey(value=>value+1);setPage('chat');window.location.hash='#/chat';setSidebar(false);setPalette(false)},[])
 const changeSession=useCallback((id:string|undefined)=>{setSession(id);window.history.replaceState(null,'','#/chat'+(id?'?session='+encodeURIComponent(id):''))},[])
 useEffect(()=>{if(!learningNotice)return;const timer=setTimeout(()=>setLearningNotice(null),9000);return()=>clearTimeout(timer)},[learningNotice])
 useEffect(()=>{
  const sync=()=>{const current=locationState();setPage(current.page);setSession(current.session);setSidebar(false);setPalette(false)}
  window.addEventListener('hashchange',sync)
  return()=>window.removeEventListener('hashchange',sync)
 },[])
 useEffect(()=>{
  let mounted=true
  const check=async()=>{
   try{
    const status=await get<{jobs?:number}>('/api/status')
    if(mounted){setOnline(true);setRunning(Number(status.jobs)||0)}
   }catch{if(mounted){setOnline(false);setRunning(0)}}
  }
  const approvals=async()=>{
   try{const data=await get<unknown[]>('/api/approvals');if(mounted)setPending(Array.isArray(data)?data.length:0)}
   catch{if(mounted)setPending(0)}
  }
  void check();void approvals()
  const poll=setInterval(()=>{void check()},12000)
  const stream=new EventSource('/api/events')
  stream.onmessage=event=>{
   try{
    const item=JSON.parse(event.data)
    if(item.type==='learning_feedback'){window.dispatchEvent(new CustomEvent('dominion-learning-feedback',{detail:item}));if(Number(item.inserted)>0||item.skill)setLearningNotice({inserted:Number(item.inserted)||0,skill:Boolean(item.skill)});return}
    if(item.type==='room_event'){window.dispatchEvent(new CustomEvent('dominion-room-event',{detail:item}));return}
    if(['record','message','job_event','audit','approval','channel_status','goal_event','execution_event'].includes(item.type)){
     window.dispatchEvent(new Event('dominion-refresh'))
     if(item.type==='approval')void approvals()
     if(item.type==='job_event'||item.type==='goal_event'||item.type==='execution_event')void check()
    }
   }catch{}
  }
  const shortcuts=(e:KeyboardEvent)=>{
   if((e.metaKey||e.ctrlKey)&&e.key.toLowerCase()==='k'){e.preventDefault();setPalette(open=>!open)}
   if(e.key==='Escape')setPalette(false)
  }
  window.addEventListener('keydown',shortcuts)
  return()=>{mounted=false;clearInterval(poll);stream.close();window.removeEventListener('keydown',shortcuts)}
 },[])
 return <div className="min-h-screen bg-[#F6F3EC] font-sans text-[#24211D]">
  <WorkspaceSidebar page={page} open={sidebar} collapsed={collapsed} online={online} running={running} pending={pending} onNavigate={navigate} onNewChat={newChat} onToggle={toggleNav} onClose={()=>setSidebar(false)}/>
  <main className={'min-h-screen min-w-0 transition-[margin] duration-200 '+(collapsed?'ml-0':'ml-[216px] max-[519px]:ml-0')}>
   <WorkspaceHeader page={page} online={online} collapsed={collapsed} onMenu={toggleNav} onSearch={()=>setPalette(true)}/>
   <div className={'min-w-0 page-'+page}>
    <Suspense fallback={<div role="status" className="p-8 text-sm text-[#7C7469]">화면 불러오는 중…</div>}>
    {page==='capabilities'&&<CapabilitiesPage navigate={navigate}/>}
    {page==='memory'&&<MemoryPage/>}
    {page==='workflows'&&<WorkflowsPage openSession={id=>navigate('chat',id)}/>}
    {page==='chat'&&<ChatPage key={chatResetKey} sessionId={session} onSession={changeSession} openModels={()=>navigate('models')}/>}
    {page==='video'&&<VideoPage navigate={navigate}/>}
    {page==='models'&&<ModelHub/>}
    {page==='agents'&&<AgentsPage/>}
    {page==='rooms'&&<RoomsPage openSession={id=>navigate('chat',id)}/>}
    {page==='extensions'&&<ExtensionsPage/>}
    {page==='connections'&&<ConnectionsPage/>}
    {page==='developer'&&<DeveloperPage/>}
    {(page==='automations'||page==='activity'||page==='settings')&&<OperationsPage section={page}/>}
    </Suspense>
   </div>
  </main>
  {learningNotice&&<div className="learning-feedback-toast" role="status"><span>{learningNotice.inserted>0?'장기기억 '+learningNotice.inserted+'개 저장':''}{learningNotice.skill?(learningNotice.inserted>0?' · ':'')+'스킬 후보 생성':''}</span><button onClick={()=>{navigate('memory');setLearningNotice(null)}}>메모리 보기</button><button aria-label="알림 닫기" onClick={()=>setLearningNotice(null)}>×</button></div>}
  {palette&&<CommandPalette onNavigate={navigate} onClose={()=>setPalette(false)}/>}
 </div>
}
