import { ContextMenu } from './components/ContextMenu'
import {useCallback,useEffect,useState} from 'react'
import {get} from './api'
import {WorkspaceSidebar,WorkspaceHeader,CommandPalette,navigation,type PageId} from './components/WorkspaceShell'
import {CapabilitiesPage} from './pages/Capabilities'
import {MemoryPage} from './pages/Memory'
import {WorkflowsPage} from './pages/Workflows'
import {ChatPage} from './pages/Chat'
import {VideoPage} from './pages/Video'
import {ModelHub} from './pages/Models'
import {AgentsPage,RoomsPage} from './pages/Agents'
import {ExtensionsPage} from './pages/Extensions'
import {ConnectionsPage} from './pages/Connections'
import {OperationsPage} from './pages/Operations'
import {DeveloperPage} from './pages/Developer'

function locationState(){
 const raw=window.location.hash.replace(/^#\/?/,'')
 const [name,query]=raw.split('?')
 const valid=navigation.some(group=>group.items.some(item=>item.id===name))
 return {page:(valid?name:'chat') as PageId,session:new URLSearchParams(query||'').get('session')||undefined}
}
export default function App(){
 const [page,setPage]=useState<PageId>(()=>locationState().page)
 const [session,setSession]=useState<string|undefined>(()=>locationState().session)
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
 useEffect(()=>{
  const sync=()=>{const current=locationState();setPage(current.page);if(current.session)setSession(current.session);setSidebar(false);setPalette(false)}
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
   <ContextMenu />
  <WorkspaceSidebar page={page} open={sidebar} collapsed={collapsed} online={online} running={running} pending={pending} onNavigate={navigate} onClose={()=>setSidebar(false)}/>
  <main className={'min-h-screen min-w-0 transition-[margin] duration-200 '+(collapsed?'ml-0':'ml-[216px] max-[519px]:ml-0')}>
   <WorkspaceHeader page={page} online={online} collapsed={collapsed} onMenu={toggleNav} onSearch={()=>setPalette(true)}/>
   <div className={'min-w-0 page-'+page}>
    {page==='capabilities'&&<CapabilitiesPage navigate={navigate}/>}
    {page==='memory'&&<MemoryPage/>}
    {page==='workflows'&&<WorkflowsPage openSession={id=>navigate('chat',id)}/>}
    {page==='chat'&&<ChatPage sessionId={session} onSession={setSession} openModels={()=>navigate('models')}/>}
    {page==='video'&&<VideoPage navigate={navigate}/>}
    {page==='models'&&<ModelHub/>}
    {page==='agents'&&<AgentsPage/>}
    {page==='rooms'&&<RoomsPage openSession={id=>navigate('chat',id)}/>}
    {page==='extensions'&&<ExtensionsPage/>}
    {page==='connections'&&<ConnectionsPage/>}
    {page==='developer'&&<DeveloperPage/>}
    {(page==='automations'||page==='activity'||page==='settings')&&<OperationsPage section={page}/>}
   </div>
  </main>
  {palette&&<CommandPalette onNavigate={navigate} onClose={()=>setPalette(false)}/>}
 </div>
}
