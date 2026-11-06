import {useCallback,useEffect,useLayoutEffect,useRef,useState} from 'react'
import {ArrowDownToLine,ArrowRight,Check,ChevronDown,CircleAlert,CloudDownload,Cpu,ExternalLink,FolderOpen,HardDrive,LoaderCircle,Pause,Play,RefreshCw,Search,Square,Trash2,X} from 'lucide-react'
import {Button,SelectInput,TextInput} from './Primitives'
import {get,post,del} from '../api'
type Catalog={id:string;author:string;name:string;library:string;createdAt?:string;downloads:number;likes:number;gated:boolean;updatedAt:string;description:string;task:string;tags:string[]}
type Option={id:string;format:'gguf'|'mlx';label:string;quantization:string;bytes:number;files:string[];shards:number}
type Detail=Catalog&{revision:string;license:string;architecture:string;options:Option[];readme:string}
type Installed={id:string;repo:string;optionId?:string;source?:string;externalPath?:string;format:'gguf'|'mlx';label:string;bytes:number;installedAt:string}
type Job={id:string;repo:string;optionId:string;status:string;downloaded:number;total:number;current:string;error:string;installedId?:string}
type Runtime={status:string;id?:string;repo?:string;engine?:string;name?:string;error?:string;setup?:{status:string;backend:string;downloaded:number;total:number;tag:string;file?:string;error:string};capabilities?:{gguf:{available:boolean;backend?:string;installable?:boolean};mlx:{available:boolean};memory:{total:number;free:number;platform?:string;arch?:string};gpu?:{kind:string;free:number}|null}}
const fmt=(v=0)=>v>=1073741824?(v/1073741824).toFixed(1)+' GB':v>=1048576?(v/1048576).toFixed(0)+' MB':v>=1024?(v/1024).toFixed(0)+' KB':String(v)+' B'
const shorten=(v:string,n=45)=>v.length>n?v.slice(0,n-3)+'…':v
const compactNumber=(v=0)=>v>=1e6?(v/1e6).toFixed(1)+'M':v>=1e3?(v/1e3).toFixed(1)+'K':String(v)
const cleanMarkdown=(value='')=>String(value).replace(/^---[\s\S]*?---/,'').replace(/```[\s\S]*?```/g,' ').replace(/\[!\[[^\]]*\]\([^)]*\)\]\([^)]*\)/g,' ').replace(/!\[[^\]]*\]\([^)]*\)/g,' ').replace(/\[([^\]]+)\]\([^)]*\)/g,'$1').replace(/^\s*\|.*$/gm,' ').replace(/^\s*[-:]{3,}\s*$/gm,' ').replace(/<[^>]+>/g,' ').replace(/^#{1,6}\s*/gm,'').replace(/[*_`>]/g,'').replace(/\n{3,}/g,'\n\n').trim()
const updatedLabel=(value:string)=>value?new Date(value).toLocaleDateString():''
const viewTabs=[{id:'installed',label:'내 컴퓨터의 모델',icon:HardDrive},{id:'search',label:'다운로드할 모델 찾기',icon:Search},{id:'import',label:'기존 GGUF 연결',icon:FolderOpen}] as const
export function ModelExplorer(){
 const [view,setView]=useState<'installed'|'search'|'import'>('installed')
 const [installed,setInstalled]=useState<Installed[]>([])
 const [jobs,setJobs]=useState<Job[]>([])
 const [runtime,setRuntime]=useState<Runtime>({status:'idle'})
 const [q,setQ]=useState('')
 const [format,setFormat]=useState('gguf')
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
 const native='__DOMINION_NATIVE__' in window
 const unsupported='현재 Rust 모델 라이브러리에서 지원하지 않습니다.'
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
    const data=await get<Catalog[]>('/api/model-library/catalog?'+new URLSearchParams({q,limit:String(limit)}),controller.signal)
    if(!controller.signal.aborted){setCatalog(data);setSelected(s=>data.some(m=>m.id===s)?s:'');setError('')}
   }catch(e){if(!controller.signal.aborted)setError((e as Error).message)}
   finally{if(!controller.signal.aborted)setLoading(false)}
  },300)
  return()=>{controller.abort();clearTimeout(timer)}
 },[view,q,limit])
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
  if(picking||!native)return;
  setPicking(true);setError('');
  try{const {path}=await post<{path?:string}>('/api/model-library/pick-file',{});if(path)setLocalPath(path)}
  catch(e){setError((e as Error).message)}finally{setPicking(false)}
 }
 const load=(id:string)=>safe('load'+id,()=>post('/api/model-library/runtime/load',{id,context}))
 const unload=()=>safe('unload',()=>post('/api/model-library/runtime/unload',{}))
 const remove=async(m:Installed)=>{
  if(!window.confirm(m.source==='external'?'등록을 해제하시겠습니까? 원본 모델 파일은 유지됩니다.':'다운로드한 모델 파일을 삭제하시겠습니까?'))return
  await safe('remove'+m.id,()=>del('/api/model-library/installed/'+encodeURIComponent(m.id)))
 }
 const btn='inline-flex h-8 items-center justify-center gap-2 rounded-lg border border-[#dfe4ed] bg-white px-3 text-[11px] font-semibold text-[#3A342D] hover:bg-[#f5f7fa] disabled:opacity-40'
 return <section className="overflow-hidden rounded-xl border border-[#dfe4eb] bg-white">
  <div className="flex flex-wrap items-center justify-between gap-3 border-b border-[#e7eaf0] px-4 py-3">
   <div className="flex flex-wrap items-center gap-1">{viewTabs.map(tab=>{const Icon=tab.icon;return <button key={tab.id} type="button" onClick={()=>{setView(tab.id);setError('')}} className={'flex h-8 items-center gap-1.5 rounded-lg px-3 text-[12px] font-semibold '+(view===tab.id?'bg-[#F7E8E1] text-[#A94E30]':'text-[#758295] hover:bg-[#f3f5f9]')}><Icon size={14}/>{tab.label}{tab.id==='installed'&&<span className="ml-1 rounded bg-white/75 px-1.5 text-[10px]">{installed.length}</span>}</button>})}</div>
   <div className="flex items-center gap-2 text-[11px] text-[#697789]"><span className={'size-1.5 rounded-full '+(runtime.status==='ready'?'bg-emerald-500':runtime.status==='loading'?'bg-amber-500':'bg-slate-300')}/>{runtime.status==='ready'?'모델 실행 중':runtime.status==='loading'?'로딩 중':runtime.status==='installing'?'실행기 설치 중':'모델 대기'}<button type="button" aria-label="새로고침" onClick={()=>void refresh()} className="ml-2 text-[#9C9388] hover:text-[#4A443C]"><RefreshCw size={14}/></button></div>
  </div>
  {error&&<div role="alert" className="mx-4 mt-3 flex items-start gap-2 rounded-lg border border-rose-200 bg-rose-50 px-3 py-2 text-[12px] text-rose-700"><CircleAlert size={15}/><span className="flex-1">{error}</span><button aria-label="닫기" onClick={()=>setError('')}><X size={14}/></button></div>}
  {runtime.status==='ready'&&<div className="flex flex-wrap items-center justify-between gap-3 border-b border-emerald-100 bg-[#f2fbf7] px-4 py-3">
   <span className="flex min-w-0 items-center gap-2 text-[12px]"><Check size={16} className="text-emerald-600"/><strong className="truncate">{runtime.name||runtime.repo}</strong><span className="text-emerald-700">실행 중</span></span>
   <div className="flex gap-2"><button type="button" className={btn} onClick={()=>{window.location.hash='#/chat'}}><ArrowRight size={14}/> 채팅으로 이동</button><button type="button" className={btn} onClick={()=>void unload()} disabled={Boolean(working)}><Square size={13}/> 중지</button></div>
  </div>}
  {view==='installed'&&<div className="p-4">
   <div className="mb-3 flex flex-wrap items-center justify-between gap-2"><h3 className="text-[13px] font-bold">실행할 수 있는 모델</h3><button type="button" className={btn} onClick={()=>setView('import')}><FolderOpen size={14}/> 기존 파일 가져오기</button></div>
   {installed.length?<div className="divide-y divide-[#edf0f4] rounded-lg border border-[#e7eaf0]">{installed.map(m=><div key={m.id} className="flex flex-wrap items-center gap-3 p-3">
    <div className="grid size-9 shrink-0 place-items-center rounded-lg bg-[#edf1fb] text-[#5870b7]"><Cpu size={17}/></div>
    <div className="min-w-0 flex-1"><div className="break-all text-[12px] font-semibold text-[#263247]">{m.repo}</div><div className="mt-1 text-[11px] text-[#8591a0]">{m.format.toUpperCase()} · {fmt(m.bytes)} · {m.source==='external'?'로컬 파일 연결':m.source==='lmstudio'?'LM Studio':'다운로드됨'}{m.externalPath?' · '+shorten(m.externalPath,65):''}</div></div>
    <div className="flex items-center gap-1.5">{runtime.status==='ready'&&runtime.id===m.id?<span className="mr-1 rounded bg-emerald-50 px-2 py-1 text-[10px] font-semibold text-emerald-700">사용 중</span>:<button type="button" className={btn+' !border-[#C96442] !bg-[#C96442] !text-white hover:!bg-[#A94E30]'} disabled={Boolean(working)||runtime.status==='loading'} onClick={()=>void load(m.id)}><Play size={13}/>{working==='load'+m.id?'로딩 중':'실행'}</button>}
     <button type="button" className="rounded-md p-2 text-[#9ca5b3] hover:bg-rose-50 hover:text-rose-600" aria-label={'모델 삭제 '+m.repo} disabled={Boolean(working)||m.source==='lmstudio'||(runtime.id===m.id&&runtime.status==='ready')} title={m.source==='lmstudio'?unsupported:undefined} onClick={()=>void remove(m)}><Trash2 size={14}/></button>
    </div>
   </div>)}</div>:<div className="flex min-h-48 flex-col items-center justify-center gap-3 rounded-lg border border-dashed border-[#dfe4ed] bg-[#fafbfc] px-4 py-7 text-center"><HardDrive size={25} className="text-[#a1adbc]"/><strong className="text-[13px]">등록된 모델이 없습니다</strong><p className="text-[11px] text-[#8290a1]">이미 가지고 있는 GGUF·MLX 파일을 불러오거나 새로 다운로드하세요.</p><div className="flex gap-2"><button type="button" className={btn} onClick={()=>setView('import')}>기존 GGUF 연결</button><button type="button" className={btn} onClick={()=>setView('search')}>모델 다운로드</button></div></div>}
  </div>}
  {view==='import'&&<div className="max-w-3xl p-4">
   <h3 className="text-[13px] font-bold">이미 있는 GGUF 파일 연결</h3>
   <p className="mt-1 text-[11px] text-[#7b8898]">GGUF 파일을 선택하면 원본 위치에서 실행합니다.</p>
   <div className="mt-4 flex flex-wrap items-center gap-2"><button type="button" className={btn+' !border-[#8da2e7] !bg-[#edf1ff] !text-[#3857bb]'}>GGUF</button><button type="button" disabled title={unsupported} className={btn}>MLX</button></div>
   <label className="mt-4 block text-[11px] font-medium text-[#526074]">GGUF 파일 경로</label>
   <div className="mt-1.5 flex flex-col gap-2 sm:flex-row"><TextInput className="!flex-1" value={localPath} onChange={e=>setLocalPath(e.target.value)} placeholder="/models/model.gguf"/><button type="button" className={btn} disabled={!native||picking} onClick={()=>void chooseLocal()}><FolderOpen size={14}/> 파일 선택</button></div>
   <p className="mt-2 text-[11px] text-[#8b97a7]">브라우저에서 접속했다면 DOMINION 서버가 실행 중인 컴퓨터의 파일 경로를 입력하세요.</p>
   <button type="button" disabled={!localPath.trim()||Boolean(working)} onClick={()=>void importLocal()} className="mt-5 flex h-9 items-center gap-2 rounded-lg bg-[#C96442] px-4 text-[12px] font-semibold text-white hover:bg-[#A94E30] disabled:opacity-40">{working==='import'?<LoaderCircle className="animate-spin" size={15}/>:<Check size={15}/>}모델 등록</button>
  </div>}
  {view==='search'&&<div className="grid min-h-[560px] grid-cols-1 lg:grid-cols-[400px_minmax(0,1fr)]">
   <div className="min-w-0 border-b border-[#E7E0D5] lg:border-b-0 lg:border-r">
    <div className="border-b border-[#EFE9DF] p-4">
     <div className="flex h-11 items-center gap-2 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] px-3 focus-within:border-[#C96442]"><Search size={17} className="text-[#A79C8E]"/><input value={q} onChange={e=>setQ(e.target.value)} placeholder="Hugging Face 모델 검색 (gemma, qwen, uncensored…)" className="h-full w-full min-w-0 bg-transparent text-[14px] outline-none placeholder:text-[#A79C8E]"/>{q&&<button type="button" aria-label="검색어 지우기" onClick={()=>setQ('')} className="rounded p-1 text-[#A79C8E] hover:bg-[#F3EEE5]"><X size={14}/></button>}</div>
     <div className="mt-3 flex flex-wrap items-center gap-1.5">{[{id:'all',label:'전체'},{id:'gguf',label:'GGUF'},{id:'mlx',label:'MLX'}].map(item=><Button key={item.id} variant={format===item.id?'primary':'ghost'} disabled={item.id==='mlx'} onClick={()=>setFormat(item.id)} className="!px-3 !py-1.5 !text-[12px]">{item.label}</Button>)}<Button variant="primary" disabled title={unsupported} className="!px-3 !py-1.5 !text-[12px]">대화 모델만</Button><SelectInput aria-label="모델 크기" value="" disabled title={unsupported} className="!w-auto !py-1.5 !text-[12px]"><option value="">크기 전체</option><option value="1">1B 이하</option><option value="2">2B 이하</option><option value="3">3B 이하</option><option value="4">4B 이하</option><option value="8">8B 이하</option></SelectInput><SelectInput aria-label="정렬" value="downloads" disabled title={unsupported} className="!ml-auto !w-auto !py-1.5 !text-[12px]"><option value="downloads">다운로드순</option><option value="likes">인기순</option><option value="lastModified">최신순</option><option value="params">작은 모델순</option></SelectInput></div>
     <div className="mt-2 flex items-center justify-between text-[11px] text-[#8A8176]"><span>{loading?'검색 중…':catalog.length+'개 모델'}</span><span>Hugging Face 실시간</span></div>
    </div>
    <div ref={catalogueScroll} className="max-h-[610px] overflow-y-auto overscroll-contain">
     {loading&&<div className="space-y-2 p-3">{[0,1,2,3].map(i=><div key={i} className="h-20 animate-pulse rounded-xl bg-[#FAF7F1]"/>)}</div>}
     {!loading&&catalog.map(m=><button type="button" key={m.id} onClick={()=>setSelected(m.id)} className={'w-full border-b border-[#EFE9DF] px-4 py-3.5 text-left transition-colors hover:bg-[#FAF7F1] '+(selected===m.id?'bg-[#F7E8E1]':'')}><div className="flex items-start gap-3"><span className="grid size-9 shrink-0 place-items-center rounded-xl bg-[#F3EEE5] text-[13px] font-bold text-[#C96442]">{m.name.slice(0,1).toUpperCase()}</span><div className="min-w-0 flex-1"><div className="flex items-center gap-2"><strong className="truncate text-[13px] text-[#24211D]">{m.name}</strong>{m.gated&&<span className="rounded bg-amber-50 px-1.5 py-0.5 text-[9px] font-semibold text-amber-700">gated</span>}</div><div className="mt-0.5 truncate text-[11px] text-[#8A8176]">{m.author} · {m.task||m.library||'model'}</div><p className="mt-1 line-clamp-2 text-[11px] leading-5 text-[#5C544A]">{m.description||'설명을 불러오는 중입니다.'}</p><div className="mt-1.5 flex flex-wrap items-center gap-2 text-[10px] text-[#8A8176]"><span>↓ {compactNumber(m.downloads)}</span><span>♡ {compactNumber(m.likes)}</span><span>{updatedLabel(m.updatedAt)}</span><span className="rounded bg-[#F3EEE5] px-1.5 py-0.5 font-semibold">{/gguf/i.test(m.tags.join(' '))?'GGUF':'MLX'}</span></div></div></div></button>)}
     {!loading&&!catalog.length&&<div className="p-8 text-center text-[12px] leading-6 text-[#7C7469]">검색 결과가 없습니다.</div>}
     {!loading&&catalog.length>0&&catalog.length>=limit&&limit<100&&<div className="p-3"><Button variant="secondary" className="w-full" onClick={()=>setLimit(100)}>더 보기 · {catalog.length}개 표시 중</Button></div>}
    </div>
   </div>
   <div ref={detailScroll} className="min-w-0 max-h-[610px] overflow-y-auto overscroll-contain p-4 pr-2">
    {!selected?<div className="flex min-h-52 flex-col items-center justify-center gap-3 text-[12px] text-[#8d99a8]"><CloudDownload size={23}/>왼쪽에서 모델을 선택하세요.</div>:!detail?<div className="flex h-44 items-center justify-center text-[12px] text-[#8290a1]"><LoaderCircle size={17} className="mr-2 animate-spin"/>모델 상세 정보 불러오는 중</div>:<>
     <div className="flex items-start justify-between gap-3"><div className="min-w-0"><h3 className="break-all text-[20px] font-bold tracking-[-.02em] text-[#24211D]">{detail.name||detail.id}</h3><div className="mt-1 break-all text-[11px] text-[#8A8176]">{detail.id}</div><div className="mt-2 flex flex-wrap items-center gap-2 text-[11px] text-[#7C7469]"><span>↓ {compactNumber(detail.downloads)}</span><span>♡ {compactNumber(detail.likes)}</span><span>{detail.architecture||'언어 모델'}</span>{detail.license&&<span>{detail.license}</span>}<span>{detail.options.length}개 배포 형식</span></div></div><a href={'https://huggingface.co/'+detail.id} target="_blank" rel="noreferrer" aria-label="Hugging Face에서 보기" className="inline-flex shrink-0 items-center gap-1 rounded-md px-2 py-1.5 text-[11px] font-semibold text-[#C96442] hover:bg-[#F7E8E1]">Hugging Face <ExternalLink size={14}/></a></div>
     <div className="mt-5 text-[13px] font-semibold text-[#3A342D]">다운로드할 파일 선택</div>
     <div className="mt-2 max-h-56 overflow-auto rounded-xl border border-[#E7E0D5]">{detail.options.filter(o=>format==='all'||o.format===format).map(o=><button type="button" key={o.id} onClick={()=>setVariant(o.id)} className={'flex w-full items-center gap-3 border-b border-[#EFE9DF] px-3.5 py-3.5 text-left last:border-0 '+(variant===o.id?'bg-[#F7E8E1]':'hover:bg-[#FAF7F1]')}><span className="rounded bg-[#F3EEE5] px-2 py-1 text-[10px] font-semibold text-[#7C7469]">{o.format.toUpperCase()}</span><span className="min-w-0 flex-1 text-[13px] font-semibold text-[#3A342D]">{o.label}<span className="ml-2 text-[10px] font-normal text-[#8A8176]">{o.quantization&&o.quantization!==o.label?o.quantization:''}{o.shards>1?' · '+o.shards+'개 파일':''}</span></span><span className="text-[12px] text-[#5C544A]">{fmt(o.bytes)}</span>{variant===o.id&&<Check size={15} className="text-[#C96442]"/>}</button>)}</div>
     {!detail.options.length&&<p className="mt-3 text-[12px] text-amber-700">이 저장소에는 지원되는 GGUF·MLX 모델 파일이 없습니다.</p>}
     {option&&<div className="mt-4 flex flex-wrap items-center gap-3 rounded-xl bg-[#FAF7F1] p-3">
      <span className="text-[12px] text-[#5C544A]">다운로드 용량 <strong>{fmt(option.bytes)}</strong></span>
      {existing?<button type="button" className={btn} onClick={()=>{setView('installed')}}><Check size={14}/> 설치됨 · 내 모델 보기</button>:
       <button type="button" className="flex h-8 items-center gap-2 rounded-lg bg-[#C96442] px-3 text-[11px] font-semibold text-white disabled:opacity-40" disabled={Boolean(working)||['resolving','downloading'].includes(selectedJob?.status||'')||detail.gated} onClick={()=>void safe('download',()=>post('/api/model-library/download',{repo:detail.id,optionId:option.id}))}><ArrowDownToLine size={14}/>{working==='download'?'요청 중':selectedJob?.status==='downloading'?'다운로드 중':'다운로드 시작'}</button>}
      {detail.gated&&<span className="text-[11px] text-amber-700">접근 승인 필요</span>}
     </div>}
     {selectedJob&&<div className="mt-3 rounded-lg border border-[#e8ebf1] p-3 text-[11px]"><div className="flex justify-between gap-3"><span>{selectedJob.status} {selectedJob.current&&'· '+shorten(selectedJob.current,30)}</span><span>{fmt(selectedJob.downloaded)} / {fmt(selectedJob.total)}</span></div><div className="mt-2 h-1.5 overflow-hidden rounded-full bg-[#F3EEE5]"><div className="h-full bg-[#C96442]" style={{width:Math.min(100,selectedJob.downloaded/Math.max(1,selectedJob.total)*100)+'%'}}/></div>{selectedJob.error&&<p className="mt-2 text-rose-600">{selectedJob.error}</p>}<div className="mt-2 flex gap-2">{selectedJob.status==='downloading'&&<button type="button" className={btn} disabled title={unsupported}><Pause size={13}/> 일시정지</button>}{['paused','failed','cancelled'].includes(selectedJob.status)&&<button type="button" className={btn} disabled title={unsupported}><Play size={13}/> 재개</button>}{['resolving','downloading','paused'].includes(selectedJob.status)&&<button type="button" className={btn} onClick={()=>void safe('cancel',()=>post('/api/model-library/jobs/'+selectedJob.id+'/cancel',{}))}><Square size={13}/> 취소</button>}</div></div>}
         <div className="mt-5 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-4"><div className="flex items-center justify-between gap-2"><strong className="text-[13px] text-[#3A342D]">모델 설명 · README</strong><span className="text-[10px] font-semibold text-[#A79C8E]">HUGGING FACE</span></div><p className="mt-2 whitespace-pre-wrap text-[12px] leading-6 text-[#5C544A]">{readmePlain.slice(0,1200)||readmeSummary||'README가 비어 있습니다.'}</p>{readmePlain.length>1200&&<details className="mt-2"><summary className="cursor-pointer text-[11px] font-semibold text-[#C96442]">README 전체 보기</summary><pre className="mt-2 max-h-72 overflow-auto whitespace-pre-wrap text-[11px] leading-5 text-[#5C544A]">{readmePlain.slice(0,14000)}</pre></details>}</div>
     </>}
   </div>
  </div>}
  {(installing||runtime.setup?.status==='failed'||(runtime.status==='installing'))&&<div role="status" className="border-t border-[#e4e9f1] bg-[#f3f6fc] px-4 py-3 text-[11px] text-[#53637d]">
    <div className="flex flex-wrap items-center justify-between gap-2"><strong>{runtime.setup?.status==='failed'?'실행기 설치 실패':runtime.setup?.status==='verifying'?'실행기 검증 중':runtime.setup?.status==='extracting'?'압축 해제 중':'공식 llama.cpp 실행기 설치 중'}</strong><span>{runtime.setup?.tag||''} {runtime.setup?.total?installerPercent+'%':''}</span></div>
    {installing&&<div className="mt-2 h-1.5 overflow-hidden rounded-full bg-[#dce4f2]"><div className="h-full bg-[#C96442]" style={{width:installerPercent+'%'}}/></div>}
    {runtime.setup?.error&&<p className="mt-2 text-rose-700">{runtime.setup.error}</p>}
    {runtime.setup?.file&&<p className="mt-1 truncate text-[#8d9bad]">{runtime.setup.file}</p>}
   </div>}
  {runtime.status==='error'&&runtime.error&&<div className="border-t border-rose-100 bg-rose-50 px-4 py-2 text-[11px] text-rose-700">{runtime.error}</div>}
 </section>
}
