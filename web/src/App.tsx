import { ContextMenu } from './components/ContextMenu'
import {OwnerProfileProvider} from './components/OwnerProfile'
import {Component,lazy,Suspense,useCallback,useEffect,useLayoutEffect,useState,type ReactNode} from 'react'
import {get} from './api'
import {WorkspaceSidebar,WorkspaceHeader,CommandPalette,navigation,type PageId} from './components/WorkspaceShell'
// The original main UI shell is retained; functional pages use the Rust SDK API.
const RustWorkspacePage=lazy(()=>import('./pages/RustWorkspace').then(module=>({default:module.RustWorkspacePage})))
const HomePage=lazy(()=>import('./pages/Home').then(module=>({default:module.HomePage})))
const MemoryPage=lazy(()=>import('./pages/Memory').then(module=>({default:module.MemoryPage})))

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
 const valid=navigation.some(group=>group.items.some(item=>item.id===name))
 const params=new URLSearchParams(query)
 return {page:(valid?name:'home') as PageId,session:params.get('session')||undefined,
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
 const [engineBuild,setEngineBuild]=useState<{branch:string;commit:string}|null>(null)
 useEffect(()=>{
  let live=true
  const load=()=>{void get<{branch?:string;commit?:string}>('/api/v1/health')
   .then(info=>{if(live)setEngineBuild(info.branch&&info.commit?{branch:info.branch,commit:info.commit}:null)}).catch(()=>{if(live)setEngineBuild(null)})}
  load()
  const focus=()=>{if(document.visibilityState==='visible')load()}
  window.addEventListener('focus',focus)
  return()=>{live=false;window.removeEventListener('focus',focus)}
 },[])
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
  // Hash navigation stays immediate. Reload only when the server's built
  // entry changed; never block menu clicks on a network request.
  void checkForUpdatedUi()
 },[])
 useEffect(()=>{
  const sync=()=>{setRoute(locationState());setSidebar(false);setPalette(false)}
  window.addEventListener('hashchange',sync)
  window.addEventListener('popstate',sync)
  return()=>{window.removeEventListener('hashchange',sync);window.removeEventListener('popstate',sync)}
 },[])
 useLayoutEffect(()=>{window.scrollTo({top:0,behavior:'instant'})},[page])
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
    const runs=await get<{status:string}[]>('/api/v1/runs')
    if(mounted){
     setOnline(true)
     setRunning(runs.filter(r=>r.status==='queued'||r.status==='running').length)
     setPending(runs.filter(r=>r.status==='awaiting_approval').length)
    }
   }catch{if(mounted){setOnline(false);setRunning(0);setPending(0)}}
  }
  void check()
  const poll=setInterval(()=>{void check()},2500)
  const shortcuts=(e:KeyboardEvent)=>{
   if((e.metaKey||e.ctrlKey)&&e.key.toLowerCase()==='k'){e.preventDefault();setPalette(open=>!open)}
   if(e.key==='Escape')setPalette(false)
  }
  window.addEventListener('keydown',shortcuts)
  return()=>{mounted=false;clearInterval(poll);window.removeEventListener('keydown',shortcuts)}
 },[])
 return <OwnerProfileProvider><div className="min-h-dvh bg-[#F6F3EC] font-sans text-[#24211D]">
   <ContextMenu />
  <WorkspaceSidebar page={page} open={sidebar} collapsed={collapsed} online={online} running={running} pending={pending} onNavigate={navigate} onNewChat={()=>{setChatDraft(value=>value+1);navigate('chat')}} onClose={()=>setSidebar(false)}/>
  <main className={'min-h-dvh min-w-0 transition-[margin] duration-200 '+(collapsed?'ml-0':'ml-[216px] max-[519px]:ml-0')}>
   <WorkspaceHeader page={page} online={online} collapsed={collapsed} build={engineBuild} onMenu={toggleNav} onSearch={()=>setPalette(true)}/>
   <div className={'min-w-0 page-'+page}>
    <PageLoadBoundary key={page}>
    <Suspense fallback={<div role="status" className="p-8 text-sm text-[#7C7469]">화면 불러오는 중…</div>}>
    {page==='home'?<HomePage navigate={navigate}/>:page==='memory'?<MemoryPage/>:<RustWorkspacePage key={page+'-'+(page==='chat'?chatDraft:0)} area={page}/>}
    </Suspense>
    </PageLoadBoundary>
   </div>
  </main>
  {palette&&<CommandPalette onNavigate={navigate} onClose={()=>setPalette(false)}/>}
 </div></OwnerProfileProvider>
}
