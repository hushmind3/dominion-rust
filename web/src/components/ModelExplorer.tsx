import {useCallback,useEffect,useLayoutEffect,useRef,useState} from 'react'
import {ArrowDownToLine,ArrowRight,Check,ChevronDown,CircleAlert,CloudDownload,Cpu,ExternalLink,FolderOpen,HardDrive,LoaderCircle,Pause,Play,RefreshCw,Search,Square,Trash2,X} from 'lucide-react'
import {Button,SelectInput,TextInput} from './Primitives'
import {get,post,del} from '../api'
import {matchesRefreshTarget,type RefreshScope} from '../refreshEvents'
import {t,useLocale} from '../i18n'
import {translateError} from '../i18n/errors'
import {classifyRuntimeFailure,engineForModel,memoryFit as getMemoryFit,modelPreflight,planDownloadLaunch,preferredServerModelFormat,requiredMemory,selectRecommended,selectSmaller} from '../utils/modelHardware'
type Catalog={id:string;author:string;name:string;library:string;createdAt?:string;downloads:number;likes:number;gated:boolean;updatedAt:string;description:string;task:string;tags:string[]}
type Option={id:string;format:'gguf'|'mlx';label:string;quantization:string;bytes:number;files:string[];shards:number}
type Detail=Catalog&{revision:string;license:string;architecture:string;options:Option[];readme:string}
type Installed={id:string;repo:string;optionId?:string;source?:string;externalPath?:string;format:'gguf'|'mlx';label:string;bytes:number;installedAt:string}
type Job={id:string;repo:string;optionId:string;revision?:string;status:string;downloaded:number;total:number;current:string;error:string;installedId?:string;startedAt?:string}
type Runtime={status:string;id?:string;repo?:string;engine?:string;name?:string;model?:string;error?:string;setup?:{status:string;engine?:string;backend:string;downloaded:number;total:number;tag:string;file?:string;error:string};capabilities?:{gguf:{available:boolean;backend?:string;installable?:boolean;path?:string|null;installMethod?:string};mlx:{available:boolean;backend?:string;installable?:boolean;standalone?:boolean;path?:string|null;installMethod?:string};lmstudio?:{available:boolean};memory:{total:number;free:number;platform?:string;arch?:string};gpu?:{kind:string;free:number}|null}}
const fmt=(v=0)=>v>=1073741824?(v/1073741824).toFixed(1)+' GB':v>=1048576?(v/1048576).toFixed(0)+' MB':v>=1024?(v/1024).toFixed(0)+' KB':String(v)+' B'
const shorten=(v:string,n=45)=>v.length>n?v.slice(0,n-3)+'…':v
const compactNumber=(v=0)=>v>=1e6?(v/1e6).toFixed(1)+'M':v>=1e3?(v/1e3).toFixed(1)+'K':String(v)
const cleanMarkdown=(value='')=>String(value).replace(/^---[\s\S]*?---/,'').replace(/```[\s\S]*?```/g,' ').replace(/\[!\[[^\]]*\]\([^)]*\)\]\([^)]*\)/g,' ').replace(/!\[[^\]]*\]\([^)]*\)/g,' ').replace(/\[([^\]]+)\]\([^)]*\)/g,'$1').replace(/^\s*\|.*$/gm,' ').replace(/^\s*[-:]{3,}\s*$/gm,' ').replace(/<[^>]+>/g,' ').replace(/^#{1,6}\s*/gm,'').replace(/[*_`>]/g,'').replace(/\n{3,}/g,'\n\n').trim()
const updatedLabel=(value:string)=>value?new Date(value).toLocaleDateString():''
const viewTabs=[{id:'installed',label:'models.tab.installed',icon:HardDrive},{id:'search',label:'models.tab.search',icon:Search},{id:'import',label:'models.tab.import',icon:FolderOpen}] as const
type PendingLaunch={jobId:string;repo:string;optionId:string;revision:string;createdAt:number}
const launchKey='dominion-model-pending-launch'
const readPendingLaunch=():PendingLaunch|null=>{
 try{
  const raw=window.sessionStorage.getItem(launchKey)
  if(!raw)return null
  const value=JSON.parse(raw) as Partial<PendingLaunch>
  if(typeof value.jobId==='string'&&value.jobId&&typeof value.repo==='string'&&typeof value.optionId==='string'
   &&typeof value.revision==='string'&&typeof value.createdAt==='number'
   &&Date.now()-value.createdAt>=0&&Date.now()-value.createdAt<24*60*60*1000)return value as PendingLaunch
  window.sessionStorage.removeItem(launchKey)
 }catch{/* The browser may block session storage; launching still works in memory. */}
 return null
}
type PendingEngineLaunch={id:string;from:'installed'|'search';createdAt:number}
const engineLaunchKey='dominion-model-engine-retry'
const readPendingEngineLaunch=():PendingEngineLaunch|null=>{
 try{
  const raw=window.sessionStorage.getItem(engineLaunchKey)
  if(!raw)return null
  const value=JSON.parse(raw) as Partial<PendingEngineLaunch>
  if(typeof value.id==='string'&&value.id&&['installed','search'].includes(value.from||'')
    &&typeof value.createdAt==='number'&&Date.now()-value.createdAt>=0
    &&Date.now()-value.createdAt<24*60*60*1000)return value as PendingEngineLaunch
  window.sessionStorage.removeItem(engineLaunchKey)
 }catch{/* Session storage can be disabled; in-memory navigation still works. */}
 return null
}
export function ModelExplorer({initialView='installed',hideNavigation=false}:{initialView?:'installed'|'search'|'import';hideNavigation?:boolean}={}){
 useLocale()
 const [view,setView]=useState<'installed'|'search'|'import'>(initialView)
 const [installed,setInstalled]=useState<Installed[]>([])
 const [jobs,setJobs]=useState<Job[]>([])
 const [runtime,setRuntime]=useState<Runtime>({status:'idle'})
 const [runtimes,setRuntimes]=useState<any[]>([])
 const [q,setQ]=useState('')
 const [format,setFormat]=useState('gguf')
 const userChoseFormat=useRef(false)
 const preferredFormat=preferredServerModelFormat(runtime.capabilities?.memory.platform,runtime.capabilities?.memory.arch,runtime.capabilities?.mlx,runtime.capabilities?.gguf)
 useEffect(()=>{if(preferredFormat&&!userChoseFormat.current)setFormat(preferredFormat)},[preferredFormat])
 const [chatOnly,setChatOnly]=useState(false)
 const [maxParams,setMaxParams]=useState('')
 const [sort,setSort]=useState<'downloads'|'likes'|'lastModified'>('downloads')
 const [catalogLayout,setCatalogLayout]=useState<'cards'|'list'>('cards')
 const [catalog,setCatalog]=useState<Catalog[]>([])
 const [limit,setLimit]=useState(80)
 const [selected,setSelected]=useState('')
 const [detail,setDetail]=useState<Detail|null>(null)
 const [variant,setVariant]=useState('')
 const [loading,setLoading]=useState(false)
 const [working,setWorking]=useState('')
 const [error,setError]=useState('')
 const [pendingLaunch,setPendingLaunch]=useState<PendingLaunch|null>(readPendingLaunch)
 const [pendingEngineLaunch,setPendingEngineLaunch]=useState<PendingEngineLaunch|null>(readPendingEngineLaunch)
 useEffect(()=>{try{if(pendingEngineLaunch)window.sessionStorage.setItem(engineLaunchKey,JSON.stringify(pendingEngineLaunch));else window.sessionStorage.removeItem(engineLaunchKey)}catch{/* private session */}},[pendingEngineLaunch])
 useEffect(()=>{try{if(pendingLaunch)window.sessionStorage.setItem(launchKey,JSON.stringify(pendingLaunch));else window.sessionStorage.removeItem(launchKey)}catch{/* private browser session */}},[pendingLaunch])
 const [localPath,setLocalPath]=useState('')
 const [picking,setPicking]=useState(false)
 const detailScroll=useRef<HTMLDivElement>(null)
 const catalogueScroll=useRef<HTMLDivElement>(null)
 const localPicker=['127.0.0.1','localhost','[::1]'].includes(window.location.hostname)
 const unsupported=t('models.ui.unsupported')
 const [context,setContext]=useState(4096)
 const refresh=useCallback(async()=>{
  const results=await Promise.allSettled([get<Installed[]>('/api/model-library/installed'),get<Job[]>('/api/model-library/jobs'),get<Runtime>('/api/model-library/runtime'),get<any>('/api/model-library/runtimes')])
  if(results[0].status==='fulfilled')setInstalled(results[0].value)
  if(results[1].status==='fulfilled')setJobs(results[1].value)
  if(results[2].status==='fulfilled')setRuntime(results[2].value)
  if(results[3].status==='fulfilled')setRuntimes(results[3].value?.items||[])
 },[])
 useEffect(()=>{
  let live=true;
  let timer:ReturnType<typeof setTimeout>;
  // Local model state changes slowly; ten seconds is enough and hidden tabs
  // stop polling entirely. Focus triggers an immediate refresh.
  const update=async()=>{if(!document.hidden)await refresh();if(live)timer=setTimeout(update,10000)};
  const onVisible=()=>{if(!document.hidden)void refresh()};
  // The Rust event stream reports model download progress and runtime state.
  // Throttle event-triggered refreshes instead of waiting for the 10-second
  // fallback poll or starting one request per incoming download chunk.
  let eventTimer:ReturnType<typeof setTimeout>|undefined;
  const onChange=(event?:Event)=>{
   if(event?.type==='dominion-data-change'){
    const targets=(event as CustomEvent<RefreshScope>).detail?.targets||[]
    if(!matchesRefreshTarget('/api/model-library/runtime',targets))return
   }
   if(document.hidden||eventTimer)return;
   eventTimer=setTimeout(()=>{
    eventTimer=undefined;
    if(live&&!document.hidden)void refresh();
   },800);
  };
  document.addEventListener('visibilitychange',onVisible);
  window.addEventListener('dominion-refresh',onChange);
  window.addEventListener('dominion-data-change',onChange);
  void update();
  return()=>{
   live=false;
   clearTimeout(timer);clearTimeout(eventTimer);
   document.removeEventListener('visibilitychange',onVisible);
   window.removeEventListener('dominion-refresh',onChange);
   window.removeEventListener('dominion-data-change',onChange);
  };
 },[refresh])
 useLayoutEffect(()=>{if(detailScroll.current)detailScroll.current.scrollTop=0},[selected])
 useLayoutEffect(()=>{if(catalogueScroll.current)catalogueScroll.current.scrollTop=0},[q])
 useEffect(()=>{
  if(view!=='search')return
  const controller=new AbortController()
  const timer=setTimeout(async()=>{
   setLoading(true)
   try{
    const data=await get<Catalog[]>('/api/model-library/catalog?'+new URLSearchParams({q,limit:String(limit),format,sort,chatOnly:String(chatOnly),...(maxParams?{maxParams}:{})}),controller.signal)
    if(!controller.signal.aborted){setCatalog(data);setSelected(s=>data.some(m=>m.id===s)?s:'');setError('')}
   }catch(e){if(!controller.signal.aborted){setCatalog([]);setSelected('');setError((e as Error).message)}}
   finally{if(!controller.signal.aborted)setLoading(false)}
  },300)
  return()=>{controller.abort();clearTimeout(timer)}
 },[view,q,limit,format,sort,chatOnly,maxParams])
 useEffect(()=>{setLimit(80)},[q,format,sort,chatOnly,maxParams])
 useEffect(()=>{
  if(view!=='search'||!selected){setDetail(null);setVariant('');return}
  const controller=new AbortController()
  setDetail(null);setVariant('')
  get<Detail>('/api/model-library/details?id='+encodeURIComponent(selected),controller.signal).then(d=>{
   if(!controller.signal.aborted){setDetail(d);setVariant(d.options.find(o=>format==='all'||o.format===format)?.id||d.options[0]?.id||'')}
  }).catch(e=>{if(!controller.signal.aborted)setError((e as Error).message)})
  return()=>controller.abort()
 },[selected,view,format])
 const readmePlain=detail?cleanMarkdown(detail.readme):''
 const readmeSummary=detail?(detail.description||readmePlain.slice(0,600)):''
 const option=detail?.options.find(o=>o.id===variant)
 const availableMemory=runtime.capabilities?.memory.free||0
 const totalMemory=runtime.capabilities?.memory.total||0
 const memoryHeadroom=requiredMemory(option?.bytes||0)
 const memoryFit=getMemoryFit(option?.bytes||0,runtime.capabilities?.memory.free||0)
 const gpuMemory=runtime.capabilities?.gpu?.free||0
 const gpuKind=runtime.capabilities?.gpu?.kind||''
 const gpuFit=gpuMemory>0&&option?.bytes?(getMemoryFit(option.bytes,gpuMemory)==='insufficient'?'partial':getMemoryFit(option.bytes,gpuMemory)):'unknown'
 const compatibleOptions=detail?.options.filter(o=>(format==='all'||o.format===format)&&o.bytes>0)||[]
 const recommendation=selectRecommended(compatibleOptions,availableMemory,gpuMemory)
 const recommendedOption=recommendation.option
 const recommendationBasis=recommendation.basis==='gpu'?'GPU 여유 메모리':recommendation.basis==='ram'?'시스템 여유 메모리':''
 const runtimeFailure=classifyRuntimeFailure(runtime.error||'')
 const smaller=selectSmaller(compatibleOptions,option?.bytes||0,availableMemory,gpuMemory)
 const smallerOption=smaller.option
 const selectedJob=jobs.filter(j=>j.repo===detail?.id&&j.optionId===option?.id&&(j.revision||'main')===(detail?.revision||'main')&&['resolving','downloading','paused','failed','cancelled','completed'].includes(j.status)).sort((a,b)=>(b.startedAt||'').localeCompare(a.startedAt||''))[0]
 const installedId=selectedJob?.installedId??
  (selectedJob?.status==='completed'?selectedJob.optionId.split('/').at(-1):undefined)
 const existing=installed.find(m=>m.id===installedId||(m.repo===detail?.id&&m.optionId===option?.id))
 const selectedPreflight=option&&detail?modelPreflight(existing||{id:option.format==='mlx'?'mlx:'+detail.id:'hf:'+detail.id,format:option.format,bytes:option.bytes},runtime.capabilities):null
 const activeModel=installed.find(m=>m.id===runtime.id)
 const pendingEngineModel=installed.find(m=>m.id===pendingEngineLaunch?.id)
 const pendingEngineCheck=pendingEngineModel?modelPreflight(pendingEngineModel,runtime.capabilities):null
 const pendingEngineId=pendingEngineModel?engineForModel(pendingEngineModel):null
 const pendingEngineItem=runtimes.find((item:any)=>item.id===pendingEngineId)
 const needEngine=runtime.capabilities?.gguf.available===false;
 const installing=['checking','downloading','extracting','verifying'].includes(runtime.setup?.status||'');
 const recommendedBackend=runtime.capabilities?.memory.platform==='win32'&&runtime.capabilities.memory.arch==='x64'?'cuda':'cpu';
 const installerPercent=Math.round((runtime.setup?.downloaded||0)/Math.max(1,runtime.setup?.total||0)*100);
 const safe=async(key:string,action:()=>Promise<unknown>):Promise<boolean>=>{
  setWorking(key);setError('')
  try{await action();await refresh();return true}
  catch(e){setError((e as Error).message);return false}
  finally{setWorking('')}
 }
 const importLocal=()=>safe('import',async()=>{
  const result=await post<Installed>('/api/model-library/import',{filePath:localPath.trim(),reference:true})
  setLocalPath('');setView('installed');return result
 })
 const chooseFolder=async()=>{
  if(picking||!localPicker)return;
  setPicking(true);setError('');
  try{const {path}=await post<{path?:string}>('/api/system/pick-folder',{});if(path)setLocalPath(path)}
  catch(e){setError((e as Error).message)}finally{setPicking(false)}
 }
 const chooseLocal=async()=>{
  if(picking||!localPicker)return;
  setPicking(true);setError('');
  try{const {path}=await post<{path?:string}>('/api/model-library/pick-file',{});if(path)setLocalPath(path)}
  catch(e){setError((e as Error).message)}finally{setPicking(false)}
 }
 const queueEngineSetup=(model:Installed|undefined)=>{
  if(model)setPendingEngineLaunch(current=>current?.id===model.id?current:{id:model.id,from:view==='search'?'search':'installed',createdAt:Date.now()})
  setView('import')
 }
 const load=(id:string)=>safe('load'+id,async()=>{
  // Fetch a fresh snapshot: an engine may have been installed or removed since the last poll.
  const snapshot=await get<Runtime>('/api/model-library/runtime')
  setRuntime(snapshot)
  const model=installed.find(m=>m.id===id)
  if(model){
   const check=modelPreflight(model,snapshot.capabilities)
   if(check.status==='blocked'){
    queueEngineSetup(model)
    throw Error(check.engine+' 실행기가 필요합니다. 설치 후 아래에서 실행을 이어갈 수 있습니다.')
   }
  }
  await post('/api/model-library/runtime/load',{id,context})
  setPendingEngineLaunch(current=>current?.id===id?null:current)
 })
 const continueEngineLaunch=async()=>{
  const pending=pendingEngineLaunch
  if(!pending||!installed.some(m=>m.id===pending.id))return
  const succeeded=await load(pending.id)
  if(succeeded)setView(pending.from)
 }
 const requestDownload=async(autoLaunch:boolean,target:Option|undefined=option)=>{
  if(!detail||!target)return
  const previous=jobs.filter(j=>j.repo===detail.id&&j.optionId===target.id
   &&(j.revision||'main')===(detail.revision||'main'))
   .sort((a,b)=>(b.startedAt||'').localeCompare(a.startedAt||''))[0]
  const alreadyInstalled=installed.find(m=>m.id===previous?.installedId)
   ||installed.find(m=>m.repo===detail.id&&m.optionId===target.id)
  const plan=planDownloadLaunch(previous?.status,Boolean(alreadyInstalled))
  if(plan==='installed'){
   if(autoLaunch&&alreadyInstalled)void load(alreadyInstalled.id)
   return
  }
  if(plan==='wait'&&previous){
   if(autoLaunch)setPendingLaunch({jobId:previous.id,repo:detail.id,optionId:target.id,revision:detail.revision,createdAt:Date.now()})
   return
  }
  setWorking('download');setError('')
  try{
   const job=plan==='resume'&&previous
    ?await post<Job>('/api/model-library/jobs/'+encodeURIComponent(previous.id)+'/resume',{})
    :await post<Job>('/api/model-library/download',{repo:detail.id,optionId:target.id,format:target.format,files:target.files,revision:detail.revision})
   if(autoLaunch)setPendingLaunch({jobId:job.id,repo:detail.id,optionId:target.id,revision:detail.revision,createdAt:Date.now()})
   await refresh()
  }catch(e){if(autoLaunch)setPendingLaunch(null);setError((e as Error).message)}
  finally{setWorking('')}
 }
 useEffect(()=>{
  if(!pendingLaunch)return
  const job=jobs.find(j=>j.id===pendingLaunch.jobId)
  if(!job)return
  if(['failed','cancelled'].includes(job.status)){
   setError('다운로드 후 자동 실행이 중단됐습니다: '+(job.error?translateError(job.error):job.status))
   setPendingLaunch(null)
   return
  }
  if(job.status!=='completed')return
  const model=installed.find(m=>m.id===job.installedId)
  if(!model)return
  setPendingLaunch(null)
  void load(model.id)
 },[pendingLaunch,jobs,installed])
 const unload=()=>safe('unload',()=>post('/api/model-library/runtime/unload',{}))
 const remove=async(m:Installed)=>{
  if(!window.confirm(m.source==='external'?t('models.ui.removeExternal'):t('models.ui.removeDownloaded')))return
  await safe('remove'+m.id,()=>del('/api/model-library/installed/'+encodeURIComponent(m.id)))
 }
 const btn='inline-flex h-8 items-center justify-center gap-2 rounded-lg border border-[#E7E0D5] bg-white px-3 text-[11px] font-semibold text-[#3A342D] hover:bg-[#f5f7fa] disabled:opacity-40'
 return <section className="overflow-hidden rounded-xl border border-[#E7E0D5] bg-white">
  <div className="flex flex-wrap items-center justify-between gap-3 border-b border-[#EFE9DF] px-4 py-3">
   <div className="flex flex-wrap items-center gap-1">{(!hideNavigation||view!=='search')&&viewTabs.map(tab=>{const Icon=tab.icon;return <Button variant="bare" key={tab.id} type="button" onClick={()=>{setView(tab.id);setError('')}} className={'flex h-8 items-center gap-1.5 rounded-lg px-3 text-[12px] font-semibold '+(view===tab.id?'bg-[#F7E8E1] text-[#A94E30]':'text-[#7C7469] hover:bg-[#F3EEE5]')}><Icon size={14}/>{t(tab.label)}{tab.id==='installed'&&<span className="ml-1 rounded bg-white/75 px-1.5 text-[10px]">{installed.length}</span>}</Button>})}</div>
   <div className="flex items-center gap-2 text-[11px] text-[#7C7469]"><span className={'size-1.5 rounded-full '+(runtime.status==='ready'?'bg-emerald-500':runtime.status==='loading'?'bg-amber-500':'bg-slate-300')}/>{runtime.status==='ready'?t('models.status.ready'):runtime.status==='loading'?t('models.status.loading'):runtime.status==='installing'?t('models.status.installing'):t('models.status.idle')}<Button variant="bare" type="button" aria-label={t('models.refresh')} onClick={()=>void refresh()} className="ml-2 text-[#9C9388] hover:text-[#4A443C]"><RefreshCw size={14}/></Button></div>
  </div>
  {error&&<div role="alert" className="mx-4 mt-3 flex items-start gap-2 rounded-lg border border-rose-200 bg-rose-50 px-3 py-2 text-[12px] text-rose-700"><CircleAlert size={15}/><span className="flex-1">{error}</span><Button variant="bare" aria-label={t('models.ui.close')} onClick={()=>setError('')}><X size={14}/></Button></div>}
  {pendingEngineLaunch&&view!=='import'&&<div role="status" className="mx-4 mt-3 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-[#D9DFC8] bg-[#F6F8F0] px-3 py-2 text-[11px] text-[#5C544A]">
   <span className="min-w-0 break-all">실행기 설치 후 이어갈 모델 · {pendingEngineLaunch.id}</span>
   <Button variant="bare" type="button" className={btn} onClick={()=>setView('import')}><ArrowRight size={13}/>실행 준비 이어가기</Button>
  </div>}
  {pendingLaunch&&<div role="status" className="mx-4 mt-3 flex flex-wrap items-center justify-between gap-2 rounded-lg border border-emerald-200 bg-emerald-50 px-3 py-2 text-[12px] text-emerald-800"><span className="min-w-0 break-all">다운로드 후 자동 실행 대기 · {pendingLaunch.repo} · {pendingLaunch.optionId}</span><Button variant="bare" type="button" className="text-[11px] font-semibold underline" onClick={()=>setPendingLaunch(null)}>자동 실행 취소 (다운로드 유지)</Button></div>}
  {runtime.status==='error'&&runtime.error&&<div role="alert" className="flex flex-wrap items-center justify-between gap-3 border-t border-rose-100 bg-rose-50 px-4 py-3 text-[11px] text-rose-700"><div className="flex min-w-0 flex-1 items-start gap-2"><CircleAlert size={15} className="mt-0.5 shrink-0"/><div><strong className="block">모델 실행 실패</strong><p className="mt-1 break-words">{translateError(runtime.error)}</p><p className="mt-1 text-rose-600">{runtimeFailure==='memory'?'메모리가 부족할 수 있습니다. 더 작은 양자화 모델을 선택해 보세요.':runtimeFailure==='file'?'모델 파일을 찾을 수 없거나 손상되었을 수 있습니다. 설치 목록과 파일을 확인하세요.':runtimeFailure==='engine'?'실행 엔진을 사용할 수 없습니다. 로컬 실행 엔진의 설치 상태를 확인하세요.':'오류 내용을 확인하고 실행 상태를 다시 조회해 보세요.'}</p></div></div><div className="flex shrink-0 flex-wrap gap-2">{runtimeFailure==='memory'&&smallerOption&&(runtime.id===existing?.id||runtime.id===selectedJob?.installedId)&&<Button variant="bare" type="button" className={btn} disabled={Boolean(working)} onClick={()=>{
 setView('search');setVariant(smallerOption.id);setError('')
 if(!smaller.fits)return
 const completed=jobs.filter(j=>j.repo===detail?.id&&j.optionId===smallerOption.id&&j.status==='completed').sort((a,b)=>(b.startedAt||'').localeCompare(a.startedAt||''))[0]
 const ready=installed.find(m=>m.id===completed?.installedId)
 if(ready)void load(ready.id)
 else void requestDownload(true,smallerOption)
}}>작은 양자화 {smaller.fits?'다운로드·실행':'선택'} · {fmt(smallerOption.bytes)}{smaller.fits?'':' (메모리 확인 필요)'}</Button>}{runtimeFailure==='memory'&&(!smallerOption||(runtime.id!==existing?.id&&runtime.id!==selectedJob?.installedId))&&<Button variant="bare" type="button" className={btn} onClick={()=>{setView('search');setError('')}}>작은 모델 찾기</Button>}{runtimeFailure==='file'&&<Button variant="bare" type="button" className={btn} onClick={()=>{setView('installed');setError('')}}>설치 목록 확인</Button>}{runtimeFailure==='engine'&&<Button variant="bare" type="button" className={btn} onClick={()=>{queueEngineSetup(installed.find(m=>m.id===runtime.id));setError('')}}>실행 엔진 확인</Button>}<Button variant="bare" type="button" className={btn} onClick={()=>void refresh()}><RefreshCw size={13}/>상태 확인</Button>{activeModel&&<Button variant="bare" type="button" className={btn} disabled={Boolean(working)} onClick={()=>void load(activeModel.id)}><Play size={13}/>다시 실행</Button>}</div></div>}
  {runtime.status==='ready'&&<div className="flex flex-wrap items-center justify-between gap-3 border-b border-emerald-100 bg-[#f2fbf7] px-4 py-3">
   <span className="flex min-w-0 items-center gap-2 text-[12px]"><Check size={16} className="text-emerald-600"/><strong className="truncate">{runtime.name||runtime.repo}</strong><span className="text-emerald-700">{t('models.ui.running')}</span></span>
   <div className="flex gap-2"><Button variant="bare" type="button" className={btn} disabled={!runtime.model} onClick={()=>{if(runtime.model)window.location.hash='#/chat?'+new URLSearchParams({provider:'dominion-native',model:runtime.model})}}><ArrowRight size={14}/> {t('models.chat')}</Button><Button variant="bare" type="button" className={btn} onClick={()=>void unload()} disabled={Boolean(working)}><Square size={13}/> {t('models.stop')}</Button></div>
  </div>}
  {view==='installed'&&<div className="p-4">
   <div className="mb-3 flex flex-wrap items-center justify-between gap-2"><h3 className="text-[13px] font-bold">{t('models.installedTitle')}</h3><Button variant="bare" type="button" className={btn} onClick={()=>setView('import')}><FolderOpen size={14}/> {t('models.importFile')}</Button></div>
   {installed.length?<div className="divide-y divide-[#EFE9DF] rounded-lg border border-[#EFE9DF]">{installed.map(m=><div key={m.id} className="flex flex-wrap items-center gap-3 p-3">
    <div className="grid size-9 shrink-0 place-items-center rounded-lg bg-[#F3EEE5] text-[#A94E30]"><Cpu size={17}/></div>
    <div className="min-w-0 flex-1"><div className="break-all text-[12px] font-semibold text-[#24211D]">{m.repo}</div><div className="mt-1 text-[11px] text-[#8A8176]">{m.format.toUpperCase()} · {fmt(m.bytes)} · {m.source==='external'?t('models.source.external'):m.source==='lmstudio'?t('models.source.lmstudio'):t('models.source.downloaded')}{m.externalPath?' · '+shorten(m.externalPath,65):''}</div>{modelPreflight(m,runtime.capabilities).status==='blocked'&&<div className="mt-1 text-[11px] font-semibold text-rose-700">실행기 필요 · {modelPreflight(m,runtime.capabilities).engine}</div>}{modelPreflight(m,runtime.capabilities).status==='warning'&&<div className="mt-1 text-[11px] font-semibold text-amber-700">메모리 여유 부족 가능 · 실행 전 확인</div>}</div>
    <div className="flex items-center gap-1.5">{runtime.status==='ready'&&runtime.id===m.id?<span className="mr-1 rounded bg-emerald-50 px-2 py-1 text-[10px] font-semibold text-emerald-700">{t('models.inUse')}</span>:<Button variant="bare" type="button" className={btn+' !border-[#C96442] !bg-[#C96442] !text-white hover:!bg-[#A94E30]'} disabled={Boolean(working)||runtime.status==='loading'} onClick={()=>void load(m.id)}><Play size={13}/>{working==='load'+m.id?t('models.loadingModel'):t('models.run')}</Button>}
     <Button variant="bare" type="button" className="rounded-md p-2 text-[#A79C8E] hover:bg-rose-50 hover:text-rose-600" aria-label={t('models.delete')+' '+m.repo} disabled={Boolean(working)||m.source==='lmstudio'||(runtime.id===m.id&&runtime.status==='ready')} title={m.source==='lmstudio'?unsupported:undefined} onClick={()=>void remove(m)}><Trash2 size={14}/></Button>
    </div>
   </div>)}</div>:<div className="flex min-h-48 flex-col items-center justify-center gap-3 rounded-lg border border-dashed border-[#E7E0D5] bg-[#FAF7F1] px-4 py-7 text-center"><HardDrive size={25} className="text-[#B8AFA2]"/><strong className="text-[13px]">{t('models.emptyTitle')}</strong><p className="text-[11px] text-[#8A8176]">{t('models.emptyDesc')}</p><div className="flex flex-wrap gap-2"><Button variant="bare" type="button" className={btn} onClick={()=>setView('import')}>{t('models.connectExisting')}</Button><Button variant="bare" type="button" className={btn} onClick={()=>setView('search')}>{t('models.downloadModels')}</Button></div></div>}
  </div>}
  {view==='import'&&<div className="max-w-3xl p-4">
   {pendingEngineLaunch&&<div role="status" className="mb-4 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-[#D9DFC8] bg-[#F6F8F0] p-3">
    <div className="min-w-0 flex-1"><strong className="text-[12px] text-[#3A342D]">실행 이어가기 · {pendingEngineModel?.label||pendingEngineLaunch.id}</strong>
     <p className="mt-1 text-[11px] leading-5 text-[#6F675D]">{!pendingEngineModel?'모델을 설치 목록에서 찾지 못했습니다. 목록을 새로고침한 후 확인하세요.':pendingEngineCheck?.status==='blocked'?((pendingEngineItem?.name as string)||pendingEngineCheck.engine)+' 실행기가 필요합니다. 설치가 끝나면 바로 실행할 수 있습니다.':pendingEngineCheck?.status==='warning'?'실행기는 준비됐습니다. 메모리 여유가 적을 수 있으니 실행 시 확인하세요.':'실행기 상태를 다시 확인한 후 원래 모델을 실행할 수 있습니다.'}</p>
     {installing&&<p className="mt-1 text-[11px] text-[#A06C31]">실행기 설치 진행 중 · 완료 후 상태가 자동 갱신됩니다.</p>}
    </div>
    <div className="flex flex-wrap items-center gap-2">
     {pendingEngineModel&&pendingEngineCheck?.status==='blocked'&&pendingEngineItem?.installable&&!installing&&<Button variant="bare" type="button" className={btn} disabled={Boolean(working)} onClick={()=>void safe('install-'+pendingEngineId,()=>post('/api/model-library/runtime/install',{engine:pendingEngineId}))}><ArrowDownToLine size={13}/>필요한 실행기 설치</Button>}
     {pendingEngineModel&&pendingEngineCheck?.status==='blocked'&&pendingEngineItem?.url&&!pendingEngineItem?.installable&&<a className={btn} href={pendingEngineItem.url} target="_blank" rel="noreferrer">실행기 받기 <ExternalLink size={12}/></a>}
     {pendingEngineModel&&pendingEngineCheck?.status!=='blocked'&&<Button variant="bare" type="button" className={btn+' !border-[#C96442] !bg-[#C96442] !text-white'} disabled={Boolean(working)||installing||runtime.status==='loading'} onClick={()=>void continueEngineLaunch()}><Play size={13}/>원래 모델 실행</Button>}
     <Button variant="bare" type="button" className={btn} onClick={()=>setPendingEngineLaunch(null)}><X size={13}/>대기 취소</Button>
    </div>
   </div>}
   <h3 className="text-[13px] font-bold">{t('models.importTitle')}</h3>
   <p className="mt-1 text-[11px] text-[#7C7469]">{t('models.importDesc')}</p>
   <label className="mt-4 block text-[11px] font-medium text-[#5C544A]">{t('models.path')}</label>
   <div className="mt-1.5 flex flex-col gap-2 sm:flex-row"><TextInput className="!flex-1" value={localPath} onChange={e=>setLocalPath(e.target.value)} placeholder="/models/model.gguf · /models/mlx-model"/><Button variant="bare" type="button" className={btn} disabled={!localPicker||picking} onClick={()=>void chooseLocal()}><FolderOpen size={14}/> {t('models.pickFile')}</Button><Button variant="bare" type="button" className={btn} disabled={!localPicker||picking} onClick={()=>void chooseFolder()}><FolderOpen size={14}/> {t('models.pickFolder')}</Button></div>
   <p className="mt-2 text-[11px] text-[#8A8176]">{localPicker?t('models.pickHintLocal'):t('models.pickHintRemote')}</p>
   <Button variant="bare" type="button" disabled={!localPath.trim()||Boolean(working)} onClick={()=>void importLocal()} className="mt-5 flex h-9 items-center gap-2 rounded-lg bg-[#C96442] px-4 text-[12px] font-semibold text-white hover:bg-[#A94E30] disabled:opacity-40">{working==='import'?<LoaderCircle className="animate-spin" size={15}/>:<Check size={15}/>}{t('models.file.register')}</Button>
   <div className="mt-7 border-t border-[#EFE9DF] pt-5">
    <div className="mb-3"><strong className="text-[13px] text-[#24211D]">{t('models.runtimes')}</strong><p className="mt-1 text-[11px] text-[#818a96]">{t('models.runtimesDesc')}</p></div>
    <div className="grid gap-3 lg:grid-cols-2">{runtimes.map((item:any)=>{
    const setup=runtime.setup
    const active=Boolean(item.installing)
    const percent=Math.round((setup?.downloaded||0)/Math.max(1,setup?.total||0)*100)
    return <div className="flex min-w-0 flex-col gap-3 rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] p-4" key={item.id}>
     <div className="flex items-start justify-between gap-3">
      <div className="flex min-w-0 items-center gap-3">
       <span className="grid size-10 shrink-0 place-items-center rounded-lg bg-[#F7E8E1] text-[#C96442]"><Cpu size={20}/></span>
       <div className="min-w-0"><strong className="block text-[13px] text-[#24211D]">{item.name}</strong><small className="text-[10px] text-[#8A8176]">{(item.formats||[]).join(' · ')} · {item.installMethod}</small></div>
      </div>
      <span className={'shrink-0 rounded px-2 py-0.5 text-[10px] font-semibold '+(item.installed?'bg-emerald-50 text-emerald-700':'bg-[#F3EEE5] text-[#7C7469]')}>{item.installed?t('models.runtime.installed'):t('models.runtime.missing')}</span>
     </div>
     <p className="text-[11px] leading-5 text-[#7C7469]">{t(('models.runtime.desc.'+item.id) as any)||item.description||''}</p>
     {item.path&&<code className="block max-w-full truncate rounded bg-[#F7F2EA] px-2 py-1 text-[10px] text-[#8A8176]" title={item.path}>{item.path}</code>}
     {item.version&&<small className="text-[10px] text-[#9C9388]">{t('models.runtime.version',{version:item.version})}</small>}
     {active?<div><div className="flex items-center justify-between text-[10px] text-[#7C7469]"><span>{item.name} · {t('models.runtime.installing')}</span><span>{setup?.total?percent+'%':''}</span></div><div className="mt-1 h-1.5 overflow-hidden rounded-full bg-[#E7E0D5]"><div className="h-full bg-[#C96442]" style={{width:Math.max(4,percent)+'%'}}/></div></div>:
      <div className="mt-auto flex items-center justify-between gap-2 border-t border-[#EFE9DF] pt-3">
       <small className="text-[10px] text-[#9C9388]">{!item.installed&&!item.installable?t('models.runtime.external'):''}</small>
       {!item.installed&&(item.installable
        ?<Button variant="bare" type="button" className={btn+' !border-[#C96442] !bg-[#C96442] !text-white hover:!bg-[#A94E30]'} disabled={Boolean(working)||installing} onClick={()=>void safe('install-'+item.id,()=>post('/api/model-library/runtime/install',{engine:item.id}))}>{t('models.runtime.install')}</Button>
        :item.url?<a className={btn} href={item.url} target="_blank" rel="noreferrer">{t('models.runtime.download')} <ExternalLink size={12}/></a>:null)}
      </div>}
    </div>
   })}</div>
   </div>
  </div>}
  {view==='search'&&<div className="grid min-h-[560px] grid-cols-1 lg:grid-cols-[400px_minmax(0,1fr)]">
   <div className="min-w-0 border-b border-[#E7E0D5] lg:border-b-0 lg:border-r">
    <div className="border-b border-[#EFE9DF] p-4">
     <div className="flex h-11 items-center gap-2 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] px-3 focus-within:border-[#C96442]"><Search size={17} className="text-[#A79C8E]"/><input value={q} onChange={e=>setQ(e.target.value)} placeholder={t("models.searchPlaceholder")} className="h-full w-full min-w-0 bg-transparent text-[14px] outline-none placeholder:text-[#A79C8E]"/>{q&&<Button variant="bare" type="button" aria-label={t('models.ui.clearSearch')} onClick={()=>setQ('')} className="rounded p-1 text-[#A79C8E] hover:bg-[#F3EEE5]"><X size={14}/></Button>}</div>
     <div className="mt-3 flex flex-wrap items-center gap-1.5">{[{id:'all',label:t('models.all')},{id:'gguf',label:'GGUF'},{id:'mlx',label:'MLX'}].map(item=><Button key={item.id} variant={format===item.id?'primary':'ghost'} onClick={()=>{userChoseFormat.current=true;setFormat(item.id)}} className="!px-3 !py-1.5 !text-[12px]">{item.label}</Button>)}{preferredFormat&&<span className="ml-1 rounded bg-[#F7E8E1] px-2 py-0.5 text-[10px] font-semibold text-[#A94E30]">서버 추천 · {preferredFormat.toUpperCase()}</span>}<Button variant={chatOnly?'primary':'ghost'} aria-pressed={chatOnly} onClick={()=>setChatOnly(value=>!value)} className="!px-3 !py-1.5 !text-[12px]">{t('models.chatOnly')}</Button><SelectInput aria-label={t('models.size')} value={maxParams} onChange={e=>setMaxParams(e.target.value)} className="!w-auto !py-1.5 !text-[12px]"><option value="">{t('models.sizeAll')}</option><option value="1">{t('models.ui.sizeMax',{n:1})}</option><option value="2">{t('models.ui.sizeMax',{n:2})}</option><option value="3">{t('models.ui.sizeMax',{n:3})}</option><option value="4">{t('models.ui.sizeMax',{n:4})}</option><option value="8">{t('models.ui.sizeMax',{n:8})}</option></SelectInput><SelectInput aria-label={t('models.sort')} value={sort} onChange={e=>setSort(e.target.value as typeof sort)} className="!ml-auto !w-auto !py-1.5 !text-[12px]"><option value="downloads">{t('models.sort.downloads')}</option><option value="likes">{t('models.sort.likes')}</option><option value="lastModified">{t('models.sort.updated')}</option></SelectInput></div>
     <div className="mt-2 flex items-center justify-between text-[11px] text-[#8A8176]"><span>{loading?t('models.searching'):t('models.resultCount',{count:catalog.length})}</span><span>{t('models.realtime')}</span></div>
    </div>
    <div className="flex items-center justify-between gap-2 border-b border-[#EFE9DF] bg-[#FFFDF9] px-4 py-2">
     <span className="text-[11px] font-medium text-[#7C7469]">모델 둘러보기 · {catalog.length}개</span>
     <div className="flex gap-1"><Button variant="bare" type="button" aria-pressed={catalogLayout==='cards'} onClick={()=>setCatalogLayout('cards')} className={'rounded-lg px-2.5 py-1 text-[11px] '+(catalogLayout==='cards'?'bg-[#F3EEE5] font-semibold':'text-[#8A8176]')}>카드</Button><Button variant="bare" type="button" aria-pressed={catalogLayout==='list'} onClick={()=>setCatalogLayout('list')} className={'rounded-lg px-2.5 py-1 text-[11px] '+(catalogLayout==='list'?'bg-[#F3EEE5] font-semibold':'text-[#8A8176]')}>목록</Button></div>
    </div>
    <div ref={catalogueScroll} className={"max-h-[610px] overflow-y-auto overscroll-contain "+(catalogLayout==='cards'?'grid grid-cols-1 gap-2 p-2 sm:grid-cols-2 lg:grid-cols-1 xl:grid-cols-2':'')}>
     {loading&&<div className="space-y-2 p-3">{[0,1,2,3].map(i=><div key={i} className="h-20 animate-pulse rounded-xl bg-[#FAF7F1]"/>)}</div>}
     {!loading&&catalog.map(m=><Button variant="bare" type="button" key={m.id} onClick={()=>setSelected(m.id)} className={'w-full text-left transition-colors hover:bg-[#FAF7F1] '+(catalogLayout==='cards'?'rounded-xl border border-[#E7E0D5] bg-white p-3 shadow-sm':'border-b border-[#EFE9DF] px-4 py-3.5')+' '+(selected===m.id?'bg-[#F7E8E1] ring-1 ring-[#C96442]':'')}><div className="flex items-start gap-3"><span className="grid size-9 shrink-0 place-items-center rounded-xl bg-[#F3EEE5] text-[13px] font-bold text-[#C96442]">{m.name.slice(0,1).toUpperCase()}</span><div className="min-w-0 flex-1"><div className="flex items-center gap-2"><strong className="truncate text-[13px] text-[#24211D]">{m.name}</strong>{m.gated&&<span className="rounded bg-amber-50 px-1.5 py-0.5 text-[9px] font-semibold text-amber-700">{t('models.ui.gated')}</span>}</div><div className="mt-0.5 truncate text-[11px] text-[#8A8176]">{m.author} · {m.task||m.library||'model'}</div><p className="mt-1 line-clamp-2 text-[11px] leading-5 text-[#5C544A]">{m.description||t('models.ui.loadingDesc')}</p><div className="mt-1.5 flex flex-wrap items-center gap-2 text-[10px] text-[#8A8176]"><span>↓ {compactNumber(m.downloads)}</span><span>♡ {compactNumber(m.likes)}</span><span>{updatedLabel(m.updatedAt)}</span><span className="rounded bg-[#F3EEE5] px-1.5 py-0.5 font-semibold">{/gguf/i.test(m.tags.join(' '))?'GGUF':'MLX'}</span></div></div></div></Button>)}
     {!loading&&!catalog.length&&<div className="p-8 text-center text-[12px] leading-6 text-[#7C7469]">{t('models.noResults')}</div>}
     {!loading&&catalog.length>0&&catalog.length>=limit&&limit<100&&<div className="p-3"><Button variant="secondary" className="w-full" onClick={()=>setLimit(100)}>{t('models.more',{count:catalog.length})}</Button></div>}
    </div>
   </div>
   <div ref={detailScroll} className="min-w-0 max-h-[610px] overflow-y-auto overscroll-contain p-4 pr-2">
    {!selected?<div className="flex min-h-52 flex-col items-center justify-center gap-3 text-[12px] text-[#9C9388]"><CloudDownload size={23}/>{t('models.selectModel')}</div>:!detail?<div className="flex h-44 items-center justify-center text-[12px] text-[#8A8176]"><LoaderCircle size={17} className="mr-2 animate-spin"/>{t('models.loadingDetail')}</div>:<>
     <div className="flex items-start justify-between gap-3"><div className="min-w-0"><h3 className="break-all text-[20px] font-bold tracking-[-.02em] text-[#24211D]">{detail.name||detail.id}</h3><div className="mt-1 break-all text-[11px] text-[#8A8176]">{detail.id}</div><div className="mt-2 flex flex-wrap items-center gap-2 text-[11px] text-[#7C7469]"><span>↓ {compactNumber(detail.downloads)}</span><span>♡ {compactNumber(detail.likes)}</span><span>{detail.architecture||t('models.ui.languageModel')}</span>{detail.license&&<span>{detail.license}</span>}<span>{t('models.ui.formats',{count:detail.options.length})}</span></div></div><a href={'https://huggingface.co/'+detail.id} target="_blank" rel="noreferrer" aria-label={t('models.ui.viewHf')} className="inline-flex shrink-0 items-center gap-1 rounded-md px-2 py-1.5 text-[11px] font-semibold text-[#C96442] hover:bg-[#F7E8E1]">Hugging Face <ExternalLink size={14}/></a></div>
     <div className="mt-4 grid grid-cols-2 gap-2 sm:grid-cols-4">
      <div className="rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] p-3"><div className="text-[10px] text-[#8A8176]">사용 가능한 형식</div><strong className="mt-1 block text-[13px] text-[#24211D]">{[...new Set(detail.options.map(o=>o.format.toUpperCase()))].join(' · ')||'—'}</strong></div>
      <div className="rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] p-3"><div className="text-[10px] text-[#8A8176]">양자화 선택지</div><strong className="mt-1 block text-[13px] text-[#24211D]">{new Set(detail.options.map(o=>o.quantization).filter(Boolean)).size}개</strong></div>
      <div className="rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] p-3"><div className="text-[10px] text-[#8A8176]">가장 작은 파일</div><strong className="mt-1 block text-[13px] text-[#24211D]">{detail.options.some(o=>o.bytes>0)?fmt(Math.min(...detail.options.filter(o=>o.bytes>0).map(o=>o.bytes))):'—'}</strong></div>
      <div className="rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] p-3"><div className="text-[10px] text-[#8A8176]">선택한 파일</div><strong className="mt-1 block text-[13px] text-[#24211D]">{option?fmt(option.bytes):'—'}</strong></div>
     </div>
     <div className="mt-5 text-[13px] font-semibold text-[#3A342D]">{t('models.downloadFiles')} <span className="ml-2 text-[11px] font-normal text-[#8A8176]">원하는 양자화 파일을 선택하세요</span></div>
     {detail.options.length>0&&<div className="mt-2 grid grid-cols-2 gap-2 sm:grid-cols-3">
      <div className="rounded-lg bg-[#FAF7F1] p-2.5"><div className="text-[10px] text-[#8A8176]">최소 다운로드 용량</div><strong className="text-[13px] text-[#3A342D]">{detail.options.some(o=>(format==='all'||o.format===format)&&o.bytes>0)?fmt(Math.min(...detail.options.filter(o=>(format==='all'||o.format===format)&&o.bytes>0).map(o=>o.bytes))):'정보 없음'}</strong></div>
      <div className="rounded-lg bg-[#FAF7F1] p-2.5"><div className="text-[10px] text-[#8A8176]">선택한 양자화</div><strong className="text-[13px] text-[#3A342D]">{option?.quantization||option?.format.toUpperCase()||'선택 전'}</strong></div>
      <div className="col-span-2 rounded-lg bg-[#FAF7F1] p-2.5 sm:col-span-1"><div className="text-[10px] text-[#8A8176]">선택한 용량</div><strong className="text-[13px] text-[#3A342D]">{option?fmt(option.bytes):'선택 전'}</strong></div>
     </div>}
     {recommendedOption&&<div className="mt-2 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-[#CFE4D4] bg-[#F3FAF4] p-3">
      <div className="min-w-0"><div className="text-[11px] font-semibold text-emerald-700">내 기기 추천 양자화</div><div className="mt-1 truncate text-[12px] font-semibold text-[#3A342D]">{recommendedOption.label} · {fmt(recommendedOption.bytes)}</div><p className="mt-1 text-[10px] text-[#6F806F]">{recommendationBasis} 기준 · 실행 시 컨텍스트와 엔진 추가 메모리가 필요할 수 있습니다.</p></div>
      <Button variant="bare" type="button" onClick={()=>setVariant(recommendedOption.id)} className="shrink-0 rounded-lg bg-emerald-700 px-3 py-2 text-[11px] font-semibold text-white">이 옵션 선택</Button>
     </div>}
     {option&&<div className="mt-2 rounded-xl border border-[#E7E0D5] bg-white p-3">
      <div className="flex flex-wrap items-center justify-between gap-2"><strong className="text-[12px] text-[#3A342D]">내 기기 메모리 적합도</strong><span className={'rounded-full px-2 py-1 text-[11px] font-semibold '+(memoryFit==='comfortable'?'bg-emerald-50 text-emerald-700':memoryFit==='tight'?'bg-amber-50 text-amber-700':memoryFit==='insufficient'?'bg-red-50 text-red-700':'bg-[#F3EEE5] text-[#7C7469]')}>{memoryFit==='comfortable'?'여유 있음':memoryFit==='tight'?'빠듯함':memoryFit==='insufficient'?'메모리 부족 가능':'정보 확인 중'}</span></div>
      <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-[#7C7469]"><span>사용 가능 메모리: {availableMemory?fmt(availableMemory):'정보 없음'}</span><span>전체 메모리: {totalMemory?fmt(totalMemory):'정보 없음'}</span><span>예상 필요 용량: {fmt(memoryHeadroom)}</span></div>
      <div className="mt-2 flex flex-wrap items-center gap-2 border-t border-[#EFE9DF] pt-2 text-[11px] text-[#7C7469]"><span>GPU: {gpuKind||'정보 없음'}</span><span>여유 GPU 메모리: {gpuMemory?fmt(gpuMemory):'정보 없음'}</span><span className={'rounded px-2 py-0.5 font-semibold '+(gpuFit==='comfortable'?'bg-emerald-50 text-emerald-700':gpuFit==='tight'?'bg-amber-50 text-amber-700':gpuFit==='partial'?'bg-[#F3EEE5] text-[#7C7469]':'bg-[#F3EEE5] text-[#8A8176]')}>{gpuFit==='comfortable'?'GPU 전체 적재 여유':gpuFit==='tight'?'GPU 적재 빠듯':gpuFit==='partial'?'GPU 부분 적재 가능성':'GPU 적합도 정보 없음'}</span></div>
      <p className="mt-2 text-[10px] leading-4 text-[#8A8176]">파일 크기의 약 1.3배를 기준으로 한 참고치입니다. 시스템 RAM과 GPU 메모리는 합산하지 않습니다. GPU 부분 적재는 실행 엔진의 오프로딩 지원 여부에 따라 달라집니다.</p>
     </div>}
     <div className="mt-2 max-h-56 overflow-auto rounded-xl border border-[#E7E0D5]">{detail.options.filter(o=>format==='all'||o.format===format).map(o=><Button variant="bare" type="button" key={o.id} onClick={()=>setVariant(o.id)} className={'flex w-full items-center gap-3 border-b border-[#EFE9DF] px-3.5 py-3.5 text-left last:border-0 '+(variant===o.id?'bg-[#F7E8E1]':'hover:bg-[#FAF7F1]')}><span className="rounded bg-[#F3EEE5] px-2 py-1 text-[10px] font-semibold text-[#7C7469]">{o.format.toUpperCase()}</span><span className="min-w-0 flex-1 text-[13px] font-semibold text-[#3A342D]">{o.label}<span className="ml-2 text-[10px] font-normal text-[#8A8176]">{o.quantization&&o.quantization!==o.label?o.quantization:''}{o.shards>1?' · '+t('models.ui.shardFiles',{count:o.shards}):''}</span></span><span className="text-[12px] text-[#5C544A]">{fmt(o.bytes)}</span>{variant===o.id&&<Check size={15} className="text-[#C96442]"/>}</Button>)}</div>
     {!detail.options.length&&<p className="mt-3 text-[12px] text-amber-700">{t('models.noGguf')}</p>}
     {selectedPreflight&&<div className={'mt-3 flex flex-wrap items-center justify-between gap-3 rounded-xl border p-3 text-[11px] '+(selectedPreflight.status==='blocked'?'border-rose-200 bg-rose-50 text-rose-800':selectedPreflight.status==='warning'?'border-amber-200 bg-amber-50 text-amber-900':'border-[#E7E0D5] bg-[#FAF7F1] text-[#5C544A]')}>
      <div className="min-w-0"><strong className="text-[12px]">실행 전 점검 · {selectedPreflight.status==='blocked'?'실행기 필요':selectedPreflight.status==='warning'?'메모리 주의':selectedPreflight.status==='ready'?'준비 상태 양호':'측정 정보 확인 중'}</strong>
      <p className="mt-1 leading-5">{selectedPreflight.status==='blocked'?selectedPreflight.engine+' 실행기가 없습니다. 먼저 설치하거나 기존 실행기를 연결하세요.':selectedPreflight.status==='warning'?'선택한 모델의 파일 크기에 비해 사용 가능한 메모리가 빠듯할 수 있습니다. 실제 사용량은 컨텍스트와 오프로딩 방식에 따라 달라집니다.':selectedPreflight.status==='ready'?'실행기 및 모델 용량에 대한 기본 점검을 통과했습니다.':'실행기 또는 사용 가능 메모리 정보가 부족합니다. 실행할 때 서버 상태를 다시 검사합니다.'}</p></div>
      {selectedPreflight.status==='blocked'&&<Button variant="bare" type="button" className={btn} onClick={()=>queueEngineSetup(existing)}><Cpu size={13}/>실행기 설치·확인</Button>}
      {selectedPreflight.status==='warning'&&smallerOption&&<Button variant="bare" type="button" className={btn} onClick={()=>setVariant(smallerOption.id)}>작은 양자화 선택</Button>}
     </div>}
     {option&&<div className="mt-4 flex flex-wrap items-center gap-3 rounded-xl bg-[#FAF7F1] p-3">
      <span className="text-[12px] text-[#5C544A]">{t('models.sizeLabel')} <strong>{fmt(option.bytes)}</strong></span>
      {existing?<><Button variant="bare" type="button" className={btn+' !border-[#C96442] !bg-[#C96442] !text-white'} disabled={Boolean(working)||runtime.status==='loading'||(runtime.status==='ready'&&runtime.id===existing.id)} onClick={()=>void load(existing.id)}><Play size={13}/>{working==='load'+existing.id?t('models.loadingModel'):runtime.status==='ready'&&runtime.id===existing.id?t('models.inUse'):t('models.run')}</Button>{runtime.status==='ready'&&runtime.id===existing.id&&runtime.model&&<Button variant="bare" type="button" className={btn} onClick={()=>{window.location.hash='#/chat?'+new URLSearchParams({provider:'dominion-native',model:runtime.model!})}}><ArrowRight size={14}/>{t('models.chat')}</Button>}<Button variant="bare" type="button" className={btn} onClick={()=>setView('installed')}><Check size={14}/>{t('models.installedView')}</Button></>:
       <><Button variant="bare" type="button" className="flex h-8 items-center gap-2 rounded-lg bg-[#C96442] px-3 text-[11px] font-semibold text-white disabled:opacity-40" disabled={Boolean(working)||['resolving','downloading','paused'].includes(selectedJob?.status||'')||detail.gated} onClick={()=>void requestDownload(false)}><ArrowDownToLine size={14}/>{working==='download'?t('models.requesting'):selectedJob?.status==='downloading'?t('models.downloading'):t('models.downloadStart')}</Button><Button variant="bare" type="button" className={btn} disabled={Boolean(working)||detail.gated||Boolean(pendingLaunch&&pendingLaunch.repo===detail.id&&pendingLaunch.optionId===option.id)} onClick={()=>void requestDownload(true)}><Play size={13}/>{pendingLaunch?.repo===detail.id&&pendingLaunch.optionId===option.id?'자동 실행 대기 중':['paused','failed','cancelled'].includes(selectedJob?.status||'')?'이어받고 실행':'다운로드 후 실행'}</Button></>}
      {detail.gated&&<span className="text-[11px] text-amber-700">{t('models.approval')}</span>}
     </div>}
     
     {selectedJob?.status==='completed'&&!existing&&<div className="mt-2 rounded-lg border border-emerald-200 bg-emerald-50 px-3 py-2 text-[11px] text-emerald-800">다운로드가 완료되었습니다. 설치 목록을 새로고침하면 실행할 수 있습니다. <Button variant="bare" type="button" className="ml-2 underline" onClick={()=>void refresh()}>목록 새로고침</Button></div>}
     {selectedJob&&<div className="mt-3 rounded-lg border border-[#EFE9DF] p-3 text-[11px]"><div className="flex justify-between gap-3"><span>{selectedJob.status} {selectedJob.current&&'· '+shorten(selectedJob.current,30)}</span><span>{fmt(selectedJob.downloaded)} / {fmt(selectedJob.total)}</span></div><div className="mt-2 h-1.5 overflow-hidden rounded-full bg-[#F3EEE5]"><div className="h-full bg-[#C96442]" style={{width:Math.min(100,selectedJob.downloaded/Math.max(1,selectedJob.total)*100)+'%'}}/></div>{selectedJob.error&&<p className="mt-2 text-rose-600">{translateError(selectedJob.error)}</p>}<div className="mt-2 flex gap-2">{selectedJob.status==='downloading'&&<Button variant="bare" type="button" className={btn} onClick={()=>void safe('pause',()=>post('/api/model-library/jobs/'+selectedJob.id+'/pause',{}))}><Pause size={13}/> {t('models.pause')}</Button>}{['paused','failed','cancelled'].includes(selectedJob.status)&&<Button variant="bare" type="button" className={btn} onClick={()=>void safe('resume',()=>post('/api/model-library/jobs/'+selectedJob.id+'/resume',{}))}><Play size={13}/> {t('models.resume')}</Button>}{['resolving','downloading','paused'].includes(selectedJob.status)&&<Button variant="bare" type="button" className={btn} onClick={()=>void safe('cancel',()=>post('/api/model-library/jobs/'+selectedJob.id+'/cancel',{}))}><Square size={13}/> {t('models.download.cancel')}</Button>}</div></div>}
         <div className="mt-5 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-4"><div className="flex items-center justify-between gap-2"><strong className="text-[13px] text-[#3A342D]">{t('models.readme')}</strong><span className="text-[10px] font-semibold text-[#A79C8E]">HUGGING FACE</span></div><p className="mt-2 whitespace-pre-wrap text-[12px] leading-6 text-[#5C544A]">{readmePlain.slice(0,1200)||readmeSummary||t('models.readmeEmpty')}</p>{readmePlain.length>1200&&<details className="mt-2"><summary className="cursor-pointer text-[11px] font-semibold text-[#C96442]">{t('models.showReadme')}</summary><pre className="mt-2 max-h-72 overflow-auto whitespace-pre-wrap text-[11px] leading-5 text-[#5C544A]">{readmePlain.slice(0,14000)}</pre></details>}</div>
     </>}
   </div>
  </div>}
  {(installing||runtime.setup?.status==='failed'||(runtime.status==='installing'))&&<div role="status" className="border-t border-[#EFE9DF] bg-[#F3EEE5] px-4 py-3 text-[11px] text-[#5C544A]">
    <div className="flex flex-wrap items-center justify-between gap-2"><strong>{runtime.setup?.status==='failed'?t('models.installFailed'):runtime.setup?.engine==='mlx'?t('models.installingMlx'):runtime.setup?.status==='verifying'?t('models.verifying'):runtime.setup?.status==='extracting'?t('models.extracting'):t('models.installingRunner')}</strong><span>{runtime.setup?.tag||''} {runtime.setup?.total?installerPercent+'%':''}</span></div>
    {installing&&<div className="mt-2 h-1.5 overflow-hidden rounded-full bg-[#E7E0D5]"><div className="h-full bg-[#C96442]" style={{width:installerPercent+'%'}}/></div>}
    {runtime.setup?.error&&<p className="mt-2 text-rose-700">{translateError(runtime.setup.error)}</p>}
    {runtime.setup?.file&&<p className="mt-1 truncate text-[#9C9388]">{runtime.setup.file}</p>}
   </div>}

 </section>
}
