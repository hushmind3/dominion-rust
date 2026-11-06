import { ContextMenu } from './components/ContextMenu'
import {lazy,Suspense,useCallback,useEffect,useLayoutEffect,useState} from 'react'
import {get} from './api'
import {WorkspaceSidebar,WorkspaceHeader,CommandPalette,navigation,type PageId} from './components/WorkspaceShell'
const HomePage=lazy(()=>import('./pages/Home').then(module=>({default:module.HomePage})))
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
 const valid=navigation.some(group=>group.items.some(item=>item.id===name))
 const params=new URLSearchParams(query)
 return {page:(valid?name:'chat') as PageId,session:params.get('session')||undefined,
  agent:params.get('agent')||undefined,provider:params.get('provider')||undefined,model:params.get('model')||undefined}
}
export default function App(){
 const [{page,session,agent,provider,model},setRoute]=useState(locationState)
 const [chatDraft,setChatDraft]=useState(0)
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
  const next='#/'+target+(sessionId?'?session='+encodeURIComponent(sessionId):'')
  if(window.location.hash!==next)window.history.pushState(null,'',next)
  setRoute(locationState())
  setSidebar(false)
  setPalette(false)
 },[])
 useEffect(()=>{
  const sync=()=>{setRoute(locationState());setSidebar(false);setPalette(false)}
  window.addEventListener('hashchange',sync)
  window.addEventListener('popstate',sync)
  return()=>{window.removeEventListener('hashchange',sync);window.removeEventListener('popstate',sync)}
 },[])
 useLayoutEffect(()=>{window.scrollTo({top:0,behavior:'instant'})},[page])
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
  let refreshTimer: ReturnType<typeof setTimeout> | undefined
  stream.onmessage=event=>{
   try{
    const item=JSON.parse(event.data)
    if(item.type==='room_event'){window.dispatchEvent(new CustomEvent('dominion-room-event',{detail:item}));return}
    if(['record','message','job_event','audit','approval','channel_status','goal_event','execution_event'].includes(item.type)){
     refreshTimer ??= setTimeout(()=>{
      refreshTimer=undefined
      window.dispatchEvent(new Event('dominion-refresh'))
     },100)
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
  return()=>{mounted=false;clearInterval(poll);clearTimeout(refreshTimer);stream.close();window.removeEventListener('keydown',shortcuts)}
 },[])
 return <div className="min-h-dvh bg-[#F6F3EC] font-sans text-[#24211D]">
   <ContextMenu />
  <WorkspaceSidebar page={page} open={sidebar} collapsed={collapsed} online={online} running={running} pending={pending} onNavigate={navigate} onNewChat={()=>{setChatDraft(value=>value+1);navigate('chat')}} onClose={()=>setSidebar(false)}/>
  <main className={'min-h-dvh min-w-0 transition-[margin] duration-200 '+(collapsed?'ml-0':'ml-[216px] max-[519px]:ml-0')}>
   <WorkspaceHeader page={page} online={online} collapsed={collapsed} onMenu={toggleNav} onSearch={()=>setPalette(true)}/>
   <div className={'min-w-0 page-'+page}>
    <Suspense fallback={<div role="status" className="p-8 text-sm text-[#7C7469]">화면 불러오는 중…</div>}>
    {page==='home'&&<HomePage navigate={navigate}/>}
    {page==='capabilities'&&<CapabilitiesPage navigate={navigate}/>}
    {page==='memory'&&<MemoryPage/>}
    {page==='workflows'&&<WorkflowsPage openSession={id=>navigate('chat',id)}/>}
    {page==='chat'&&<ChatPage key={chatDraft} sessionId={session} initialAgent={agent} initialProvider={provider} initialModel={model} onSession={id=>{if(locationState().page==='chat')navigate('chat',id)}} openModels={()=>navigate('models')}/>}
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
  {palette&&<CommandPalette onNavigate={navigate} onClose={()=>setPalette(false)}/>}
 </div>
}
