import {useCallback,useEffect,useLayoutEffect,useRef,useState} from 'react'
import {ArrowDownToLine,ArrowRight,Check,ChevronDown,CircleAlert,CloudDownload,Cpu,ExternalLink,FolderOpen,HardDrive,LoaderCircle,Pause,Play,RefreshCw,Search,Square,Trash2,X} from 'lucide-react'
import {Button,SelectInput,TextInput} from './Primitives'
import {get,post,del} from '../api'
import {t,useLocale} from '../i18n'
import {translateError} from '../i18n/errors'
type Catalog={id:string;author:string;name:string;library:string;createdAt?:string;downloads:number;likes:number;gated:boolean;updatedAt:string;description:string;task:string;tags:string[]}
type Option={id:string;format:'gguf'|'mlx';label:string;quantization:string;bytes:number;files:string[];shards:number}
type Detail=Catalog&{revision:string;license:string;architecture:string;options:Option[];readme:string}
type Installed={id:string;repo:string;optionId?:string;source?:string;externalPath?:string;format:'gguf'|'mlx';label:string;bytes:number;installedAt:string}
type Job={id:string;repo:string;optionId:string;status:string;downloaded:number;total:number;current:string;error:string;installedId?:string}
type Runtime={status:string;id?:string;repo?:string;engine?:string;name?:string;model?:string;error?:string;setup?:{status:string;engine?:string;backend:string;downloaded:number;total:number;tag:string;file?:string;error:string};capabilities?:{gguf:{available:boolean;backend?:string;installable?:boolean;path?:string|null;installMethod?:string};mlx:{available:boolean;backend?:string;installable?:boolean;standalone?:boolean;path?:string|null;installMethod?:string};memory:{total:number;free:number;platform?:string;arch?:string};gpu?:{kind:string;free:number}|null}}
const fmt=(v=0)=>v>=1073741824?(v/1073741824).toFixed(1)+' GB':v>=1048576?(v/1048576).toFixed(0)+' MB':v>=1024?(v/1024).toFixed(0)+' KB':String(v)+' B'
const shorten=(v:string,n=45)=>v.length>n?v.slice(0,n-3)+'…':v
const compactNumber=(v=0)=>v>=1e6?(v/1e6).toFixed(1)+'M':v>=1e3?(v/1e3).toFixed(1)+'K':String(v)
const cleanMarkdown=(value='')=>String(value).replace(/^---[\s\S]*?---/,'').replace(/```[\s\S]*?```/g,' ').replace(/\[!\[[^\]]*\]\([^)]*\)\]\([^)]*\)/g,' ').replace(/!\[[^\]]*\]\([^)]*\)/g,' ').replace(/\[([^\]]+)\]\([^)]*\)/g,'$1').replace(/^\s*\|.*$/gm,' ').replace(/^\s*[-:]{3,}\s*$/gm,' ').replace(/<[^>]+>/g,' ').replace(/^#{1,6}\s*/gm,'').replace(/[*_`>]/g,'').replace(/\n{3,}/g,'\n\n').trim()
const updatedLabel=(value:string)=>value?new Date(value).toLocaleDateString():''
const viewTabs=[{id:'installed',label:'models.tab.installed',icon:HardDrive},{id:'search',label:'models.tab.search',icon:Search},{id:'import',label:'models.tab.import',icon:FolderOpen}] as const
export function ModelExplorer(){
 useLocale()
 const [view,setView]=useState<'installed'|'search'|'import'>('installed')
 const [installed,setInstalled]=useState<Installed[]>([])
 const [jobs,setJobs]=useState<Job[]>([])
 const [runtime,setRuntime]=useState<Runtime>({status:'idle'})
 const [q,setQ]=useState('')
 const [format,setFormat]=useState(()=>/Mac/i.test(navigator.platform)?'mlx':'gguf')
 const [catalog,setCatalog]=useState<Catalog[]>([])
 const [limit,setLimit]=useState(80)
 const [selected,setSelected]=useState('')
 const [detail,setDetail]=useState<Detail|null>(null)
 const [variant,setVariant]=useState('')
 const [loading,setLoading]=useState(false)
 const [working,setWorking]=useState('')
 const [error,setError]=useState('')
 const [localPath,setLocalPath]=useState('')
 const [picking,setPicking]=useState(false)
 const detailScroll=useRef<HTMLDivElement>(null)
 const catalogueScroll=useRef<HTMLDivElement>(null)
 const localPicker=['127.0.0.1','localhost','[::1]'].includes(window.location.hostname)
 const unsupported=t('models.ui.unsupported')
 const [context,setContext]=useState(4096)
 const refresh=useCallback(async()=>{
  const results=await Promise.allSettled([get<Installed[]>('/api/model-library/installed'),get<Job[]>('/api/model-library/jobs'),get<Runtime>('/api/model-library/runtime')])
  if(results[0].status==='fulfilled')setInstalled(results[0].value)
  if(results[1].status==='fulfilled')setJobs(results[1].value)
  if(results[2].status==='fulfilled')setRuntime(results[2].value)
 },[])
 useEffect(()=>{
  let live=true;
  let timer:ReturnType<typeof setTimeout>;
  const update=async()=>{await refresh();if(live)timer=setTimeout(update,1800)};
  void update();
  return()=>{live=false;clearTimeout(timer)};
 },[refresh])
 useLayoutEffect(()=>{if(detailScroll.current)detailScroll.current.scrollTop=0},[selected])
 useLayoutEffect(()=>{if(catalogueScroll.current)catalogueScroll.current.scrollTop=0},[q])
 useEffect(()=>{
  if(view!=='search')return
  const controller=new AbortController()
  const timer=setTimeout(async()=>{
   setLoading(true)
   try{
    const data=await get<Catalog[]>('/api/model-library/catalog?'+new URLSearchParams({q,limit:String(limit),format:format==='mlx'?'mlx':'gguf'}),controller.signal)
    if(!controller.signal.aborted){setCatalog(data);setSelected(s=>data.some(m=>m.id===s)?s:'');setError('')}
   }catch(e){if(!controller.signal.aborted)setError((e as Error).message)}
   finally{if(!controller.signal.aborted)setLoading(false)}
  },300)
  return()=>{controller.abort();clearTimeout(timer)}
 },[view,q,limit,format])
 useEffect(()=>{setLimit(80)},[q])
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
 const selectedJob=jobs.find(j=>j.repo===detail?.id&&j.optionId===option?.id&&['resolving','downloading','paused','failed','cancelled','completed'].includes(j.status))
 const installedId=selectedJob?.installedId??
  (selectedJob?.status==='completed'?selectedJob.optionId.split('/').at(-1):undefined)
 const existing=installed.find(m=>m.id===installedId)
 const activeModel=installed.find(m=>m.id===runtime.id)
 const needEngine=runtime.capabilities?.gguf.available===false;
 const installing=['checking','downloading','extracting','verifying'].includes(runtime.setup?.status||'');
 const recommendedBackend=runtime.capabilities?.memory.platform==='win32'&&runtime.capabilities.memory.arch==='x64'?'cuda':'cpu';
 const installerPercent=Math.round((runtime.setup?.downloaded||0)/Math.max(1,runtime.setup?.total||0)*100);
 const safe=async(key:string,action:()=>Promise<unknown>)=>{
  setWorking(key);setError('')
  try{await action();await refresh()}
  catch(e){setError((e as Error).message)}
  finally{setWorking('')}
 }
 const importLocal=()=>safe('import',async()=>{
  const result=await post<Installed>('/api/model-library/import',{filePath:localPath.trim(),reference:true})
  setLocalPath('');setView('installed');return result
 })
 const chooseLocal=async()=>{
  if(picking||!localPicker)return;
  setPicking(true);setError('');
  try{const {path}=await post<{path?:string}>('/api/model-library/pick-file',{});if(path)setLocalPath(path)}
  catch(e){setError((e as Error).message)}finally{setPicking(false)}
 }
 const load=(id:string)=>safe('load'+id,()=>post('/api/model-library/runtime/load',{id,context}))
 const unload=()=>safe('unload',()=>post('/api/model-library/runtime/unload',{}))
 const remove=async(m:Installed)=>{
  if(!window.confirm(m.source==='external'?t('models.ui.removeExternal'):t('models.ui.removeDownloaded')))return
  await safe('remove'+m.id,()=>del('/api/model-library/installed/'+encodeURIComponent(m.id)))
 }
 const btn='inline-flex h-8 items-center justify-center gap-2 rounded-lg border border-[#E7E0D5] bg-white px-3 text-[11px] font-semibold text-[#3A342D] hover:bg-[#f5f7fa] disabled:opacity-40'
 return <section className="overflow-hidden rounded-xl border border-[#E7E0D5] bg-white">
  <div className="flex flex-wrap items-center justify-between gap-3 border-b border-[#EFE9DF] px-4 py-3">
   <div className="flex flex-wrap items-center gap-1">{viewTabs.map(tab=>{const Icon=tab.icon;return <button key={tab.id} type="button" onClick={()=>{setView(tab.id);setError('')}} className={'flex h-8 items-center gap-1.5 rounded-lg px-3 text-[12px] font-semibold '+(view===tab.id?'bg-[#F7E8E1] text-[#A94E30]':'text-[#7C7469] hover:bg-[#F3EEE5]')}><Icon size={14}/>{t(tab.label)}{tab.id==='installed'&&<span className="ml-1 rounded bg-white/75 px-1.5 text-[10px]">{installed.length}</span>}</button>})}</div>
   <div className="flex items-center gap-2 text-[11px] text-[#7C7469]">{runtime.capabilities?.gguf.installable&&<button type="button" className={btn} disabled={Boolean(working)||installing} onClick={()=>void safe('install',()=>post('/api/model-library/runtime/install',{}))}>{working==='install'||installing?<LoaderCircle size={13} className="animate-spin"/>:<ArrowDownToLine size={13}/>}{t('models.installRunner')}</button>}{runtime.capabilities?.mlx.installable&&<button type="button" className={btn} disabled={Boolean(working)||installing} onClick={()=>void safe('install-mlx',()=>post('/api/model-library/runtime/install',{engine:'mlx'}))}>{working==='install-mlx'||installing?<LoaderCircle size={13} className="animate-spin"/>:<ArrowDownToLine size={13}/>}{t('models.installMlx')}</button>}<span className={'size-1.5 rounded-full '+(runtime.status==='ready'?'bg-emerald-500':runtime.status==='loading'?'bg-amber-500':'bg-slate-300')}/>{runtime.status==='ready'?t('models.status.ready'):runtime.status==='loading'?t('models.status.loading'):runtime.status==='installing'?t('models.status.installing'):t('models.status.idle')}<button type="button" aria-label={t('models.refresh')} onClick={()=>void refresh()} className="ml-2 text-[#9C9388] hover:text-[#4A443C]"><RefreshCw size={14}/></button></div>
  </div>
  <div className="flex flex-wrap items-center gap-3 border-b border-[#EFE9DF] bg-[#FAF7F1] px-4 py-2 text-[11px] text-[#7C7469]">
   <strong className="text-[10px] font-bold uppercase tracking-widest text-[#9C9388]">{t('models.runtimes')}</strong>
   <span className="inline-flex items-center gap-1.5"><i className={'size-1.5 rounded-full '+(runtime.capabilities?.gguf.available?'bg-emerald-500':'bg-slate-300')}/>llama.cpp · {runtime.capabilities?.gguf.available?t('models.runtimeReady'):t('models.runtimeMissing')}{runtime.capabilities?.gguf.path&&<code className="hidden max-w-[220px] truncate text-[10px] text-[#9C9388] md:inline">{runtime.capabilities.gguf.path}</code>}</span>
   <span className="inline-flex items-center gap-1.5"><i className={'size-1.5 rounded-full '+(runtime.capabilities?.mlx.available?'bg-emerald-500':'bg-slate-300')}/>MLX · {runtime.capabilities?.mlx.available?t('models.runtimeReady'):t('models.runtimeMissing')}{runtime.capabilities?.mlx.path&&<code className="hidden max-w-[260px] truncate text-[10px] text-[#9C9388] md:inline">{runtime.capabilities.mlx.path}</code>}</span>
   {runtime.capabilities?.gguf.installable&&<button type="button" className={btn} disabled={Boolean(working)||installing} onClick={()=>void safe('install',()=>post('/api/model-library/runtime/install',{}))}>{t('models.installRunner')}</button>}
   {runtime.capabilities?.mlx.installable&&<button type="button" className={btn} disabled={Boolean(working)||installing} onClick={()=>void safe('install-mlx',()=>post('/api/model-library/runtime/install',{engine:'mlx'}))}>{t('models.installMlx')}</button>}
  </div>
  {error&&<div role="alert" className="mx-4 mt-3 flex items-start gap-2 rounded-lg border border-rose-200 bg-rose-50 px-3 py-2 text-[12px] text-rose-700"><CircleAlert size={15}/><span className="flex-1">{error}</span><button aria-label={t('models.ui.close')} onClick={()=>setError('')}><X size={14}/></button></div>}
  {runtime.status==='ready'&&<div className="flex flex-wrap items-center justify-between gap-3 border-b border-emerald-100 bg-[#f2fbf7] px-4 py-3">
   <span className="flex min-w-0 items-center gap-2 text-[12px]"><Check size={16} className="text-emerald-600"/><strong className="truncate">{runtime.name||runtime.repo}</strong><span className="text-emerald-700">{t('models.ui.running')}</span></span>
   <div className="flex gap-2"><button type="button" className={btn} disabled={!runtime.model} onClick={()=>{if(runtime.model)window.location.hash='#/chat?'+new URLSearchParams({provider:'dominion-native',model:runtime.model})}}><ArrowRight size={14}/> {t('models.chat')}</button><button type="button" className={btn} onClick={()=>void unload()} disabled={Boolean(working)}><Square size={13}/> {t('models.stop')}</button></div>
  </div>}
  {view==='installed'&&<div className="p-4">
   <div className="mb-3 flex flex-wrap items-center justify-between gap-2"><h3 className="text-[13px] font-bold">{t('models.installedTitle')}</h3><button type="button" className={btn} onClick={()=>setView('import')}><FolderOpen size={14}/> {t('models.importFile')}</button></div>
   {installed.length?<div className="divide-y divide-[#EFE9DF] rounded-lg border border-[#EFE9DF]">{installed.map(m=><div key={m.id} className="flex flex-wrap items-center gap-3 p-3">
    <div className="grid size-9 shrink-0 place-items-center rounded-lg bg-[#F3EEE5] text-[#A94E30]"><Cpu size={17}/></div>
    <div className="min-w-0 flex-1"><div className="break-all text-[12px] font-semibold text-[#24211D]">{m.repo}</div><div className="mt-1 text-[11px] text-[#8A8176]">{m.format.toUpperCase()} · {fmt(m.bytes)} · {m.source==='external'?t('models.source.external'):m.source==='lmstudio'?t('models.source.lmstudio'):t('models.source.downloaded')}{m.externalPath?' · '+shorten(m.externalPath,65):''}</div></div>
    <div className="flex items-center gap-1.5">{runtime.status==='ready'&&runtime.id===m.id?<span className="mr-1 rounded bg-emerald-50 px-2 py-1 text-[10px] font-semibold text-emerald-700">{t('models.inUse')}</span>:<button type="button" className={btn+' !border-[#C96442] !bg-[#C96442] !text-white hover:!bg-[#A94E30]'} disabled={Boolean(working)||runtime.status==='loading'} onClick={()=>void load(m.id)}><Play size={13}/>{working==='load'+m.id?t('models.loadingModel'):t('models.run')}</button>}
     <button type="button" className="rounded-md p-2 text-[#A79C8E] hover:bg-rose-50 hover:text-rose-600" aria-label={t('models.delete')+' '+m.repo} disabled={Boolean(working)||m.source==='lmstudio'||(runtime.id===m.id&&runtime.status==='ready')} title={m.source==='lmstudio'?unsupported:undefined} onClick={()=>void remove(m)}><Trash2 size={14}/></button>
    </div>
   </div>)}</div>:<div className="flex min-h-48 flex-col items-center justify-center gap-3 rounded-lg border border-dashed border-[#E7E0D5] bg-[#FAF7F1] px-4 py-7 text-center"><HardDrive size={25} className="text-[#B8AFA2]"/><strong className="text-[13px]">{t('models.emptyTitle')}</strong><p className="text-[11px] text-[#8A8176]">{t('models.emptyDesc')}</p><div className="flex flex-wrap gap-2"><button type="button" className={btn} onClick={()=>setView('import')}>{t('models.connectExisting')}</button><button type="button" className={btn} onClick={()=>setView('search')}>{t('models.downloadModels')}</button>{runtime.capabilities?.gguf.installable&&<button type="button" className={btn} disabled={Boolean(working)||installing} onClick={()=>void safe('install',()=>post('/api/model-library/runtime/install',{}))}>{working==='install'||installing?<LoaderCircle size={13} className="animate-spin"/>:<ArrowDownToLine size={13}/>}{t('models.installRunner')}</button>}{runtime.capabilities?.mlx.installable&&<button type="button" className={btn} disabled={Boolean(working)||installing} onClick={()=>void safe('install-mlx',()=>post('/api/model-library/runtime/install',{engine:'mlx'}))}>{working==='install-mlx'||installing?<LoaderCircle size={13} className="animate-spin"/>:<ArrowDownToLine size={13}/>}{t('models.installMlx')}</button>}</div></div>}
  </div>}
  {view==='import'&&<div className="max-w-3xl p-4">
   <h3 className="text-[13px] font-bold">{t('models.importTitle')}</h3>
   <p className="mt-1 text-[11px] text-[#7C7469]">{t('models.importDesc')}</p>
   <label className="mt-4 block text-[11px] font-medium text-[#5C544A]">{t('models.path')}</label>
   <div className="mt-1.5 flex flex-col gap-2 sm:flex-row"><TextInput className="!flex-1" value={localPath} onChange={e=>setLocalPath(e.target.value)} placeholder="/models/model.gguf"/><button type="button" className={btn} disabled={!localPicker||picking} onClick={()=>void chooseLocal()}><FolderOpen size={14}/> {t('models.pickFile')}</button></div>
   <p className="mt-2 text-[11px] text-[#8A8176]">{localPicker?t('models.pickHintLocal'):t('models.pickHintRemote')}</p>
   <button type="button" disabled={!localPath.trim()||Boolean(working)} onClick={()=>void importLocal()} className="mt-5 flex h-9 items-center gap-2 rounded-lg bg-[#C96442] px-4 text-[12px] font-semibold text-white hover:bg-[#A94E30] disabled:opacity-40">{working==='import'?<LoaderCircle className="animate-spin" size={15}/>:<Check size={15}/>}{t('models.file.register')}</button>
  </div>}
  {view==='search'&&<div className="grid min-h-[560px] grid-cols-1 lg:grid-cols-[400px_minmax(0,1fr)]">
   <div className="min-w-0 border-b border-[#E7E0D5] lg:border-b-0 lg:border-r">
    <div className="border-b border-[#EFE9DF] p-4">
     <div className="flex h-11 items-center gap-2 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] px-3 focus-within:border-[#C96442]"><Search size={17} className="text-[#A79C8E]"/><input value={q} onChange={e=>setQ(e.target.value)} placeholder={t("models.searchPlaceholder")} className="h-full w-full min-w-0 bg-transparent text-[14px] outline-none placeholder:text-[#A79C8E]"/>{q&&<button type="button" aria-label={t('models.ui.clearSearch')} onClick={()=>setQ('')} className="rounded p-1 text-[#A79C8E] hover:bg-[#F3EEE5]"><X size={14}/></button>}</div>
     <div className="mt-3 flex flex-wrap items-center gap-1.5">{[{id:'all',label:t('models.all')},{id:'gguf',label:'GGUF'},{id:'mlx',label:'MLX'}].map(item=><Button key={item.id} variant={format===item.id?'primary':'ghost'} onClick={()=>setFormat(item.id)} className="!px-3 !py-1.5 !text-[12px]">{item.label}</Button>)}{format==='mlx'&&<span className="ml-1 rounded bg-[#F7E8E1] px-2 py-0.5 text-[10px] font-semibold text-[#A94E30]">{t('models.recommended')}</span>}<Button variant="primary" disabled title={unsupported} className="!px-3 !py-1.5 !text-[12px]">{t('models.chatOnly')}</Button><SelectInput aria-label={t('models.size')} value="" disabled title={unsupported} className="!w-auto !py-1.5 !text-[12px]"><option value="">{t('models.sizeAll')}</option><option value="1">{t('models.ui.sizeMax',{n:1})}</option><option value="2">{t('models.ui.sizeMax',{n:2})}</option><option value="3">{t('models.ui.sizeMax',{n:3})}</option><option value="4">{t('models.ui.sizeMax',{n:4})}</option><option value="8">{t('models.ui.sizeMax',{n:8})}</option></SelectInput><SelectInput aria-label={t('models.sort')} value="downloads" disabled title={unsupported} className="!ml-auto !w-auto !py-1.5 !text-[12px]"><option value="downloads">{t('models.sort.downloads')}</option><option value="likes">{t('models.sort.likes')}</option><option value="lastModified">{t('models.sort.updated')}</option><option value="params">{t('models.sort.params')}</option></SelectInput></div>
     <div className="mt-2 flex items-center justify-between text-[11px] text-[#8A8176]"><span>{loading?t('models.searching'):t('models.resultCount',{count:catalog.length})}</span><span>{t('models.realtime')}</span></div>
    </div>
    <div ref={catalogueScroll} className="max-h-[610px] overflow-y-auto overscroll-contain">
     {loading&&<div className="space-y-2 p-3">{[0,1,2,3].map(i=><div key={i} className="h-20 animate-pulse rounded-xl bg-[#FAF7F1]"/>)}</div>}
     {!loading&&catalog.map(m=><button type="button" key={m.id} onClick={()=>setSelected(m.id)} className={'w-full border-b border-[#EFE9DF] px-4 py-3.5 text-left transition-colors hover:bg-[#FAF7F1] '+(selected===m.id?'bg-[#F7E8E1]':'')}><div className="flex items-start gap-3"><span className="grid size-9 shrink-0 place-items-center rounded-xl bg-[#F3EEE5] text-[13px] font-bold text-[#C96442]">{m.name.slice(0,1).toUpperCase()}</span><div className="min-w-0 flex-1"><div className="flex items-center gap-2"><strong className="truncate text-[13px] text-[#24211D]">{m.name}</strong>{m.gated&&<span className="rounded bg-amber-50 px-1.5 py-0.5 text-[9px] font-semibold text-amber-700">{t('models.ui.gated')}</span>}</div><div className="mt-0.5 truncate text-[11px] text-[#8A8176]">{m.author} · {m.task||m.library||'model'}</div><p className="mt-1 line-clamp-2 text-[11px] leading-5 text-[#5C544A]">{m.description||t('models.ui.loadingDesc')}</p><div className="mt-1.5 flex flex-wrap items-center gap-2 text-[10px] text-[#8A8176]"><span>↓ {compactNumber(m.downloads)}</span><span>♡ {compactNumber(m.likes)}</span><span>{updatedLabel(m.updatedAt)}</span><span className="rounded bg-[#F3EEE5] px-1.5 py-0.5 font-semibold">{/gguf/i.test(m.tags.join(' '))?'GGUF':'MLX'}</span></div></div></div></button>)}
     {!loading&&!catalog.length&&<div className="p-8 text-center text-[12px] leading-6 text-[#7C7469]">{t('models.noResults')}</div>}
     {!loading&&catalog.length>0&&catalog.length>=limit&&limit<100&&<div className="p-3"><Button variant="secondary" className="w-full" onClick={()=>setLimit(100)}>{t('models.more',{count:catalog.length})}</Button></div>}
    </div>
   </div>
   <div ref={detailScroll} className="min-w-0 max-h-[610px] overflow-y-auto overscroll-contain p-4 pr-2">
    {!selected?<div className="flex min-h-52 flex-col items-center justify-center gap-3 text-[12px] text-[#9C9388]"><CloudDownload size={23}/>{t('models.selectModel')}</div>:!detail?<div className="flex h-44 items-center justify-center text-[12px] text-[#8A8176]"><LoaderCircle size={17} className="mr-2 animate-spin"/>{t('models.loadingDetail')}</div>:<>
     <div className="flex items-start justify-between gap-3"><div className="min-w-0"><h3 className="break-all text-[20px] font-bold tracking-[-.02em] text-[#24211D]">{detail.name||detail.id}</h3><div className="mt-1 break-all text-[11px] text-[#8A8176]">{detail.id}</div><div className="mt-2 flex flex-wrap items-center gap-2 text-[11px] text-[#7C7469]"><span>↓ {compactNumber(detail.downloads)}</span><span>♡ {compactNumber(detail.likes)}</span><span>{detail.architecture||t('models.ui.languageModel')}</span>{detail.license&&<span>{detail.license}</span>}<span>{t('models.ui.formats',{count:detail.options.length})}</span></div></div><a href={'https://huggingface.co/'+detail.id} target="_blank" rel="noreferrer" aria-label={t('models.ui.viewHf')} className="inline-flex shrink-0 items-center gap-1 rounded-md px-2 py-1.5 text-[11px] font-semibold text-[#C96442] hover:bg-[#F7E8E1]">Hugging Face <ExternalLink size={14}/></a></div>
     <div className="mt-5 text-[13px] font-semibold text-[#3A342D]">{t('models.downloadFiles')}</div>
     <div className="mt-2 max-h-56 overflow-auto rounded-xl border border-[#E7E0D5]">{detail.options.filter(o=>format==='all'||o.format===format).map(o=><button type="button" key={o.id} onClick={()=>setVariant(o.id)} className={'flex w-full items-center gap-3 border-b border-[#EFE9DF] px-3.5 py-3.5 text-left last:border-0 '+(variant===o.id?'bg-[#F7E8E1]':'hover:bg-[#FAF7F1]')}><span className="rounded bg-[#F3EEE5] px-2 py-1 text-[10px] font-semibold text-[#7C7469]">{o.format.toUpperCase()}</span><span className="min-w-0 flex-1 text-[13px] font-semibold text-[#3A342D]">{o.label}<span className="ml-2 text-[10px] font-normal text-[#8A8176]">{o.quantization&&o.quantization!==o.label?o.quantization:''}{o.shards>1?' · '+t('models.ui.shardFiles',{count:o.shards}):''}</span></span><span className="text-[12px] text-[#5C544A]">{fmt(o.bytes)}</span>{variant===o.id&&<Check size={15} className="text-[#C96442]"/>}</button>)}</div>
     {!detail.options.length&&<p className="mt-3 text-[12px] text-amber-700">{t('models.noGguf')}</p>}
     {option&&<div className="mt-4 flex flex-wrap items-center gap-3 rounded-xl bg-[#FAF7F1] p-3">
      <span className="text-[12px] text-[#5C544A]">{t('models.sizeLabel')} <strong>{fmt(option.bytes)}</strong></span>
      {existing?<button type="button" className={btn} onClick={()=>{setView('installed')}}><Check size={14}/> {t('models.installedView')}</button>:
       <button type="button" className="flex h-8 items-center gap-2 rounded-lg bg-[#C96442] px-3 text-[11px] font-semibold text-white disabled:opacity-40" disabled={Boolean(working)||['resolving','downloading'].includes(selectedJob?.status||'')||detail.gated} onClick={()=>void safe('download',()=>post('/api/model-library/download',{repo:detail.id,optionId:option.id,format:option.format,files:option.files}))}><ArrowDownToLine size={14}/>{working==='download'?t('models.requesting'):selectedJob?.status==='downloading'?t('models.downloading'):t('models.downloadStart')}</button>}
      {detail.gated&&<span className="text-[11px] text-amber-700">{t('models.approval')}</span>}
     </div>}
     {selectedJob&&<div className="mt-3 rounded-lg border border-[#EFE9DF] p-3 text-[11px]"><div className="flex justify-between gap-3"><span>{selectedJob.status} {selectedJob.current&&'· '+shorten(selectedJob.current,30)}</span><span>{fmt(selectedJob.downloaded)} / {fmt(selectedJob.total)}</span></div><div className="mt-2 h-1.5 overflow-hidden rounded-full bg-[#F3EEE5]"><div className="h-full bg-[#C96442]" style={{width:Math.min(100,selectedJob.downloaded/Math.max(1,selectedJob.total)*100)+'%'}}/></div>{selectedJob.error&&<p className="mt-2 text-rose-600">{translateError(selectedJob.error)}</p>}<div className="mt-2 flex gap-2">{selectedJob.status==='downloading'&&<button type="button" className={btn} onClick={()=>void safe('pause',()=>post('/api/model-library/jobs/'+selectedJob.id+'/pause',{}))}><Pause size={13}/> {t('models.pause')}</button>}{['paused','failed','cancelled'].includes(selectedJob.status)&&<button type="button" className={btn} onClick={()=>void safe('resume',()=>post('/api/model-library/jobs/'+selectedJob.id+'/resume',{}))}><Play size={13}/> {t('models.resume')}</button>}{['resolving','downloading','paused'].includes(selectedJob.status)&&<button type="button" className={btn} onClick={()=>void safe('cancel',()=>post('/api/model-library/jobs/'+selectedJob.id+'/cancel',{}))}><Square size={13}/> {t('models.download.cancel')}</button>}</div></div>}
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
  {runtime.status==='error'&&runtime.error&&<div className="border-t border-rose-100 bg-rose-50 px-4 py-2 text-[11px] text-rose-700">{translateError(runtime.error)}</div>}
 </section>
}
