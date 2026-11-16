import {useEffect,useMemo,useState} from 'react'
import {
 ArrowRight,Check,CheckCircle2,ChevronRight,Clapperboard,Clock3,CloudUpload,
 Cpu,Download,ExternalLink,FileJson2,Film,FolderOpen,HardDrive,Info,
 Layers3,LoaderCircle,LockKeyhole,MonitorPlay,Play,RefreshCw,Search,
 Settings2,ShieldCheck,SlidersHorizontal,Sparkles,Upload,Video,Youtube
} from 'lucide-react'
import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {FormField,Notice,Panel,StateTag,useRemote} from '../components/UI'
import {downloadApiVideo,getApiVideoPreview,openOAuth,post} from '../api'
import {jobFinished,useJob,type JobState} from '../hooks/useJob'
import {t,useLocale} from '../i18n'

type VideoEngine='native'|'comfyui'
type StudioTab='create'|'library'|'publish'|'settings'
type Ratio='9:16'|'16:9'|'1:1'
type Workflow={id:string;name:string;bytes?:number}
type VideoJob=JobState&{
 progress?:number;kind:'video';engine?:VideoEngine;title?:string;brief:string;
 outputUrl?:string|null;result?:{bytes?:number};duration?:number;aspectRatio?:Ratio;
 createdAt?:string;updatedAt?:string;costKrw?:number
}
type VideoStatus={
 config:{engine:VideoEngine;comfyUrl:string;voice?:string;narration?:boolean;videoModel?:string;resolution?:string;steps?:number};
 native?:{available:boolean};comfy:{reachable:boolean;available?:boolean};
 workflows:Workflow[];jobs:VideoJob[]
}
type VideoModel={id:string;name:string;kind:string;available:boolean;hint:string}
type UploadJob=JobState&{videoId?:string;title?:string;progress?:number;result?:{url:string};createdAt?:string}
type YoutubeStatus={connected:boolean;uploadAllowed:boolean;account?:{displayName?:string;scopes?:string}|null;uploads:UploadJob[]}
const RATIOS:{
 [key in Ratio]:{width:number;height:number;label:string;subtitle:string}
}={
 '9:16':{width:360,height:640,label:'세로 영상',subtitle:'쇼츠 · 릴스'},
 '16:9':{width:640,height:360,label:'가로 영상',subtitle:'유튜브 · 와이드'},
 '1:1':{width:480,height:480,label:'정사각형',subtitle:'피드 · 카드'}
}
const NAV=[
 {id:'create' as const,label:'제작 스튜디오',sub:'영상 만들기',icon:Sparkles},
 {id:'library' as const,label:'작업 보관함',sub:'이력과 결과',icon:Layers3},
 {id:'publish' as const,label:'YouTube 게시',sub:'검토 후 업로드',icon:Youtube},
 {id:'settings' as const,label:'엔진·연결',sub:'모델과 계정',icon:Settings2},
]
const activeStatus=(status?:string)=>status==='running'||status==='queued'||status==='downloading'
const statusLabel=(status?:string)=>({
 completed:'완료',running:'생성 중',queued:'대기 중',downloading:'저장 중',
 failed:'실패',cancelled:'취소',needs_review:'확인 필요',interrupted:'중단'
}[status||'']||status||'대기')
const statusClasses=(status?:string)=>{
 if(status==='completed')return 'border-emerald-200 bg-emerald-50 text-emerald-800'
 if(activeStatus(status))return 'border-sky-200 bg-sky-50 text-sky-800'
 if(status==='failed'||status==='interrupted')return 'border-rose-200 bg-rose-50 text-rose-800'
 return 'border-amber-200 bg-amber-50 text-amber-800'
}
const bytesLabel=(bytes?:number)=>{
 const n=Number(bytes||0)
 if(!Number.isFinite(n)||n<=0)return '크기 확인 전'
 if(n>=1024**3)return (n/1024**3).toFixed(1)+' GB'
 if(n>=1024**2)return (n/1024**2).toFixed(1)+' MB'
 return Math.max(1,Math.round(n/1024))+' KB'
}
const dateLabel=(input?:string)=>{
 if(!input)return '시간 정보 없음'
 const d=new Date(input)
 return Number.isNaN(d.getTime())?'시간 정보 없음':new Intl.DateTimeFormat('ko-KR',{
  month:'short',day:'numeric',hour:'2-digit',minute:'2-digit'
 }).format(d)
}
const safeSession=(key:string)=>{
 try{return window.sessionStorage.getItem(key)||''}catch{return ''}
}
function StatusPill({status}:{status?:string}){
 return <span className={'inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] font-semibold '+statusClasses(status)}>
  <span className={'size-1.5 shrink-0 rounded-full '+(activeStatus(status)?'bg-sky-500 motion-safe:animate-pulse':status==='completed'?'bg-emerald-500':status==='failed'?'bg-rose-500':'bg-amber-500')}/>
  {statusLabel(status)}
 </span>
}
function SectionTitle({eyebrow,title,detail}:{eyebrow:string;title:string;detail?:string}){
 return <div className="min-w-0">
  <p className="text-[10px] font-bold uppercase tracking-[.16em] text-[#A78371]">{eyebrow}</p>
  <h2 className="mt-1 text-[19px] font-bold tracking-[-.035em] text-[#282620]">{title}</h2>
  {detail&&<p className="mt-1 text-[12px] leading-5 text-[#847A70]">{detail}</p>}
 </div>
}
function EmptyStage({title,detail}:{title:string;detail:string}){
 return <div className="flex min-h-[240px] flex-col items-center justify-center rounded-2xl border border-dashed border-[#DBD6CF] bg-[#F9F7F3] px-5 text-center">
  <div className="mb-3 flex size-12 items-center justify-center rounded-2xl bg-white text-[#B56A4F] shadow-sm"><Clapperboard size={23}/></div>
  <strong className="text-[14px] text-[#342E29]">{title}</strong>
  <p className="mt-1 max-w-xs text-[12px] leading-5 text-[#8D8176]">{detail}</p>
 </div>
}

export function VideoPage({navigate:_navigate}:{navigate:(page:any)=>void}){
 useLocale()
 const video=useRemote<VideoStatus>('/api/video/status',{
  config:{engine:'native',comfyUrl:''},native:{available:false},
  comfy:{reachable:false},workflows:[],jobs:[]
 },false)
 const youtube=useRemote<YoutubeStatus>('/api/youtube/status',{connected:false,uploadAllowed:false,account:null,uploads:[]})
 const oauth=useRemote<{id:string;configured:boolean;callback?:string}[]>('/api/oauth/providers',[])
 const outputs=useRemote<VideoJob[]>('/api/video/outputs',[])
 const catalog=useRemote<{videoModels:VideoModel[]}>('/api/video/catalog',{videoModels:[]})
 const [tab,setTab]=useState<StudioTab>('create')
 const [selectedId,setSelectedId]=useState(()=>safeSession('dominion.video.selected'))
 const [libraryFilter,setLibraryFilter]=useState<'all'|'completed'|'active'|'issues'>('all')
 const [librarySearch,setLibrarySearch]=useState('')
 const [engineChoice,setEngineChoice]=useState<VideoEngine|null>(null)
 const [title,setTitle]=useState('')
 const [prompt,setPrompt]=useState('')
 const [negative,setNegative]=useState('')
 const [ratio,setRatio]=useState<Ratio>('9:16')
 const [seconds,setSeconds]=useState(6)
 const [fps,setFps]=useState(16)
 const [seed,setSeed]=useState('-1')
 const [comfyUrl,setComfyUrl]=useState('')
 const [narration,setNarration]=useState(true)
 const [savingConfig,setSavingConfig]=useState(false)
 const [dragging,setDragging]=useState(false)
 const [refreshing,setRefreshing]=useState(false)
 const [gid,setGid]=useState('')
 const [gsecret,setGsecret]=useState('')
 const [gBusy,setGBusy]=useState(false)
 const [generating,setGenerating]=useState(false)
 const [error,setError]=useState('')
 const [success,setSuccess]=useState('')
 const [uploadFile,setUploadFile]=useState('')
 const [uploadTitle,setUploadTitle]=useState('')
 const [uploadDescription,setUploadDescription]=useState('')
 const [uploadPrivacy,setUploadPrivacy]=useState('private')
 const [uploadTags,setUploadTags]=useState('')
 const [uploading,setUploading]=useState(false)
 const [uploadId,setUploadId]=useState('')
 const [previewUrl,setPreviewUrl]=useState('')
 const [previewError,setPreviewError]=useState('')
 const [previewLoading,setPreviewLoading]=useState(false)
 const [videoSubmission,setVideoSubmission]=useState<VideoJob|null>(null)
 const [uploadSubmission,setUploadSubmission]=useState<UploadJob|null>(null)

 const selectedFromList=video.value.jobs.find(item=>item.id===selectedId)
 const activeVideo=useJob<VideoJob>(selectedId?'/api/video/jobs/'+selectedId:null,
  videoSubmission?.id===selectedId?videoSubmission:selectedFromList??null)
 const videoJob=activeVideo.value?.id===selectedId?activeVideo.value:selectedFromList
 const selectedOutput=uploadFile
  ?outputs.value.find(item=>item.id===uploadFile)??(videoJob?.id===uploadFile&&videoJob.status==='completed'?videoJob:undefined)
  :outputs.value[0]
 const selectedUpload=youtube.value.uploads.find(item=>item.id===uploadId)
 const activeUpload=useJob<UploadJob>(uploadId?'/api/youtube/uploads/'+uploadId:null,
  uploadSubmission?.id===uploadId?uploadSubmission:selectedUpload??null)
 const uploadJob=activeUpload.value
 const engine=engineChoice??video.value.config.engine??'native'
 const usingComfy=engine==='comfyui'
 const ready=!video.loading&&(usingComfy?video.value.comfy.available===true:video.value.native?.available===true)
 const maxDuration=usingComfy?9:180
 const busy=video.value.jobs.filter(item=>activeStatus(item.status))
 const finishedCount=video.value.jobs.filter(item=>item.status==='completed').length
 const problemCount=video.value.jobs.filter(item=>['failed','interrupted','needs_review'].includes(item.status)).length
 const youtubeApp=oauth.value.find(item=>item.id==='youtube')
 const googleConfigured=Boolean(youtubeApp?.configured)
 const canGenerate=ready&&prompt.trim().length>0&&!generating&&!savingConfig&&busy.length===0
 const workflow=video.value.workflows[0]

 const filteredJobs=useMemo(()=>{
  const search=librarySearch.trim().toLocaleLowerCase()
  return video.value.jobs.filter(item=>{
   if(libraryFilter==='completed'&&item.status!=='completed')return false
   if(libraryFilter==='active'&&!activeStatus(item.status))return false
   if(libraryFilter==='issues'&&!['failed','interrupted','needs_review'].includes(item.status))return false
   return !search||(item.title||'').toLocaleLowerCase().includes(search)||(item.brief||'').toLocaleLowerCase().includes(search)
  })
 },[video.value.jobs,libraryFilter,librarySearch])

 useEffect(()=>{setComfyUrl(video.value.config.comfyUrl||'')},[video.value.config.comfyUrl])
 useEffect(()=>{setNarration(video.value.config.narration!==false)},[video.value.config.narration])
 useEffect(()=>{
  if(!video.loading&&!video.value.jobs.some(item=>item.id===selectedId))
   setSelectedId(video.value.jobs[0]?.id||'')
 },[video.loading,video.value.jobs,selectedId])
 useEffect(()=>{
  try{if(selectedId)sessionStorage.setItem('dominion.video.selected',selectedId)
   else sessionStorage.removeItem('dominion.video.selected')}catch{ /* storage disabled */ }
 },[selectedId])
 useEffect(()=>{
  if(usingComfy&&seconds>9)setSeconds(6)
 },[usingComfy,seconds])
 useEffect(()=>{
  if(!busy.length)return
  const timer=window.setInterval(()=>void video.reload(),5000)
  return()=>window.clearInterval(timer)
 },[busy.length,video.reload])
 useEffect(()=>{
  if(videoJob?.status==='completed')void outputs.reload()
 },[videoJob?.status,outputs.reload])
 useEffect(()=>{
  if(uploadJob?.status==='completed')void youtube.reload()
 },[uploadJob?.status,youtube.reload])
 useEffect(()=>{
  if(uploadFile||!outputs.value[0])return
  const recent=outputs.value[0]
  setUploadFile(recent.id)
  setUploadTitle((recent.title||recent.brief||'').slice(0,100))
  setUploadDescription(recent.brief||'')
 },[uploadFile,outputs.value])
 useEffect(()=>{
  const onOAuth=(event:MessageEvent)=>{
   if(event.origin!==window.location.origin||event.data?.type!=='dominion-oauth-completed')return
   void Promise.all([youtube.reload(),oauth.reload()])
  }
  window.addEventListener('message',onOAuth)
  return()=>window.removeEventListener('message',onOAuth)
 },[youtube.reload,oauth.reload])
 useEffect(()=>{
  if(!videoJob?.outputUrl||videoJob.status!=='completed'||(tab!=='create'&&tab!=='library')){
   setPreviewUrl('');setPreviewError('');setPreviewLoading(false);return
  }
  const controller=new AbortController()
  let objectUrl=''
  setPreviewUrl('');setPreviewError('');setPreviewLoading(true)
  void getApiVideoPreview(videoJob.outputUrl,controller.signal).then(url=>{
   if(controller.signal.aborted){URL.revokeObjectURL(url);return}
   objectUrl=url;setPreviewUrl(url)
  }).catch((e:unknown)=>{
   if(!controller.signal.aborted)setPreviewError((e as Error).message)
  }).finally(()=>{if(!controller.signal.aborted)setPreviewLoading(false)})
  return()=>{controller.abort();if(objectUrl)URL.revokeObjectURL(objectUrl)}
 },[videoJob?.id,videoJob?.outputUrl,videoJob?.status,tab])

 const clearMessage=()=>{setError('');setSuccess('')}
 const refreshAll=async()=>{
  setRefreshing(true);clearMessage()
  try{
   await Promise.all([video.reload(),outputs.reload(),catalog.reload(),youtube.reload(),oauth.reload()])
  }finally{setRefreshing(false)}
 }
 const saveConfig=async(patch:Record<string,unknown>)=>{
  setSavingConfig(true);clearMessage()
  try{
   await post('/api/video/config',patch)
   await Promise.all([video.reload(),catalog.reload()])
   setSuccess('설정이 저장되었습니다.')
   return true
  }catch(e){setError((e as Error).message);return false}
  finally{setSavingConfig(false)}
 }
 const changeEngine=async(next:VideoEngine)=>{
  if(savingConfig||next===engine)return
  const previous=engine
  setEngineChoice(next)
  const ok=await saveConfig({engine:next})
  if(!ok)setEngineChoice(previous)
  else if(next==='comfyui'&&seconds>9)setSeconds(6)
 }
 const importWorkflow=async(file:File)=>{
  if(savingConfig)return
  setSavingConfig(true);clearMessage()
  try{
   if(file.size>1024*1024)throw Error('API 워크플로는 1MB 이하여야 합니다.')
   if(!file.name.toLowerCase().endsWith('.json'))throw Error('JSON 파일을 선택해 주세요.')
   const graph:unknown=JSON.parse(await file.text())
   await post('/api/video/config',{engine:'comfyui',comfyWorkflow:graph,comfyWorkflowName:file.name})
   setEngineChoice('comfyui')
   await Promise.all([video.reload(),catalog.reload()])
   setSuccess('ComfyUI API 워크플로를 가져왔습니다.')
  }catch(e){setError((e as Error).message)}
  finally{setSavingConfig(false);setDragging(false)}
 }
 const saveGoogle=async()=>{
  if(gBusy)return
  setGBusy(true);clearMessage()
  try{
   await post('/api/oauth/providers/youtube',{clientId:gid.trim(),clientSecret:gsecret.trim()})
   setGsecret('');await oauth.reload();setSuccess('YouTube OAuth 앱 설정을 저장했습니다.')
  }catch(e){setError((e as Error).message)}
  finally{setGBusy(false)}
 }
 const connectGoogle=async()=>{
  clearMessage()
  try{await openOAuth('youtube')}
  catch(e){setError(t('vid.youtubeFailed',{error:(e as Error).message}))}
 }
 const generate=async()=>{
  if(!canGenerate)return
  const requestedSeed=Number(seed)
  if(!Number.isInteger(seconds)||seconds<3||seconds>maxDuration||
   (usingComfy&&(!seed.trim()||!Number.isSafeInteger(requestedSeed)||requestedSeed< -1||
    !Number.isInteger(fps)||fps<1||fps>60))){
   setError('길이, FPS 또는 시드 값을 확인해 주세요.');return
  }
  setGenerating(true);clearMessage()
  try{
   const created=await post<VideoJob>('/api/video/jobs',{
    engine,title,brief:prompt.trim(),aspectRatio:ratio,duration:seconds,
    ...(usingComfy?{negativePrompt:negative,fps,seed:requestedSeed}:{})
   })
   setVideoSubmission(created)
   setSelectedId(created.id)
   await video.reload()
   setSuccess('영상 생성 작업을 접수했습니다. 아래 진행 상태에서 확인할 수 있습니다.')
  }catch(e){setError((e as Error).message)}
  finally{setGenerating(false)}
 }
 const pickOutput=(item:VideoJob,openPublish=false)=>{
  setUploadFile(item.id)
  setUploadTitle((item.title||item.brief||'').slice(0,100))
  setUploadDescription(item.brief||'')
  if(openPublish)setTab('publish')
 }
 const startUpload=async()=>{
  if(!youtube.value.uploadAllowed){setError('먼저 YouTube 계정을 연결해 주세요.');return}
  if(!selectedOutput||!uploadTitle.trim()||uploading||activeStatus(uploadJob?.status))return
  setUploading(true);clearMessage()
  try{
   const created=await post<UploadJob>('/api/video/jobs/'+selectedOutput.id+'/upload',{
    title:uploadTitle.trim(),description:uploadDescription,
    privacyStatus:uploadPrivacy,tags:uploadTags
   })
   setUploadSubmission(created)
   setUploadId(created.id)
   await youtube.reload()
   setSuccess('YouTube 업로드를 시작했습니다. 게시 상태를 계속 확인할 수 있습니다.')
  }catch(e){setError((e as Error).message)}
  finally{setUploading(false)}
 }
 const downloadVideo=async(id:string)=>{
  clearMessage()
  try{
   const result=await post<{downloadUrl?:string}>('/api/video/jobs/'+id+'/reveal',{})
   if(!result.downloadUrl)throw Error('영상 다운로드 주소가 없습니다.')
   await downloadApiVideo(result.downloadUrl,id)
  }catch(e){setError((e as Error).message)}
 }

 return <div className="mx-auto w-full max-w-[1480px] space-y-5 overflow-x-hidden px-4 pb-12 pt-4 text-[#292722] sm:px-6 lg:px-8 lg:pt-6">
  {/* A focused production header rather than a large multi-section settings form. */}
  <header className="relative overflow-hidden rounded-[28px] bg-[#1F2A30] text-white shadow-[0_24px_60px_rgba(36,39,38,.12)]">
   <div className="pointer-events-none absolute -right-20 -top-32 size-80 rounded-full bg-[#F3AE75]/15 blur-3xl"/>
   <div className="pointer-events-none absolute -bottom-40 left-1/4 size-80 rounded-full bg-[#6BA9A2]/20 blur-3xl"/>
   <div className="relative flex flex-col gap-6 px-6 py-7 sm:px-8 md:flex-row md:items-center md:justify-between lg:px-10 lg:py-9">
    <div className="max-w-2xl">
     <div className="mb-3 flex items-center gap-2 text-[11px] font-bold uppercase tracking-[.19em] text-[#F5C19E]"><span className="inline-block size-1.5 rounded-full bg-[#F2B38B]"/>DOMINION / VIDEO STUDIO</div>
     <h1 className="text-[26px] font-bold leading-tight tracking-[-.045em] sm:text-[34px]">아이디어에서 영상, 게시까지.</h1>
     <p className="mt-2 max-w-xl text-[13px] leading-6 text-[#C2CCC9]">원하는 화면 비율을 고르고 내용을 입력하세요. 생성 작업과 결과물, YouTube 게시를 한곳에서 관리합니다.</p>
     <div className="mt-4 flex flex-wrap items-center gap-2 text-[11px]">
      <span className="rounded-full border border-white/15 bg-white/10 px-3 py-1.5 text-[#E1E8E6]">{usingComfy?'ComfyUI · AI 영상':'로컬 FFmpeg · 카드 영상'}</span>
      <span className="rounded-full border border-white/15 bg-white/10 px-3 py-1.5 text-[#E1E8E6]">{ready?'엔진 준비 완료':'설정 확인 필요'}</span>
      <span className="rounded-full border border-white/15 bg-white/10 px-3 py-1.5 text-[#E1E8E6]">{youtube.value.connected?'YouTube 연결됨':'YouTube 미연결'}</span>
     </div>
    </div>
    <div className="grid grid-cols-3 gap-2 sm:min-w-[260px]">
     {[
      {label:'전체 작업',value:video.value.jobs.length},
      {label:'제작 완료',value:finishedCount},
      {label:'진행 중',value:busy.length},
     ].map(item=><div key={item.label} className="rounded-2xl border border-white/10 bg-white/[.08] px-3 py-4 text-center backdrop-blur-sm">
      <p className="text-[23px] font-semibold tabular-nums tracking-tight text-white">{item.value}</p>
      <p className="mt-1 whitespace-nowrap text-[10px] text-[#CAD2CF]">{item.label}</p>
     </div>)}
    </div>
   </div>
  </header>

  <nav aria-label="영상 작업 단계" className="grid grid-cols-2 gap-2 rounded-[20px] border border-[#E6DFD5] bg-[#FAF8F4] p-2 lg:grid-cols-4">
   {NAV.map(({id,label,sub,icon:Icon},index)=><button
    key={id} type="button" aria-current={tab===id?'page':undefined}
    onClick={()=>{setTab(id);clearMessage()}}
    className={'group flex min-w-0 items-center gap-3 rounded-[14px] px-3 py-3 text-left outline-none transition-all focus-visible:ring-2 focus-visible:ring-[#C76C4F] sm:px-4 '+(
     tab===id?'bg-white text-[#282723] shadow-[0_5px_18px_rgba(35,31,26,.08)] ring-1 ring-[#EAE1D6]':'text-[#81766A] hover:bg-white/65'
    )}>
    <span className={'flex size-10 shrink-0 items-center justify-center rounded-xl '+(tab===id?'bg-[#FCEDE4] text-[#C36B47]':'bg-[#EFEAE2] text-[#908579]')}><Icon size={19}/></span>
    <span className="min-w-0 flex-1"><strong className="block truncate text-[12px] sm:text-[13px]">{label}</strong><small className="mt-0.5 block truncate text-[10px] text-[#9D9285]">{sub}</small></span>
    <span className="hidden text-[10px] tabular-nums text-[#AAA094] xl:block">{String(index+1).padStart(2,'0')}</span>
   </button>)}
  </nav>

  {(error||video.error||outputs.error||activeVideo.error||activeUpload.error||youtube.error)&&
   <Notice message={error||video.error||outputs.error||activeVideo.error||activeUpload.error||youtube.error}/>}
  {success&&<div role="status" className="flex items-start gap-2 rounded-xl border border-emerald-200 bg-emerald-50 px-4 py-3 text-[12px] text-emerald-800"><CheckCircle2 size={16} className="mt-0.5 shrink-0"/>{success}</div>}

  {tab==='create'&&<div className="grid items-start gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(360px,.92fr)]">
   <div className="space-y-5">
    <Panel className="overflow-hidden">
     <div className="border-b border-[#EFE9E0] px-5 py-5 sm:px-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
       <SectionTitle eyebrow="01 / CREATE" title="새 영상 만들기" detail="필요한 정보부터 입력하고, 고급 옵션은 필요할 때만 펼치세요."/>
       <button type="button" onClick={()=>setTab('settings')} className="inline-flex items-center gap-1.5 rounded-xl px-2.5 py-2 text-[11px] font-semibold text-[#A2674F] hover:bg-[#FBEEE5] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#C96442]">
        <Settings2 size={14}/> 엔진 설정 <ChevronRight size={13}/>
       </button>
      </div>
      <div className="mt-5 grid grid-cols-2 gap-2 rounded-2xl bg-[#F4F0E9] p-1.5">
       <button type="button" aria-pressed={!usingComfy} disabled={savingConfig} onClick={()=>void changeEngine('native')}
        className={'flex items-center justify-center gap-2 rounded-xl px-3 py-3 text-[12px] font-semibold transition-colors '+(!usingComfy?'bg-white text-[#3D342F] shadow-sm':'text-[#94877B] hover:text-[#5B524B]')}>
        <Cpu size={17}/> 로컬 영상
       </button>
       <button type="button" aria-pressed={usingComfy} disabled={savingConfig} onClick={()=>void changeEngine('comfyui')}
        className={'flex items-center justify-center gap-2 rounded-xl px-3 py-3 text-[12px] font-semibold transition-colors '+(usingComfy?'bg-white text-[#3D342F] shadow-sm':'text-[#94877B] hover:text-[#5B524B]')}>
        <Sparkles size={17}/> AI 영상 · ComfyUI
       </button>
      </div>
      <div className={'mt-3 flex items-start gap-2 rounded-xl border px-3.5 py-3 text-[11px] leading-5 '+(ready?'border-[#E3E9DF] bg-[#F4F8F1] text-[#55704F]':'border-[#EFD8C0] bg-[#FFF7EC] text-[#926B47]')}>
       {ready?<CheckCircle2 size={16} className="mt-0.5 shrink-0"/>:<Info size={16} className="mt-0.5 shrink-0"/>}
       <p className="flex-1">{usingComfy
        ?ready?'ComfyUI 서버와 API 워크플로가 준비됐습니다. 실제 생성 모델이 워크플로에 연결됩니다.'
         :video.value.comfy.reachable?'워크플로를 먼저 가져와 주세요. 설정에서 ComfyUI API JSON 파일을 등록합니다.'
          :'ComfyUI 연결이 필요합니다. 설정에서 서버 주소와 워크플로를 확인해 주세요.'
        :ready?'설치된 FFmpeg로 글자 카드와 내레이션을 합성합니다. AI 모션 생성은 ComfyUI를 선택하세요.'
         :'FFmpeg(drawtext 포함)가 필요합니다. 설치 및 실행 환경을 확인해 주세요.'}</p>
       {!ready&&<button type="button" className="shrink-0 font-bold underline underline-offset-2" onClick={()=>setTab('settings')}>설정</button>}
      </div>
     </div>
     <div className="space-y-6 px-5 py-6 sm:px-6">
      <div>
       <p className="mb-3 text-[12px] font-bold text-[#40372F]">1. 화면 비율 선택</p>
       <div className="grid grid-cols-3 gap-2.5">
        {(Object.keys(RATIOS) as Ratio[]).map(key=><button key={key} type="button"
         aria-pressed={ratio===key} onClick={()=>setRatio(key)}
         className={'group relative flex min-h-[120px] flex-col items-center justify-center rounded-2xl border-2 p-3 text-center transition-all focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[#B66D52] '+(
          ratio===key?'border-[#CC7858] bg-[#FFF7F1] shadow-[0_4px_12px_rgba(203,111,79,.08)]':'border-[#EEE6DC] bg-[#FAF9F6] hover:border-[#D5C7B8]'
         )}>
         {ratio===key&&<Check size={13} className="absolute right-2 top-2 text-[#B85F43]"/>}
         <span style={{aspectRatio:String(RATIOS[key].width)+'/'+String(RATIOS[key].height)}}
          className={'mb-2 flex h-10 items-center justify-center rounded-[6px] border-2 '+(ratio===key?'border-[#C86E4F] bg-[#F6E2D7]':'border-[#B8B0A4] bg-[#E9E5DE]')}>
          <Play size={13} className={ratio===key?'text-[#B6664C]':'text-[#968C80]'}/>
         </span>
         <strong className="text-[12px] text-[#3D352E]">{RATIOS[key].label}</strong>
         <small className="mt-0.5 text-[10px] text-[#94877C]">{RATIOS[key].subtitle}</small>
        </button>)}
       </div>
      </div>
      <div className="space-y-3">
       <p className="text-[12px] font-bold text-[#40372F]">2. 영상 내용</p>
       <FormField label="영상 제목">
        <TextInput value={title} onChange={e=>setTitle(e.target.value)} maxLength={180} placeholder="예: 빗속의 도시, 15초 이야기" autoComplete="off"/>
       </FormField>
       <FormField label={usingComfy?'만들고 싶은 장면 설명':'화면에 보여줄 주제와 내레이션'}>
        <TextArea rows={5} maxLength={usingComfy?10000:1000} value={prompt}
         onChange={e=>setPrompt(e.target.value)}
         placeholder={usingComfy?'장면, 분위기, 카메라 움직임, 조명과 스타일을 자유롭게 설명해 주세요.':'화면에 보일 메시지와 읽을 문장을 입력하세요.'}
         className="min-h-[132px] !leading-6"/>
       </FormField>
       <div className="flex items-center justify-between gap-2 text-[11px] text-[#94887B]">
        <span>장면을 구체적으로 작성할수록 의도가 명확해집니다.</span>
        <span className="shrink-0 tabular-nums">{prompt.length.toLocaleString()}/{usingComfy?'10,000':'1,000'}</span>
       </div>
      </div>
      <div>
       <div className="mb-3 flex items-center justify-between">
        <p className="text-[12px] font-bold text-[#40372F]">3. 영상 길이</p>
        <span className="text-[11px] text-[#958B7F]">최대 {maxDuration}초</span>
       </div>
       <div className="grid grid-cols-4 gap-2">
        {(usingComfy?[3,6,9]:[6,15,30,60]).map(value=><button type="button" key={value}
         onClick={()=>setSeconds(value)} aria-pressed={seconds===value}
         className={'rounded-xl border px-2 py-2.5 text-[12px] font-semibold transition-colors '+(seconds===value?'border-[#C97655] bg-[#FFF1E7] text-[#B45C40]':'border-[#E5DDD2] bg-white text-[#756C61] hover:bg-[#F7F2EB]')}>
         {value}초
        </button>)}
       </div>
       <label className="mt-3 flex items-center gap-3 text-[11px] text-[#80766B]">
        직접 입력
        <TextInput type="number" value={seconds} onChange={e=>setSeconds(Number(e.target.value))}
         min={3} max={maxDuration} className="!w-28 !py-2" aria-label="영상 길이 초"/>
        <span>초</span>
       </label>
      </div>
      {usingComfy&&<details className="group rounded-2xl border border-[#E8E1D7] bg-[#FCFAF6]">
       <summary className="flex cursor-pointer list-none items-center justify-between gap-2 px-4 py-3 text-[12px] font-bold text-[#61554A]">
        <span className="inline-flex items-center gap-2"><SlidersHorizontal size={15}/> 고급 생성 옵션</span>
        <span className="text-[10px] font-medium text-[#A3998E]">네거티브 · FPS · 시드</span>
       </summary>
       <div className="grid gap-4 border-t border-[#EAE2D8] px-4 py-4 sm:grid-cols-2">
        <div className="sm:col-span-2">
         <FormField label="제외할 요소">
          <TextInput value={negative} onChange={e=>setNegative(e.target.value)} maxLength={4000}
           placeholder="blurry, watermark, low quality"/>
         </FormField>
        </div>
        <FormField label="초당 프레임 (FPS)">
         <TextInput type="number" min={1} max={60} value={fps} onChange={e=>setFps(Number(e.target.value))}/>
        </FormField>
        <FormField label="시드 (-1은 무작위)">
         <div className="flex gap-2">
          <TextInput value={seed} onChange={e=>setSeed(e.target.value)} inputMode="numeric"/>
          <Button size="sm" onClick={()=>setSeed('-1')}>랜덤</Button>
         </div>
        </FormField>
       </div>
      </details>}
      {busy.length>0&&<div role="status" className="flex items-start gap-2 rounded-xl border border-sky-200 bg-sky-50 p-3 text-[12px] text-sky-800">
       <LoaderCircle size={16} className="mt-0.5 shrink-0 animate-spin"/>
       <span>현재 {busy.length}개 작업이 진행 중입니다. 완료 후 새 작업을 시작할 수 있습니다.</span>
       <button type="button" className="ml-auto shrink-0 font-bold underline underline-offset-2" onClick={()=>setTab('library')}>작업 보기</button>
      </div>}
      <Button variant="primary" className="!min-h-12 !w-full !rounded-2xl !text-[14px] !shadow-[0_10px_25px_rgba(201,100,66,.17)]"
       disabled={!canGenerate} onClick={()=>void generate()}>
       {generating?<LoaderCircle size={17} className="animate-spin"/>:<Play size={17} fill="currentColor"/>}
       {generating?'작업 제출 중...':!ready?'엔진 설정 후 생성 가능':'영상 생성 시작'}
       <ArrowRight size={16} className="ml-auto"/>
      </Button>
     </div>
    </Panel>
   </div>

   <div className="space-y-5 xl:sticky xl:top-5">
    <Panel className="overflow-hidden">
     <div className="flex flex-wrap items-center justify-between gap-3 border-b border-[#EFE9E0] px-5 py-5 sm:px-6">
      <SectionTitle eyebrow="02 / PREVIEW" title="결과 미리보기" detail="작업을 선택하면 상태와 실제 결과물을 확인합니다."/>
      <Button size="sm" variant="ghost" onClick={()=>void refreshAll()} disabled={refreshing}>
       <RefreshCw size={14} className={refreshing?'animate-spin':''}/> 새로고침
      </Button>
     </div>
     <div className="px-5 pb-5 pt-4 sm:px-6">
      {videoJob?<div className="space-y-4">
       <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
         <strong className="block truncate text-[14px] text-[#312A25]">{videoJob.title||'제목 없는 영상'}</strong>
         <p className="mt-1 text-[11px] text-[#93887D]">{dateLabel(videoJob.createdAt)} · {videoJob.aspectRatio||ratio} · {videoJob.duration||seconds}초</p>
        </div>
        <StatusPill status={videoJob.status}/>
       </div>
       <div className="relative flex min-h-[260px] items-center justify-center overflow-hidden rounded-[20px] bg-[#202A30] p-4 sm:min-h-[300px]">
        <div className="absolute inset-0 opacity-60 [background:radial-gradient(circle_at_65%_20%,#526B69_0%,transparent_55%)]"/>
        {previewUrl?<video key={previewUrl} src={previewUrl} controls playsInline preload="metadata"
         className="relative z-10 max-h-[380px] max-w-full rounded-lg shadow-2xl"
         aria-label="생성된 영상 미리보기"/>:
         <div className="relative z-10 flex max-w-xs flex-col items-center gap-3 px-3 text-center">
          <div className="flex size-16 items-center justify-center rounded-2xl border border-white/15 bg-white/10 text-[#F7CFB5]">
           {activeStatus(videoJob.status)?<LoaderCircle className="animate-spin" size={27}/>:videoJob.status==='completed'&&previewLoading?<LoaderCircle className="animate-spin" size={27}/>:<Film size={27}/>}
          </div>
          <strong className="text-[14px] text-white">{activeStatus(videoJob.status)?'영상을 제작하고 있습니다':videoJob.status==='completed'&&previewLoading?'미리보기를 준비하고 있습니다':videoJob.status==='completed'?'결과물 저장 완료':'생성 결과를 확인해 주세요'}</strong>
          <p className="text-[11px] leading-5 text-[#BECBCB]">{previewError||(
           activeStatus(videoJob.status)?'작업을 마쳐야 실제 영상을 재생할 수 있습니다.':
           videoJob.status==='completed'?'아래의 저장 버튼으로 원본 영상을 받을 수 있습니다.':
           videoJob.error||'작업 상태를 확인하거나 새로 만들어 주세요.'
          )}</p>
         </div>}
       </div>
       {activeStatus(videoJob.status)&&<div className="space-y-2" role="status" aria-live="polite">
        <div className="flex items-center justify-between text-[11px] text-[#786F66]">
         <span>서버에서 작업하는 중 · 실시간 상태 갱신</span>
         {typeof videoJob.progress==='number'&&videoJob.progress>0&&<span className="font-semibold tabular-nums">{Math.round(videoJob.progress)}%</span>}
        </div>
        {typeof videoJob.progress==='number'&&videoJob.progress>0?
         <div className="h-2 overflow-hidden rounded-full bg-[#F2EDE6]">
          <div className="h-full rounded-full bg-[#C36F52] transition-[width]" style={{width:String(Math.min(100,Math.max(0,videoJob.progress)))+'%'}}/>
         </div>:
         <div className="h-2 overflow-hidden rounded-full bg-[#ECE9E2]">
          <div className="h-full w-1/3 rounded-full bg-[#D68C6D] motion-safe:animate-pulse"/>
         </div>}
       </div>}
       {['failed','needs_review','interrupted'].includes(videoJob.status)&&<Notice message={videoJob.error||'실행 상태 확인이 필요합니다.'}/>}
       <div className="flex flex-wrap gap-2">
        <Button variant="secondary" className="flex-1" disabled={videoJob.status!=='completed'}
         onClick={()=>void downloadVideo(videoJob.id)}><Download size={15}/> 원본 영상 저장</Button>
        <Button variant="primary" className="flex-1" disabled={videoJob.status!=='completed'}
         onClick={()=>{pickOutput(videoJob,true)}}><Upload size={15}/> 게시 준비 <ArrowRight size={14}/></Button>
       </div>
       {videoJob.status==='completed'&&<p className="text-[11px] text-[#908479]">{bytesLabel(videoJob.result?.bytes)} · 서버에 저장된 원본 사용</p>}
      </div>:
      <EmptyStage title="제작 기록이 없습니다" detail="왼쪽에서 첫 영상을 만들면 진행 상태와 미리보기가 이곳에 나타납니다."/>}
     </div>
    </Panel>
    <Panel className="p-5 sm:p-6">
     <div className="mb-4 flex items-center justify-between gap-3">
      <SectionTitle eyebrow="RECENT WORK" title="최근 제작 작업"/>
      <button type="button" onClick={()=>setTab('library')} className="inline-flex items-center gap-1 text-[11px] font-semibold text-[#B66C4E] hover:underline">전체 보기 <ArrowRight size={13}/></button>
     </div>
     <div className="space-y-2">
      {video.value.jobs.length?video.value.jobs.slice(0,4).map(item=><button key={item.id} type="button" onClick={()=>setSelectedId(item.id)}
       className={'flex w-full min-w-0 items-center gap-3 rounded-xl border p-3 text-left transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#C96442] '+(videoJob?.id===item.id?'border-[#E8B99C] bg-[#FFF8F1]':'border-[#EFE8DE] hover:bg-[#FAF7F1]')}>
       <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-[#EFE6DC] text-[#94664F]"><Film size={16}/></span>
       <span className="min-w-0 flex-1"><strong className="block truncate text-[12px] text-[#484039]">{item.title||'제목 없는 영상'}</strong><small className="mt-0.5 block truncate text-[10px] text-[#9A8D80]">{dateLabel(item.createdAt)}</small></span>
       <StatusPill status={item.status}/>
      </button>):<p className="rounded-xl bg-[#FAF8F4] p-4 text-center text-[12px] text-[#92867B]">완료된 작업이 이곳에 쌓입니다.</p>}
     </div>
    </Panel>
   </div>
  </div>}

  {tab==='library'&&<Panel className="overflow-hidden">
   <div className="flex flex-wrap items-center justify-between gap-4 border-b border-[#EEE7DC] p-5 sm:p-7">
    <SectionTitle eyebrow="YOUR LIBRARY" title="모든 제작 작업" detail="진행 중인 영상과 완성본, 실패한 작업을 검색하고 다시 확인하세요."/>
    <div className="flex flex-wrap gap-2 text-[11px]">
     <StateTag online={busy.length===0}>{busy.length>0?'생성 중 '+busy.length+'개':'대기 작업 없음'}</StateTag>
     {problemCount>0&&<StatusPill status="needs_review"/>}
     <Button size="sm" variant="secondary" disabled={refreshing} onClick={()=>void refreshAll()}><RefreshCw size={14} className={refreshing?'animate-spin':''}/> 새로고침</Button>
    </div>
   </div>
   <div className="space-y-5 p-5 sm:p-7">
    <div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
     <div className="flex flex-wrap gap-2" role="group" aria-label="작업 상태 필터">
      {([
       ['all','전체',video.value.jobs.length],
       ['completed','완료',finishedCount],
       ['active','진행 중',busy.length],
       ['issues','확인 필요',problemCount],
      ] as const).map(([id,label,count])=><button key={id} type="button" aria-pressed={libraryFilter===id} onClick={()=>setLibraryFilter(id)}
       className={'rounded-xl border px-3 py-2 text-[11px] font-semibold '+(libraryFilter===id?'border-[#C97655] bg-[#FFF1E8] text-[#B65D3E]':'border-[#E9E1D6] bg-white text-[#776F65] hover:bg-[#F8F4EF]')}>
       {label} <span className="ml-1 tabular-nums opacity-65">{count}</span>
      </button>)}
     </div>
     <label className="relative block min-w-0 lg:w-64">
      <Search size={16} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-[#9C9185]"/>
      <TextInput value={librarySearch} onChange={e=>setLibrarySearch(e.target.value)} aria-label="영상 제목이나 내용 검색"
       placeholder="제목 또는 내용 검색" className="!pl-9"/>
     </label>
    </div>
    {filteredJobs.length?<div className="grid gap-3 sm:grid-cols-2 2xl:grid-cols-3">
     {filteredJobs.map(item=><button key={item.id} type="button" onClick={()=>{setSelectedId(item.id);setTab('create')}}
      className="group flex min-w-0 flex-col overflow-hidden rounded-2xl border border-[#E8E1D6] bg-[#FFFEFB] text-left transition-all hover:-translate-y-0.5 hover:border-[#DDA98D] hover:shadow-[0_10px_26px_rgba(45,39,31,.07)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[#C96442]">
      <div className="relative flex h-32 items-center justify-center overflow-hidden bg-[#27333A]">
       <div className="absolute inset-0 [background:radial-gradient(circle_at_70%_10%,#59746C_0%,transparent_60%)]"/>
       <div className="relative flex h-20 w-14 items-center justify-center rounded-lg border border-[#E7C8AF]/50 bg-[#DDB89E]/10 text-[#F3CBAE] transition-transform group-hover:scale-105" style={{aspectRatio:(item.aspectRatio==='16:9'?'16/9':item.aspectRatio==='1:1'?'1/1':'9/16')}}>
        <Video size={21}/>
       </div>
       <span className="absolute left-3 top-3 rounded-full bg-black/25 px-2 py-1 text-[10px] text-white">{item.engine==='comfyui'?'ComfyUI':'FFmpeg'}</span>
       <span className="absolute bottom-3 right-3 rounded-full bg-black/40 px-2 py-1 text-[10px] text-white">{item.aspectRatio||'9:16'}</span>
      </div>
      <div className="flex flex-1 flex-col gap-2.5 p-4">
       <div className="flex items-start justify-between gap-2"><strong className="line-clamp-2 text-[13px] leading-5 text-[#342E28]">{item.title||'제목 없는 영상'}</strong><StatusPill status={item.status}/></div>
       <p className="line-clamp-2 min-h-[32px] text-[11px] leading-4 text-[#8B8074]">{item.brief||'내용 없음'}</p>
       <div className="mt-auto flex items-center justify-between border-t border-[#F0EBE5] pt-3 text-[10px] text-[#948A7E]">
        <span className="inline-flex items-center gap-1"><Clock3 size={12}/>{dateLabel(item.createdAt)}</span>
        <span className="inline-flex items-center gap-1 font-semibold text-[#B46C52]">상세 보기 <ChevronRight size={12}/></span>
       </div>
      </div>
     </button>)}
    </div>:<EmptyStage title="조건에 맞는 작업이 없습니다" detail="검색어나 상태 필터를 변경하거나 새 영상을 만들어 보세요."/>}
   </div>
  </Panel>}

  {tab==='publish'&&<div className="grid items-start gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(310px,.72fr)]">
   <Panel className="overflow-hidden">
    <div className="border-b border-[#EFE9E0] px-5 py-5 sm:px-7">
     <SectionTitle eyebrow="03 / PUBLISH" title="YouTube에 게시" detail="완성된 영상을 선택하고 제목·공개 범위를 검토한 뒤 업로드합니다."/>
    </div>
    <div className="space-y-5 p-5 sm:p-7">
     <div className={'flex flex-wrap items-center justify-between gap-3 rounded-2xl border p-4 '+(youtube.value.connected?'border-emerald-200 bg-emerald-50':'border-[#F0E2D0] bg-[#FFF8ED]')}>
      <div className="flex items-center gap-3">
       <span className={'flex size-11 items-center justify-center rounded-xl '+(youtube.value.connected?'bg-emerald-100 text-emerald-700':'bg-white text-[#C1684B]')}><Youtube size={22}/></span>
       <div><strong className="block text-[12px]">{youtube.value.connected?'YouTube 계정 연결됨':'YouTube 계정 연결 필요'}</strong>
        <p className="mt-1 text-[11px] text-[#898074]">{youtube.value.account?.displayName||'OAuth 연결 후 업로드할 수 있습니다.'}</p></div>
      </div>
      {!youtube.value.connected&&<Button size="sm" variant="secondary" onClick={()=>googleConfigured?void connectGoogle():setTab('settings')}>
       {googleConfigured?'계정 연결':'앱 설정하기'} <ArrowRight size={13}/>
      </Button>}
     </div>
     <div className="grid gap-4">
      <FormField label="게시할 완성 영상">
       {outputs.value.length?<SelectInput value={selectedOutput?.id||''} onChange={e=>{
        const choice=outputs.value.find(item=>item.id===e.target.value)
        if(choice)pickOutput(choice)
       }}>
        {outputs.value.map(item=><option key={item.id} value={item.id}>{item.title||'제목 없는 영상'} · {bytesLabel(item.result?.bytes)}</option>)}
       </SelectInput>:<div className="rounded-xl border border-dashed border-[#E1D8CB] bg-[#FAF7F2] px-4 py-6 text-center text-[12px] text-[#8E8175]">아직 게시 가능한 완성 영상이 없습니다.</div>}
      </FormField>
      {selectedOutput&&<div className="flex items-center justify-between gap-3 rounded-xl bg-[#F5F2ED] p-3 text-[11px] text-[#82766A]">
       <span className="inline-flex min-w-0 items-center gap-2"><Film size={16} className="shrink-0 text-[#BD7657]"/><span className="truncate">{selectedOutput.title||selectedOutput.brief}</span></span>
       <span className="shrink-0">{bytesLabel(selectedOutput.result?.bytes)}</span>
      </div>}
      <FormField label="YouTube 영상 제목">
       <TextInput value={uploadTitle} onChange={e=>setUploadTitle(e.target.value)} maxLength={100} placeholder="게시할 영상의 제목"/>
      </FormField>
      <FormField label="영상 설명">
       <TextArea rows={4} value={uploadDescription} onChange={e=>setUploadDescription(e.target.value)} maxLength={4900} placeholder="설명과 출처, 관련 정보를 입력하세요."/>
      </FormField>
      <div className="grid gap-4 sm:grid-cols-2">
       <FormField label="공개 범위">
        <SelectInput value={uploadPrivacy} onChange={e=>setUploadPrivacy(e.target.value)}>
         <option value="private">비공개 · 나만 보기</option>
         <option value="unlisted">일부 공개 · 링크로 보기</option>
         <option value="public">전체 공개</option>
        </SelectInput>
       </FormField>
       <FormField label="태그">
        <TextInput value={uploadTags} onChange={e=>setUploadTags(e.target.value)} placeholder="쉼표로 구분"/>
       </FormField>
      </div>
      <div className="flex items-start gap-2 rounded-xl bg-[#F6F4F0] p-3 text-[11px] leading-5 text-[#7B7065]">
       <ShieldCheck size={16} className="mt-0.5 shrink-0 text-[#6A8274]"/>
       <span>업로드는 버튼을 누를 때만 시작됩니다. 공개 범위는 기본적으로 비공개이며, 중단된 업로드를 자동으로 중복 게시하지 않습니다.</span>
      </div>
      <Button variant="primary" className="!min-h-12 !w-full !rounded-2xl !text-[14px]" disabled={uploading||activeStatus(uploadJob?.status)||!youtube.value.uploadAllowed||!selectedOutput||!uploadTitle.trim()}
       onClick={()=>void startUpload()}>
       {uploading?<LoaderCircle size={17} className="animate-spin"/>:<CloudUpload size={18}/>}
       {uploading?'업로드 요청 중...':'YouTube 업로드 시작'}<ArrowRight size={16} className="ml-auto"/>
      </Button>
     </div>
    </div>
   </Panel>
   <div className="space-y-5">
    <Panel className="p-5 sm:p-6">
     <SectionTitle eyebrow="UPLOAD MONITOR" title="게시 상태"/>
     {uploadJob?<div className="mt-5 space-y-3">
      <div className="flex items-start justify-between gap-2"><strong className="min-w-0 truncate text-[13px]">{uploadJob.title||uploadTitle||'YouTube 영상'}</strong><StatusPill status={uploadJob.status}/></div>
      {activeStatus(uploadJob.status)&&<div className="h-2 overflow-hidden rounded-full bg-[#EFECE5]"><div className="h-full w-1/3 bg-[#C97551] motion-safe:animate-pulse"/></div>}
      {uploadJob.result?.url&&<a href={uploadJob.result.url} target="_blank" rel="noopener noreferrer" className="inline-flex items-center gap-1.5 text-[12px] font-semibold text-[#BC684B] hover:underline">YouTube에서 영상 보기 <ExternalLink size={14}/></a>}
      {uploadJob.error&&<Notice message={uploadJob.error}/>}
      {uploadJob.status==='needs_review'&&<p className="text-[11px] leading-5 text-[#916D46]">이전 업로드가 중단됐습니다. YouTube 스튜디오에서 게시 여부를 먼저 확인해 주세요.</p>}
     </div>:<EmptyStage title="진행 중인 업로드 없음" detail="영상을 선택해 게시하면 업로드 이력이 이곳에 나타납니다."/>}
    </Panel>
    <Panel className="p-5 sm:p-6">
     <div className="mb-4 flex items-center justify-between"><SectionTitle eyebrow="HISTORY" title="최근 게시 이력"/><StateTag online={youtube.value.connected}>{youtube.value.connected?'연결됨':'미연결'}</StateTag></div>
     {youtube.value.uploads.length?<div className="space-y-2">
      {youtube.value.uploads.slice(0,6).map(item=><button type="button" key={item.id} onClick={()=>{setUploadId(item.id);setUploadSubmission(item)}}
       className={'flex w-full min-w-0 items-center justify-between gap-2 rounded-xl border p-3 text-left hover:bg-[#FAF7F1] '+(uploadId===item.id?'border-[#E6B595] bg-[#FFF9F2]':'border-[#EFE8DE]')}>
       <span className="min-w-0 flex-1"><strong className="block truncate text-[11px]">{item.title||'YouTube 업로드'}</strong><small className="mt-1 block text-[10px] text-[#998C7F]">{dateLabel(item.createdAt)}</small></span>
       <StatusPill status={item.status}/>
      </button>)}
     </div>:<p className="rounded-xl bg-[#FAF8F5] p-4 text-[12px] text-[#938579]">이전 게시 기록이 없습니다.</p>}
    </Panel>
   </div>
  </div>}

  {tab==='settings'&&<div className="grid items-start gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(350px,.8fr)]">
   <div className="space-y-5">
    <Panel className="p-5 sm:p-7">
     <SectionTitle eyebrow="ENGINE SETTINGS" title="영상 생성 엔진" detail="작업에 맞는 엔진을 연결하고 생성 품질을 조절합니다."/>
     <div className="mt-5 grid gap-2 sm:grid-cols-2">
      {(['native','comfyui'] as VideoEngine[]).map(choice=><button key={choice} type="button" onClick={()=>void changeEngine(choice)} disabled={savingConfig} aria-pressed={engine===choice}
       className={'flex items-start gap-3 rounded-2xl border-2 p-4 text-left '+(engine===choice?'border-[#C87655] bg-[#FFF7EF]':'border-[#E9E1D6] bg-[#FAF9F6] hover:border-[#D7C6B5]')}>
       <span className="rounded-lg bg-white p-2 text-[#B97155]">{choice==='native'?<Cpu size={21}/>:<Sparkles size={21}/>}</span>
       <span><strong className="block text-[13px]">{choice==='native'?'로컬 FFmpeg':'ComfyUI · AI 생성'}</strong>
        <small className="mt-1 block text-[11px] leading-5 text-[#887C70]">{choice==='native'?'제목 카드·자막·음성 합성':'로컬 생성 서버·API 워크플로'}</small>
       </span>
      </button>)}
     </div>
     {engine==='comfyui'&&<div className="mt-5 space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
       <strong className="text-[12px]">ComfyUI 서버</strong>
       <StateTag online={video.value.comfy.reachable}>{video.value.comfy.reachable?'연결 가능':'연결 안 됨'}</StateTag>
      </div>
      <div className="flex flex-col gap-2 sm:flex-row">
       <TextInput value={comfyUrl} onChange={e=>setComfyUrl(e.target.value)} aria-label="ComfyUI 서버 주소" placeholder="http://127.0.0.1:8188"/>
       <Button disabled={savingConfig||!comfyUrl.trim()} onClick={()=>void saveConfig({engine:'comfyui',comfyUrl:comfyUrl.trim()})}>{savingConfig?'저장 중':'주소 저장'}</Button>
      </div>
      <p className="text-[11px] leading-5 text-[#8E8276]">현재 Rust 서버는 같은 컴퓨터의 HTTP 루프백 주소만 연결합니다.</p>
      <div className={'rounded-2xl border-2 border-dashed px-4 py-6 text-center transition-colors '+(dragging?'border-[#C57858] bg-[#FFF0E7]':'border-[#DDD3C8] bg-[#FAF8F4]')}
       onDragOver={e=>{e.preventDefault();setDragging(true)}}
       onDragLeave={e=>{e.preventDefault();setDragging(false)}}
       onDrop={e=>{e.preventDefault();setDragging(false);const file=e.dataTransfer.files[0];if(file)void importWorkflow(file)}}>
       <div className="mx-auto mb-3 flex size-11 items-center justify-center rounded-xl bg-white text-[#BF7756]"><FileJson2 size={23}/></div>
       <strong className="block text-[13px]">API 워크플로 JSON 등록</strong>
       <p className="mx-auto mt-2 max-w-sm text-[11px] leading-5 text-[#8E8276]">ComfyUI에서 ‘File → Export Workflow (API)’로 내보낸 JSON을 끌어놓거나 선택하세요. 일반 저장용 JSON과 형식이 다릅니다.</p>
       <label className="mt-3 inline-flex cursor-pointer items-center gap-2 rounded-xl border border-[#DBCDC0] bg-white px-4 py-2.5 text-[12px] font-semibold text-[#7C5746] hover:bg-[#FFF5EC] focus-within:ring-2 focus-within:ring-[#C96442]">
        <Upload size={14}/> {savingConfig?'등록 중...':'JSON 파일 선택'}
        <input className="sr-only" type="file" accept=".json,application/json" disabled={savingConfig}
         onChange={e=>{const file=e.currentTarget.files?.[0];e.currentTarget.value='';if(file)void importWorkflow(file)}}/>
       </label>
      </div>
      {workflow&&<div className="flex items-center gap-3 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-[12px] text-emerald-800">
       <CheckCircle2 size={18}/><span className="min-w-0 flex-1 truncate">{workflow.name}</span><strong>등록됨</strong>
      </div>}
      <div className="grid gap-3 sm:grid-cols-3">
       <FormField label="설치된 모델">
        <SelectInput value={video.value.config.videoModel||''} disabled={savingConfig}
         onChange={e=>void saveConfig({videoModel:e.target.value})}>
         <option value="">워크플로 기본 모델</option>
         {catalog.value.videoModels.map(model=><option key={model.id} value={model.id}>{model.name}</option>)}
        </SelectInput>
       </FormField>
       <FormField label="해상도">
        <SelectInput value={video.value.config.resolution||'480x832'} disabled={savingConfig}
         onChange={e=>void saveConfig({resolution:e.target.value})}>
         {['256x448','384x672','480x832','832x480','448x448'].map(size=><option key={size} value={size}>{size}</option>)}
        </SelectInput>
       </FormField>
       <FormField label="생성 스텝">
        <SelectInput value={String(video.value.config.steps??20)} disabled={savingConfig}
         onChange={e=>void saveConfig({steps:Number(e.target.value)})}>
         {[4,8,12,20,30,40].map(n=><option key={n} value={n}>{n}</option>)}
        </SelectInput>
       </FormField>
      </div>
      <p className="text-[11px] leading-5 text-[#90857A]">{catalog.value.videoModels.length
       ?'연결된 ComfyUI가 보고한 실제 설치 모델만 표시합니다. 모델 파일 설치는 ComfyUI에서 진행하세요.'
       :'서버 연결 후 설치 모델을 조회합니다. 모델을 지정하지 않으면 가져온 워크플로의 기본 모델을 사용합니다.'}</p>
     </div>}
     {engine==='native'&&<div className="mt-5 space-y-3">
      <div className="flex flex-wrap items-center justify-between rounded-xl bg-[#F7F5F0] p-4">
       <div className="flex items-center gap-2"><HardDrive size={17} className="text-[#A66F51]"/><span className="text-[12px] font-semibold">FFmpeg 실행 환경</span></div>
       <StateTag online={Boolean(video.value.native?.available)}>{video.value.native?.available?'사용 가능':'설치 확인 필요'}</StateTag>
      </div>
      <label className="flex cursor-pointer items-center justify-between rounded-xl border border-[#EBE2D8] bg-[#FFFDF9] px-4 py-3 text-[12px]">
       <span><strong className="block">내레이션 자동 생성</strong><small className="mt-1 block text-[11px] text-[#938478]">설치된 시스템 음성 엔진이 있을 때 합성합니다.</small></span>
       <input type="checkbox" checked={narration} disabled={savingConfig} onChange={e=>{setNarration(e.target.checked);void saveConfig({narration:e.target.checked})}}
        className="size-5 accent-[#C77351]"/>
      </label>
     </div>}
    </Panel>
   </div>
   <Panel className="p-5 sm:p-7">
    <SectionTitle eyebrow="ACCOUNT CONNECTION" title="YouTube 계정 연결" detail="게시 기능을 사용하기 위한 Google OAuth 설정입니다."/>
    <div className="mt-5 flex flex-wrap items-center justify-between gap-3 rounded-xl bg-[#F5F2EB] p-4">
     <span className="inline-flex items-center gap-2 text-[12px] font-semibold"><Youtube size={19} className="text-[#C75F50]"/>YouTube 연결 상태</span>
     <StateTag online={youtube.value.connected}>{youtube.value.connected?'계정 연결됨':googleConfigured?'앱 설정됨':'연결 필요'}</StateTag>
    </div>
    {youtube.value.account?.displayName&&<p className="mt-3 text-[12px] text-[#88776B]">연결 계정: {youtube.value.account.displayName}</p>}
    <div className="mt-5 space-y-3">
     <FormField label="Google OAuth Client ID"><TextInput value={gid} onChange={e=>setGid(e.target.value)} placeholder="Client ID" autoComplete="off"/></FormField>
     <FormField label="Google OAuth Client Secret"><TextInput type="password" value={gsecret} onChange={e=>setGsecret(e.target.value)} placeholder="Client Secret" autoComplete="off"/></FormField>
     {youtubeApp?.callback&&<div className="rounded-xl bg-[#F5F3EE] p-3">
      <p className="mb-1 text-[11px] font-semibold text-[#6C6258]">승인된 리디렉션 URI</p>
      <code className="block break-all text-[10px] leading-5 text-[#7B6E61]">{youtubeApp.callback}</code>
     </div>}
     <Button variant="secondary" className="w-full" disabled={gBusy||!gid.trim()||!gsecret.trim()} onClick={()=>void saveGoogle()}>
      <LockKeyhole size={15}/>{gBusy?'앱 설정 저장 중...':'OAuth 앱 설정 저장'}
     </Button>
     <Button variant="primary" className="w-full" disabled={!googleConfigured} onClick={()=>void connectGoogle()}>
      <Youtube size={16}/>{youtube.value.connected?'다른 계정 연결':'YouTube 로그인'}<ExternalLink size={14} className="ml-auto"/>
     </Button>
    </div>
    <p className="mt-4 text-[11px] leading-5 text-[#97897C]">OAuth 앱이 구성돼야 로그인할 수 있습니다. 계정 연결 후 ‘YouTube 게시’ 메뉴에서 완성 영상을 업로드할 수 있습니다.</p>
   </Panel>
  </div>}
 </div>
}
