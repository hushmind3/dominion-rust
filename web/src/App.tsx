import {DialogBackdrop} from './components/Dialog'
import { ContextMenu } from './components/ContextMenu'
import {OwnerProfileProvider} from './components/OwnerProfile'
import {Component,lazy,Suspense,useCallback,useEffect,useLayoutEffect,useState,type ReactNode} from 'react'
import {get} from './api'
import {classifyRefreshEvent,emptyRefreshScope,mergeRefreshScopes} from './refreshEvents'
import {WorkspaceSidebar,SettingsHeader,navigation,type PageId} from './components/WorkspaceShell'
const HomePage=lazy(()=>import('./pages/Home').then(module=>({default:module.HomePage})))
const CapabilitiesPage=lazy(()=>import('./pages/Capabilities').then(module=>({default:module.CapabilitiesPage})))
const MemoryPage=lazy(()=>import('./pages/Memory').then(module=>({default:module.MemoryPage})))
const ChatPage=lazy(()=>import('./pages/Chat').then(module=>({default:module.ChatPage})))
const ModelHub=lazy(()=>import('./pages/Models').then(module=>({default:module.ModelHub})))
const AgentsPage=lazy(()=>import('./pages/Agents').then(module=>({default:module.AgentsPage})))
const RoomsPage=lazy(()=>import('./pages/Rooms').then(module=>({default:module.RoomsPage})))
const ExtensionsPage=lazy(()=>import('./pages/Extensions').then(module=>({default:module.ExtensionsPage})))
const ConnectionsPage=lazy(()=>import('./pages/Connections').then(module=>({default:module.ConnectionsPage})))
const OperationsPage=lazy(()=>import('./pages/Operations').then(module=>({default:module.OperationsPage})))


// A running tab retains the entry bundle from the moment it was opened.
// After "npm run build", lazy-loaded page chunks have new hashed URLs, while
// this tab still points at the removed ones. React.lazy alone cannot recover.
const loadedEntry = document.querySelector<HTMLScriptElement>('script[type="module"][src]')?.getAttribute('src') || ''
const loadedBundle = /\/assets\/[^/?]+\.js(?:\?|$)/.test(loadedEntry)
 ? new URL(loadedEntry, document.baseURI).pathname : ''
let reloadScheduled = false
let lastVersionCheck = 0
let versionCheck: Promise<void> | undefined

function reloadUpdatedUi() {
 if (reloadScheduled) return
 // If a new bundle is itself broken, show the recovery button instead of
 // trapping the browser in an endless reload loop.
 const key = 'dominion.ui.autoreload'
 try {
  const last = JSON.parse(sessionStorage.getItem(key) || 'null') as {entry:string;at:number}|null
  if (last?.entry === loadedBundle && Date.now()-last.at < 15000) return
  sessionStorage.setItem(key,JSON.stringify({entry:loadedBundle,at:Date.now()}))
 } catch {}
 reloadScheduled = true
 window.location.reload()
}

function checkForUpdatedUi() {
 if (!loadedBundle || reloadScheduled) return Promise.resolve()
 if (versionCheck) return versionCheck
 if (Date.now() - lastVersionCheck < 1500) return Promise.resolve()
 lastVersionCheck=Date.now()
 versionCheck=(async()=>{
  try{
   // The Rust server already sends no-cache. Check the actual HTML entry,
   // not an API status flag or a timestamp that changes on every request.
   const response=await fetch('/',{cache:'no-store',headers:{Accept:'text/html'},signal:AbortSignal.timeout(2500)})
   if (!response.ok || !response.headers.get('content-type')?.includes('text/html')) return
   const next=new DOMParser().parseFromString(await response.text(),'text/html')
   const nextEntry=next.querySelector<HTMLScriptElement>('script[type="module"][src]')?.getAttribute('src')
   if (!nextEntry || !/\/assets\/[^/?]+\.js(?:\?|$)/.test(nextEntry)) return
   const nextBundle=new URL(nextEntry,window.location.origin).pathname
   if (nextBundle!==loadedBundle) reloadUpdatedUi()
  }catch{
   // The API or frontend may be restarting; navigation must remain usable.
  }
 })().finally(()=>{versionCheck=undefined})
 return versionCheck
}

// Vite emits this for missing lazy page assets after a rebuild.
window.addEventListener('vite:preloadError',event=>{
 event.preventDefault()
 reloadUpdatedUi()
})

class PageLoadBoundary extends Component<{children:ReactNode},{failed:boolean}>{
 state={failed:false}
 static getDerivedStateFromError(){return {failed:true}}
 componentDidCatch(error:Error){
  if (/dynamically imported module|importing a module script|loading chunk|failed to fetch.*module|module script/i.test(error.message)) {
   reloadUpdatedUi()
  }
 }
 render(){
  if (!this.state.failed) return this.props.children
  return <div role="alert" className="m-6 max-w-lg rounded-xl border border-[#E7DDCC] bg-white p-6 text-sm text-[#5C544A]">
   <p className="mb-3 font-semibold">화면을 불러오지 못했습니다. 업데이트 중이었을 수 있습니다.</p>
   <button type="button" className="rounded-lg bg-[#C96442] px-4 py-2 font-semibold text-white" onClick={()=>window.location.reload()}>최신 화면으로 다시 열기</button>
  </div>
 }
}

function locationState(){
 const raw=window.location.hash.replace(/^#\/?/,'')
 const [name,query]=raw.split('?')
 const valid=['extensions','capabilities'].includes(name)||navigation.some(group=>group.items.some(item=>item.id===name))
 const params=new URLSearchParams(query)
 return {page:(valid?name:'chat') as PageId,session:params.get('session')||undefined,
  agent:params.get('agent')||undefined,provider:params.get('provider')||undefined,model:params.get('model')||undefined}
}
export default function App(){
 const [{page,session,agent,provider,model},setRoute]=useState(locationState)
 const [chatRoute,setChatRoute]=useState(()=>{const route=locationState();return route.page==='chat'?route:{page:'chat' as PageId,session:undefined,agent:undefined,provider:undefined,model:undefined}})
 const [mode,setMode]=useState<'chat'|'rooms'>(()=>locationState().page==='rooms'?'rooms':'chat')
 const [visited,setVisited]=useState(()=>({chat:locationState().page!=='rooms',rooms:locationState().page==='rooms'}))
 const [chatDraft,setChatDraft]=useState(0)
 const [settingsNav,setSettingsNav]=useState(()=>{try{return localStorage.getItem('dominion.settings.nav')!=='0'}catch{return true}})
 const toggleSettingsNav=useCallback(()=>setSettingsNav(open=>{const next=!open;try{localStorage.setItem('dominion.settings.nav',next?'1':'0')}catch{}return next}),[])
 const settingsOpen=page!=='chat'&&page!=='rooms'
 const [online,setOnline]=useState(false)
 const [running,setRunning]=useState(0)
 const [pending,setPending]=useState(0)
 const [engineBuild,setEngineBuild]=useState<{branch:string;commit:string}|null>(null)
 useEffect(()=>{
  let live=true
  const load=()=>{void get<{branch:string;commit:string}>('/api/health')
   .then(info=>{if(live)setEngineBuild(info)}).catch(()=>{if(live)setEngineBuild(null)})}
  load()
  const focus=()=>{if(document.visibilityState==='visible')load()}
  window.addEventListener('focus',focus)
  return()=>{live=false;window.removeEventListener('focus',focus)}
 },[])
 const navigate=useCallback((target:PageId,sessionId?:string)=>{
  const next='#/'+target+(sessionId?'?session='+encodeURIComponent(sessionId):'')
  if(window.location.hash!==next)window.history.pushState(null,'',next)
  setRoute(locationState())
  // Hash navigation stays immediate. Reload only when the server's built
  // entry changed; never block menu clicks on a network request.
  void checkForUpdatedUi()
 },[])
 useEffect(()=>{
  const sync=()=>setRoute(locationState())
  window.addEventListener('hashchange',sync)
  window.addEventListener('popstate',sync)
  return()=>{window.removeEventListener('hashchange',sync);window.removeEventListener('popstate',sync)}
 },[])
 useLayoutEffect(()=>{
  if(page==='chat'){setChatRoute({page,session,agent,provider,model});setMode('chat');setVisited(previous=>previous.chat?previous:{...previous,chat:true})}
  if(page==='rooms'){setMode('rooms');setVisited(previous=>previous.rooms?previous:{...previous,rooms:true})}
 },[page,session,agent,provider,model])
 const returnToChat=useCallback(()=>{
  const params=new URLSearchParams()
  for(const key of ['session','agent','provider','model'] as const)if(chatRoute[key])params.set(key,chatRoute[key]!)
  const next='#/chat'+(params.size?'?'+params.toString():'')
  if(window.location.hash!==next)window.history.pushState(null,'',next)
  setRoute(locationState())
  void checkForUpdatedUi()
 },[chatRoute])
 const switchMode=useCallback((next:'chat'|'rooms')=>{if(next==='chat')returnToChat();else navigate('rooms')},[returnToChat,navigate])
 const closeSettings=useCallback(()=>switchMode(mode),[switchMode,mode])
 useEffect(()=>{
  const check=()=>{if(document.visibilityState==='visible')void checkForUpdatedUi()}
  window.addEventListener('focus',check)
  document.addEventListener('visibilitychange',check)
  return()=>{window.removeEventListener('focus',check);document.removeEventListener('visibilitychange',check)}
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
  let refreshTimer:ReturnType<typeof setTimeout>|undefined
  let statusTimer:ReturnType<typeof setTimeout>|undefined
  let approvalsTimer:ReturnType<typeof setTimeout>|undefined
  let pendingRefresh=emptyRefreshScope()
  const scheduleStatus=()=>{
   if(statusTimer)return
   statusTimer=setTimeout(()=>{statusTimer=undefined;void check()},250)
  }
  const scheduleApprovals=()=>{
   if(approvalsTimer)return
   approvalsTimer=setTimeout(()=>{approvalsTimer=undefined;void approvals()},250)
  }
  stream.onmessage=event=>{
   try{
    const item=JSON.parse(event.data)
    if(item.type==='room_event'){
     window.dispatchEvent(new CustomEvent('dominion-room-event',{detail:item}))
     return
    }
    // Keep SSE records scoped to their consumers. A busy tool journal should
    // not re-fetch agents, providers or account connections on every receipt.
    const next=classifyRefreshEvent(item)
    if(next.targets.length||next.rooms.length||next.sessions.length){
     pendingRefresh=mergeRefreshScopes(pendingRefresh,next)
     refreshTimer ??= setTimeout(()=>{
      refreshTimer=undefined
      const batch=pendingRefresh
      pendingRefresh=emptyRefreshScope()
      window.dispatchEvent(new CustomEvent('dominion-data-change',{detail:batch}))
     },100)
    }
    if(item.type==='approval'||(item.type==='job_event'&&['awaiting_approval','completed','failed','cancelled'].includes(item.status)))scheduleApprovals()
    if(item.type==='job_event'||item.type==='goal_event'||item.type==='execution_event'
      ||(item.type==='record'&&item.kind==='job'))scheduleStatus()
   }catch{}
  }
  const shortcuts=(e:KeyboardEvent)=>{
   if((e.metaKey||e.ctrlKey)&&e.key===','){e.preventDefault();navigate('settings')}
  }
  window.addEventListener('keydown',shortcuts)
  return()=>{mounted=false;clearInterval(poll);clearTimeout(refreshTimer);clearTimeout(statusTimer);clearTimeout(approvalsTimer);stream.close();window.removeEventListener('keydown',shortcuts)}
 },[navigate])
 return <OwnerProfileProvider><div className="h-dvh overflow-hidden bg-[#FFFDF9] font-sans text-[#24211D]">
  <ContextMenu/>
  <main inert={settingsOpen} className="h-dvh min-w-0">
   {visited.chat&&<div hidden={mode!=='chat'} className="h-full page-chat"><PageLoadBoundary><Suspense fallback={<div role="status" className="p-8 text-sm text-[#7C7469]">대화 불러오는 중…</div>}>
    <ChatPage key={chatDraft} active={mode==='chat'&&!settingsOpen} sessionId={chatRoute.session} initialAgent={chatRoute.agent} initialProvider={chatRoute.provider} initialModel={chatRoute.model} onSession={id=>{
     setChatRoute(previous=>({...previous,session:id}))
     if(locationState().page==='chat')navigate('chat',id)
    }} openModels={()=>navigate('models')} openSettings={()=>navigate('settings')} switchMode={switchMode}/>
   </Suspense></PageLoadBoundary></div>}
   {visited.rooms&&<div hidden={mode!=='rooms'} className="h-full page-rooms"><PageLoadBoundary><Suspense fallback={<div role="status" className="p-8 text-sm text-[#7C7469]">대화 불러오는 중…</div>}><RoomsPage active={mode==='rooms'&&!settingsOpen} openSession={id=>navigate('chat',id)} openSettings={()=>navigate('settings')} switchMode={switchMode}/></Suspense></PageLoadBoundary></div>}

  </main>
  {settingsOpen&&<DialogBackdrop onDismiss={closeSettings}><div role="dialog" aria-modal="true" aria-label="설정" className="workspace-settings relative h-[calc(100dvh-40px)] w-full max-w-[1440px] overflow-hidden rounded-2xl border border-[#E7E0D5] bg-[#F6F3EC] shadow-2xl">
   <WorkspaceSidebar settingsOnly page={page} open collapsed={false} online={online} running={running} pending={pending} onNavigate={navigate} onNewChat={()=>navigate('chat')} onClose={closeSettings} navOpen={settingsNav} onToggleNav={toggleSettingsNav}/>
   <div className={'workspace-settings-main flex h-full min-w-0 flex-col '+(settingsNav?'ml-[216px]':'ml-14')}>
    <SettingsHeader page={page} online={online} build={engineBuild} onClose={closeSettings}/>
    <div className="workspace-settings-content min-h-0 flex-1 overflow-y-auto">
     <PageLoadBoundary key={page}><Suspense fallback={<div role="status" className="p-8 text-sm text-[#7C7469]">화면 불러오는 중…</div>}>
      {page==='home'&&<HomePage navigate={navigate}/>}
      {page==='capabilities'&&<CapabilitiesPage navigate={navigate}/>}
      {page==='memory'&&<MemoryPage/>}
      {page==='models'&&<ModelHub/>}
      {page==='agents'&&<AgentsPage/>}
      {page==='extensions'&&<ExtensionsPage/>}
      {page==='connections'&&<ConnectionsPage/>}
      {(page==='activity'||page==='settings')&&<OperationsPage section={page}/>}
     </Suspense></PageLoadBoundary>
    </div>
   </div>
  </div></DialogBackdrop>}
 </div></OwnerProfileProvider>
}
