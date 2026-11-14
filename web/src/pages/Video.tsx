import {useEffect,useState} from 'react'
import {Check,Clapperboard,ExternalLink,LoaderCircle,Play,Power,RefreshCw,Upload} from 'lucide-react'
import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {Action,FormField,Headline,Notice,Panel,StateTag,Subheading,useRemote} from '../components/UI'
import {openOAuth,post} from '../api'
import {jobFinished,useJob,type JobState} from '../hooks/useJob'
import {t,useLocale} from '../i18n'

type Workflow={id:string;name:string;bytes:number}
type ClipQuality={verified:boolean;durationSeconds?:number;width?:number;height?:number;audioStream?:boolean;reason?:string}
type VideoQuote={model:string;seconds:number;clips:number;creditsMicro:number;referenceKrw:number;referenceFxKrwPerUsd:number;fxAsOf:string;pricingAsOf:string;promotional?:boolean;type:string}
type VideoJob=JobState & {costQuote?:VideoQuote|null;attemptedExposureCreditsMicro?:number;attemptedSubmissions?:number;progress?:number;kind:'video';title?:string;brief:string;outputUrl?:string;result?:{bytes:number;clipQuality?:ClipQuality[];finalQuality?:ClipQuality;shotCount?:number;frameAnchoredClips?:number;mode?:string;shotResults?:{index:number;duration:number;bytes:number;reused:boolean;technicalRetries:number;qualityReceipt?:ClipQuality}[]}}
type VideoEngine='native'|'comfyui'|'runway'
type VideoStatus={config:{engine:VideoEngine;comfyUrl:string;runwayModel?:string;runwayAudio?:boolean;runwayContinuityFrames?:boolean;runwayQualityGate?:boolean;hasVideoKey?:boolean};native?:{available:boolean};comfy:{reachable:boolean;available?:boolean};runway?:{available:boolean;model?:string;nativeAudio?:boolean};workflows:Workflow[];jobs:VideoJob[]}
type UploadJob=JobState & {progress?:number;result?:{url:string}}
type YoutubeStatus={connected:boolean;uploadAllowed:boolean;account?:{displayName:string;scopes:string}|null;uploads:UploadJob[]}
const ratios={'9:16':{width:360,height:640,label:'vid.ratio.shorts'},'16:9':{width:640,height:360,label:'vid.ratio.landscape'},'1:1':{width:480,height:480,label:'vid.ratio.square'}} as const
const terminal=jobFinished
const statusLabel=(status:string)=>status==='completed'?t('vid.status.completed'):status==='running'?t('vid.status.running'):status==='queued'?t('vid.status.queued'):status==='downloading'?t('vid.status.downloading'):status==='failed'?t('vid.status.failed'):status
const fmtBytes=(bytes=0)=>bytes>=1024**3?(bytes/1024**3).toFixed(1)+' GB':bytes>=1024**2?Math.round(bytes/1024**2)+' MB':'0 MB'

export function VideoPage({navigate}:{navigate:(page:any)=>void}){
 useLocale()
 const video=useRemote<VideoStatus>('/api/video/status',{config:{engine:'comfyui',comfyUrl:''},comfy:{reachable:false},workflows:[],jobs:[]},false)
 const youtube=useRemote<YoutubeStatus>('/api/youtube/status',{connected:false,uploadAllowed:false,account:null,uploads:[]})
 const oauth=useRemote<{id:string;configured:boolean;callback?:string}[]>('/api/oauth/providers',[])
 const outputs=useRemote<VideoJob[]>('/api/video/outputs',[])
 const [workflow,setWorkflow]=useState(''),[title,setTitle]=useState(''),[prompt,setPrompt]=useState(''),[negative,setNegative]=useState(''),[ratio,setRatio]=useState<keyof typeof ratios>('9:16'),[seconds,setSeconds]=useState(3),[fps,setFps]=useState(16),[seed,setSeed]=useState('-1')
 const [cinematic,setCinematic]=useState(false),[badShotRetries,setBadShotRetries]=useState(0)
 const [engineChoice,setEngine]=useState<VideoEngine>(),[comfyUrl,setComfyUrl]=useState(''),[savingConfig,setSavingConfig]=useState(false)
 const [runwayKey,setRunwayKey]=useState(''),[runwayModel,setRunwayModel]=useState('kling3.0_pro'),[runwayAudio,setRunwayAudio]=useState(true),[runwayFrames,setRunwayFrames]=useState(true),[runwayQuality,setRunwayQuality]=useState(true)
 const [tab,setTab]=useState<'create'|'settings'>('create'),[refreshing,setRefreshing]=useState(false),[gid,setGid]=useState(''),[gsecret,setGsecret]=useState(''),[gBusy,setGBusy]=useState(false)
 const [videoSubmission,setVideoSubmission]=useState<VideoJob|null>(null),[generating,setGenerating]=useState(false),[error,setError]=useState('')
 const [paidQuote,setPaidQuote]=useState<VideoQuote|null>(null),[quoteError,setQuoteError]=useState('')
 const [paidApproved,setPaidApproved]=useState(false)
 const [uploadFile,setUploadFile]=useState(''),[uploadTitle,setUploadTitle]=useState(''),[uploadDescription,setUploadDescription]=useState(''),[uploadPrivacy,setUploadPrivacy]=useState('private'),[uploadTags,setUploadTags]=useState('')
 const [uploadSubmission,setUploadSubmission]=useState<UploadJob|null>(null),[uploading,setUploading]=useState(false)
 const activeVideo=useJob(videoSubmission?'/api/video/jobs/'+videoSubmission.id:null,videoSubmission)
 const activeUpload=useJob(uploadSubmission?'/api/youtube/uploads/'+uploadSubmission.id:null,uploadSubmission)
 const videoJob=activeVideo.value??outputs.value[0]
 const uploadJob=activeUpload.value
 const uploadStatus=oauth.value.find(item=>item.id==='youtube')
 const gReady=uploadStatus?.configured
 const gCallback=uploadStatus?.callback
 const selectedOutput=uploadFile
  ? outputs.value.find(item=>item.id===uploadFile)??(videoJob?.id===uploadFile&&videoJob.outputUrl?videoJob:undefined)
  : outputs.value[0]
 const engine=engineChoice??video.value.config.engine
 const usingComfy=engine==='comfyui'
 const savedRunwayModel=video.value.config.runwayModel||'kling3.0_pro'
 const runwayClipSeconds=savedRunwayModel==='kling_4'?30:15
 const ready=usingComfy?Boolean(video.value.comfy.available&&workflow&&(!cinematic||video.value.native?.available)):engine==='runway'?Boolean(video.value.runway?.available&&(seconds<=runwayClipSeconds||video.value.native?.available)):Boolean(video.value.native?.available)
 const maxDuration=usingComfy?(cinematic?120:9):180
 // Only a fresh quote from the server can authorize a paid request.
 useEffect(()=>{
  let live=true
  setPaidQuote(null);setQuoteError('');setPaidApproved(false)
  if(engine!=='runway'||!Number.isInteger(seconds)||seconds<3||seconds>180)return
  const timer=window.setTimeout(()=>{
   void post<VideoQuote>('/api/video/quote',{duration:seconds}).then(quote=>{
    if(live&&quote.model===savedRunwayModel&&quote.seconds===seconds)setPaidQuote(quote)
    else if(live)setQuoteError('저장된 모델 설정이 변경됐습니다. 견적을 다시 조회하세요.')
   }).catch(err=>{if(live)setQuoteError((err as Error).message)})
  },250)
  return()=>{live=false;window.clearTimeout(timer)}
 },[engine,seconds,savedRunwayModel,video.value.config.runwayAudio])
 useEffect(()=>{setComfyUrl(video.value.config.comfyUrl)},[video.value.config.comfyUrl])
 useEffect(()=>{setRunwayModel(video.value.config.runwayModel||'kling3.0_pro');setRunwayAudio(video.value.config.runwayAudio!==false);setRunwayFrames(video.value.config.runwayContinuityFrames!==false);setRunwayQuality(video.value.config.runwayQualityGate!==false)},[video.value.config.runwayModel,video.value.config.runwayAudio,video.value.config.runwayContinuityFrames,video.value.config.runwayQualityGate])
 useEffect(()=>{
  if(!video.value.workflows.some(item=>item.id===workflow))setWorkflow(video.value.workflows[0]?.id||'')
 },[video.value.workflows,workflow])
 useEffect(()=>{
  if(activeVideo.value?.status==='completed')void outputs.reload()
 },[activeVideo.value?.status,outputs.reload])
 const saveGoogle=async()=>{
  setGBusy(true);setError('');
  try{await post('/api/oauth/providers/youtube',{clientId:gid.trim(),clientSecret:gsecret.trim()});setGsecret('');await oauth.reload()}
  catch(e){setError((e as Error).message)}finally{setGBusy(false)}
 }
 const connectGoogle=async()=>{
  setError('');
  try{await openOAuth('youtube')}catch(e){setError(t('vid.youtubeFailed',{error:(e as Error).message}))}
 }
 const changeEngine=async(next:VideoEngine)=>{
  if(savingConfig)return;
  setEngine(next);setSavingConfig(true);setError('');
  try{await post('/api/video/config',{engine:next,comfyUrl:comfyUrl.trim()||video.value.config.comfyUrl});await video.reload()}
  catch(e){setError((e as Error).message)}finally{setSavingConfig(false)}
 }
 const saveRunway=async()=>{
  if(savingConfig)return;
  setSavingConfig(true);setError('');
  try{
   await post('/api/video/config',{engine:'runway',runwayModel,runwayAudio,runwayContinuityFrames:runwayFrames,runwayQualityGate:runwayQuality,...(runwayKey.trim()?{videoApiKey:runwayKey.trim()}:{})});
   setRunwayKey('');await video.reload();
  }catch(e){setError((e as Error).message)}finally{setSavingConfig(false)}
 }
 const saveComfy=async()=>{
  if(savingConfig)return;
  setSavingConfig(true);setError('');
  try{await post('/api/video/config',{engine:'comfyui',comfyUrl:comfyUrl.trim()});await video.reload()}
  catch(e){setError((e as Error).message)}finally{setSavingConfig(false)}
 }
 const importWorkflow=async(file:File)=>{
  if(savingConfig)return;
  setSavingConfig(true);setError('');
  try{
   if(file.size>1024*1024)throw Error(t('vid.workflowTooBig'))
   const comfyWorkflow:unknown=JSON.parse(await file.text());
   await post('/api/video/config',{engine:'comfyui',comfyWorkflow,comfyWorkflowName:file.name});
   await video.reload();
  }catch(e){setError((e as Error).message)}finally{setSavingConfig(false)}
 }
 const reveal=async(id:string)=>{
  try{await post('/api/video/jobs/'+id+'/reveal',{})}catch(e){setError((e as Error).message)}
 }
 const generate=async()=>{
  if(!ready||!prompt.trim()||generating)return;
  const requestedSeed=Number(seed);
  if(!Number.isInteger(seconds)||seconds<3||seconds>maxDuration||
   (usingComfy&&(!seed.trim()||!Number.isSafeInteger(requestedSeed)||requestedSeed < -1||!Number.isInteger(fps)||fps<1||fps>60))){
   setError(t('vid.checkValues'));return;
  }
  if(engine==='runway'&&(!paidApproved||!paidQuote||paidQuote.seconds!==seconds||paidQuote.model!==savedRunwayModel)){
   setError('유료 영상의 현재 견적을 확인하고 승인해야 합니다.');return;
  }
  setGenerating(true);setError('');
  try{
   setVideoSubmission(await post<VideoJob>('/api/video/jobs',{
    engine,title,brief:prompt,aspectRatio:ratio,duration:seconds,
    ...(engine==='runway'?{approvedPaidQuote:true,maxCreditsMicro:paidQuote!.creditsMicro}:{}),
    ...(usingComfy?{negativePrompt:negative,fps,seed:requestedSeed,cinematic,badShotRetries:cinematic?badShotRetries:0}:{}),
   }));
  }catch(e){setError((e as Error).message)}finally{setGenerating(false)}
 }
 const useResult=()=>{
  if(!videoJob?.outputUrl)return
  setUploadFile(videoJob.id)
  setUploadTitle((videoJob.title||videoJob.brief).slice(0,90))
  setUploadDescription(videoJob.brief)
 }
 const startUpload=async()=>{
  if(!youtube.value.uploadAllowed){setError(t('vid.connectYoutubeFirst'));return}
  if(!selectedOutput||!uploadTitle.trim()||uploading)return;
  setUploading(true);setError('');
  try{
   setUploadSubmission(await post<UploadJob>('/api/video/jobs/'+selectedOutput.id+'/upload',{
    title:uploadTitle,description:uploadDescription,privacyStatus:uploadPrivacy,tags:uploadTags,
   }));
  }catch(e){setError((e as Error).message)}finally{setUploading(false)}
 }
 return <div className="mx-auto w-full max-w-[1400px] overflow-x-hidden px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
  <Headline eyebrow={t('ui.eyebrow.video')} title={t('video.title')} description={t('video.description')} actions={<div className="flex items-center gap-2"><Button variant="secondary" onClick={()=>void connectGoogle()}>{t('vid.connectYoutube')}</Button><Button variant="secondary" disabled={refreshing} onClick={async()=>{setRefreshing(true);try{await Promise.allSettled([video.reload(),youtube.reload()])}finally{setRefreshing(false)}}}><RefreshCw size={16} className={refreshing?'animate-spin':''}/> {refreshing?t('video.refreshing'):t('video.refresh')}</Button></div>}/>
  {(error||video.error||outputs.error||activeVideo.error||activeUpload.error)&&<Notice message={error||video.error||outputs.error||activeVideo.error||activeUpload.error}/>}
  <div className="mb-3 flex flex-wrap items-center gap-2"><Button variant={tab==='create'?'primary':'ghost'} onClick={()=>setTab('create')}>{t('video.tab.create')}</Button><Button variant={tab==='settings'?'primary':'ghost'} onClick={()=>setTab('settings')}>{t('video.tab.settings')}</Button></div>
  {tab==='settings'&&<>
  <Panel className="mb-4 p-4">
   <div className="flex flex-wrap items-center justify-between gap-3">
    <div className="flex items-center gap-2"><Clapperboard size={18} className="text-[#C96442]"/><strong className="text-[13px] text-[#24211D]">{t('vid.engine')}</strong></div>
    <div className="flex flex-wrap items-center gap-2">
     <SelectInput aria-label={t('vid.engine')} value={engine} onChange={e=>void changeEngine(e.target.value as VideoEngine)} disabled={savingConfig} className="!w-auto !text-[12px]"><option value="native">{t('vid.engine.native')}</option><option value="comfyui">{t('vid.engine.comfy')}</option><option value="runway">시네마틱 영상 · Kling 3.0/4.0 (유료)</option></SelectInput>
    </div>
   </div>
  </Panel>
  <div className="mb-4 grid grid-cols-1 items-start gap-3 md:grid-cols-2">
   {engine==='runway'&&<Panel className="p-4">
    <div className="flex items-center justify-between gap-3"><strong className="text-[13px] text-[#24211D]">🎬 턴키 시네마틱 생성</strong><StateTag online={Boolean(video.value.runway?.available)}>{video.value.runway?.available?'키 등록됨':'Runway Dev 키 필요'}</StateTag></div>
    <p className="mt-2 text-xs leading-5 text-[#5C544A]">2026년 Kling 3.0 멀티샷 / Kling 4.0 장면 연속 생성. 장면 계획 → 영상·오디오 → 결과 다운로드 → MP4 자동 합성. 사용량은 길이와 모델에 따라 유료 청구되며 실행 전 엔진을 선택해야 합니다.</p>
    <div className="mt-3 grid gap-2"><FormField label="Runway Dev API Key"><TextInput type="password" value={runwayKey} onChange={e=>setRunwayKey(e.target.value)} placeholder={video.value.config.hasVideoKey?'저장됨 · 변경하려면 새 키 입력':'API 키 입력'}/></FormField>
     <FormField label="화질 / 모델"><SelectInput value={runwayModel} onChange={e=>setRunwayModel(e.target.value)}><option value="kling3.0_standard">Kling 3.0 Standard · 720p</option><option value="kling3.0_pro">Kling 3.0 Pro · 1080p</option><option value="kling3.0_4k">Kling 3.0 4K</option><option value="kling_4">Kling 4.0 · 1080p · 최대 30초 단일 클립</option></SelectInput></FormField>
     <label className="flex items-center gap-2 text-xs"><input type="checkbox" checked={runwayAudio} disabled={runwayModel==='kling_4'} onChange={e=>setRunwayAudio(e.target.checked)}/> 네이티브 사운드 및 대사 생성{runwayModel==='kling_4'?' (4.0은 기본 포함)':''}</label>
     <label className="flex items-center gap-2 text-xs"><input type="checkbox" checked={runwayFrames} onChange={e=>setRunwayFrames(e.target.checked)}/> 클립 사이 인물·장면 연속성 유지 (이전 마지막 프레임 활용)</label>
     <label className="flex items-center gap-2 text-xs"><input type="checkbox" checked={runwayQuality} onChange={e=>setRunwayQuality(e.target.checked)}/> 유료 클립 자동 기술 검수 · 길이·화면비·해상도·오디오 (FFprobe 필요)</label>
     <p className="text-[11px] leading-5 text-[#8A8176]">Kling 3.0은 최대 15초 네이티브 멀티샷, Kling 4.0은 최대 30초 연속 촬영입니다. 단일 클립 길이를 넘으면 앞 클립의 실제 마지막 프레임으로 다음 클립을 시작합니다. 장면 전환이 확연히 달라야 하는 영상은 연속성 옵션을 해제하세요.</p>
     <Button variant="secondary" disabled={savingConfig} onClick={()=>void saveRunway()}>{savingConfig?'저장 중': '영상 모델 · 키 저장'}</Button>
    </div>
   </Panel>}
   {engine==='comfyui'&&<Panel className={'p-4 '+(video.value.comfy.reachable?'border-emerald-200 bg-emerald-50/50':'border-amber-200 bg-amber-50/50')}>
    <div className="flex items-center justify-between gap-3"><div className="flex items-center gap-2"><Clapperboard size={18} className={video.value.comfy.reachable?'text-emerald-700':'text-amber-700'}/><strong className="text-[13px] text-[#24211D]">{t('vid.comfyTitle')}</strong></div><StateTag online={video.value.comfy.reachable}>{video.value.comfy.reachable?t('vid.reachable'):t('vid.unreachable')}</StateTag></div>
    <p className="mt-2 text-sm text-[#5C544A]">{t('vid.comfyNote')}</p>
    <div className="mt-2 flex flex-wrap gap-2"><TextInput aria-label={t('vid.comfyUrl')} value={comfyUrl} onChange={e=>setComfyUrl(e.target.value)} placeholder="http://127.0.0.1:8188"/><Button variant="secondary" disabled={savingConfig||!comfyUrl.trim()} onClick={()=>void saveComfy()}>{savingConfig?t('agents.saving'):t('vid.save')}</Button></div>
   </Panel>}
  </div>
   {usingComfy&&<div className="mb-4 rounded-xl border border-[#EED9CC] bg-[#FFF9F5] p-4">
     <strong className="text-[12px] text-[#3A342D]">{video.value.workflows.length?t('vid.savedWorkflow',{name:video.value.workflows[0].name}):t('vid.noWorkflow')}</strong>
     <ol className="mt-2 grid gap-1 text-[11px] leading-5 text-[#5C544A]">
      <li>{t('vid.step1')}</li>
      <li>{t('vid.step2')}</li>
      <li>{t('vid.step3')}</li>
     </ol>
     <div className="mt-2 flex flex-wrap items-center gap-2"><label className="inline-flex cursor-pointer items-center gap-2 rounded-xl border border-[#E1D9CC] bg-[#FFFDF9] px-3 py-2 text-[12px] font-semibold text-[#3A342D] hover:bg-[#F7F2EA]">
           {t('vid.openJson')}
           <input type="file" className="hidden" accept=".json,application/json" disabled={savingConfig} onChange={e=>{
             const file=e.currentTarget.files?.[0];e.currentTarget.value='';
             if(file)void importWorkflow(file);
           }}/>
         </label><Button variant="ghost" onClick={()=>void video.reload()}>{t('vid.recheck')}</Button></div>
    </div>}
  </>}
  <div className="grid grid-cols-1 items-start gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(320px,380px)]">
   <div className="grid min-w-0 content-start gap-4">
    {tab==='create'&&<Panel className="min-w-0 p-5">
     <Subheading title={t('vid.generate')} extra={<span className="text-[11px] text-[#8A8176]">{usingComfy?t('vid.workflowCount',{count:video.value.workflows.length}):engine==='runway'?('Kling '+(savedRunwayModel==='kling_4'?'4.0':'3.0')+' · 자동 장면 합성'):t('vid.ffmpegCard')}</span>}/>
     <div className="grid gap-4">
      <FormField label={t('vid.videoTitle')} hint={t('vid.videoTitleHint')}><TextInput value={title} onChange={e=>setTitle(e.target.value)} placeholder={t('vid.videoTitlePlaceholder')}/></FormField>
      <FormField label={t('vid.prompt')}><TextArea rows={4} value={prompt} onChange={e=>setPrompt(e.target.value)} placeholder={t('vid.promptPlaceholder')}/></FormField>
      {usingComfy&&<FormField label={t('vid.negative')} hint={t('vid.negativeHint')}><TextInput value={negative} onChange={e=>setNegative(e.target.value)} placeholder="low quality, blurry, watermark"/></FormField>}
      {usingComfy&&<div className="space-y-2 rounded-xl border border-[#E3D9CB] p-3 text-xs">
       <label className="flex items-center gap-2 font-semibold"><input type="checkbox" checked={cinematic} onChange={e=>{setCinematic(e.target.checked);if(!e.target.checked&&seconds>9)setSeconds(9)}}/> 🎬 멀티샷 영화 만들기 (3~120초)</label>
       <p className="text-[#7C7469]">장면별 생성 후 FFprobe로 기술 검수하고, 이미 검증된 클립은 재사용합니다. FFmpeg·FFprobe 및 &#123;&#123;prompt&#125;&#125;, &#123;&#123;duration&#125;&#125; 워크플로 변수가 필요합니다.</p>
       {cinematic&&<FormField label="기술 검수 실패 장면 재생성" hint="0회: 추가 GPU 비용 없음 · 1회: 불량 장면만 한 번 더 생성"><SelectInput value={String(badShotRetries)} onChange={e=>setBadShotRetries(Number(e.target.value))}><option value="0">재생성 없음</option><option value="1">최대 1회 재생성</option></SelectInput></FormField>}
       {cinematic&&!video.value.native?.available&&<p className="text-amber-700">멀티샷 최종 병합에 FFmpeg가 필요합니다.</p>}
      </div>}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3"><FormField label={t('vid.ratio')} hint={engine==='runway'?'선택 모델에 따라 720p / 1080p / 4K':`${ratios[ratio].width}×${ratios[ratio].height}px`}><SelectInput value={ratio} onChange={e=>setRatio(e.target.value as keyof typeof ratios)}>{(Object.keys(ratios) as Array<keyof typeof ratios>).map(key=><option key={key} value={key}>{t(ratios[key].label)}</option>)}</SelectInput></FormField><FormField label={t('vid.seconds')} hint={t('vid.max',{count:maxDuration})}><TextInput type="number" min={3} max={maxDuration} value={seconds} onChange={e=>setSeconds(Number(e.target.value))}/></FormField>{usingComfy&&<FormField label="FPS" hint={t('vid.fpsHint')}><TextInput type="number" min={1} max={60} value={fps} onChange={e=>setFps(Number(e.target.value))}/></FormField>}</div>
      {usingComfy&&<FormField label={t('vid.seed')} hint={t('vid.seedHint')}><div className="flex gap-2"><TextInput value={seed} onChange={e=>setSeed(e.target.value)} placeholder={t('vid.randomValue')}/><Button variant="secondary" className="shrink-0" onClick={()=>setSeed('-1')}>{t('vid.random')}</Button></div></FormField>}
      {engine==='runway'&&<div className="rounded-xl border border-[#E5D2BC] bg-[#FFFBF4] p-3 text-xs leading-5">
       <strong>💳 유료 생성 예상 비용 · 서버 견적</strong>
       {paidQuote?<div className="mt-2 space-y-1">
        <p className="text-base font-bold text-[#3A342D]">약 {paidQuote.referenceKrw.toLocaleString('ko-KR')}원 <span className="text-[11px] font-normal">({paidQuote.clips}개 클립 · 세금·수수료 별도)</span></p>
        <p className="text-[#7C7469]">{(paidQuote.creditsMicro/1_000_000).toLocaleString('ko-KR',{maximumFractionDigits:2})} Runway Dev 크레딧 · {paidQuote.model} · {paidQuote.seconds}초</p>
        <p className="text-[#8A8176]">2026-11-13 기준 환율 {paidQuote.referenceFxKrwPerUsd.toLocaleString('ko-KR')}원 적용. 추정 견적이며 실시간 환율·실제 청구액은 별도입니다.</p>
        {paidQuote.promotional&&<p className="text-[#8A8176]">Kling 4.0 할인 요금 · 2026-11-30까지</p>}
        <label className="mt-2 flex items-center gap-2 font-semibold"><input type="checkbox" checked={paidApproved} disabled={generating} onChange={event=>setPaidApproved(event.target.checked)}/> 유료 API 견적과 재생성 시 별도 과금 가능성을 확인했습니다.</label>
       </div>:<p className="mt-2 text-[#8A8176]">{quoteError||'현재 설정으로 견적을 계산하고 있습니다.'}</p>}
      </div>}
      <div className="flex flex-wrap items-center justify-between gap-3">{usingComfy?<p className="text-[11px] leading-5 text-[#8A8176]">{t('vid.placeholderNote')}</p>:engine==='runway'?<p className="text-[11px] leading-5 text-[#8A8176]">유료 생성 · 요청한 {seconds}초를 {Number.isFinite(seconds)&&seconds>0?Math.ceil(seconds/runwayClipSeconds):"—"}개의 유료 클립으로 나누어 자동 합성합니다. 단일 클립 길이 초과 시 FFmpeg가 필요하며, 모델과 오디오 설정에 따라 청구량이 달라집니다.</p>:<p className="text-[11px] leading-5 text-[#8A8176]">{ready?t('vid.silentNote'):t('vid.ffmpegRequired')}</p>}<Action disabled={generating||Boolean(videoSubmission&&!terminal(activeVideo.value?.status))||!ready||!prompt.trim()||(engine==='runway'&&(!paidQuote||!paidApproved||paidQuote.seconds!==seconds||paidQuote.model!==savedRunwayModel))} onClick={()=>void generate()}>{generating?<><LoaderCircle size={15} className="animate-spin"/> {t('vid.requesting')}</>:<><Play size={15}/> {t('vid.generate')}</>}</Action></div>
     </div>
    </Panel>}
    {tab==='settings'&&<Panel className="min-w-0 p-5">
     <div className="mt-4 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3">
     <div className="flex items-center justify-between gap-2"><strong className="text-[12px] text-[#3A342D]">{t('vid.youtube')}</strong><StateTag online={gReady}>{gReady?t('vid.appReady'):t('vid.appNeeded')}</StateTag></div>
     <p className="mt-1 text-[10px] leading-4 text-[#8A8176]">{t('vid.redirectNote')}</p>{gCallback&&<div className="mt-1 flex items-center gap-2 rounded border border-[#E7E0D5] bg-[#FFFDF9] p-2"><code className="min-w-0 flex-1 break-all text-[10px] text-[#3A342D]">{gCallback}</code><Button variant="ghost" className="!px-2 !py-0.5 !text-[10px]" onClick={async()=>{try{await navigator.clipboard.writeText(gCallback);setError(t('vid.copied'))}catch(e){setError((e as Error).message)}}}>{t('vid.copy')}</Button></div>}
     <div className="mt-2 grid gap-2 sm:grid-cols-2"><TextInput value={gid} onChange={e=>setGid(e.target.value)} placeholder="Client ID"/><TextInput type="password" value={gsecret} onChange={e=>setGsecret(e.target.value)} placeholder="Client Secret"/></div>
     <div className="mt-2 flex flex-wrap gap-2"><Button variant="secondary" disabled={gBusy||!gid.trim()||!gsecret.trim()} onClick={()=>void saveGoogle()}>{gBusy?t('agents.saving'):t('vid.saveApp')}</Button><Button variant="secondary" onClick={()=>void connectGoogle()}>{t('vid.connectAccount')}</Button></div>
    </div>
    </Panel>}
   </div>
   {tab==='create'&&<div className="grid min-w-0 content-start gap-4">
    <Panel className="min-w-0 p-5"><Subheading title={t('vid.results')} extra={videoJob?<StateTag online={videoJob.status==='completed'}>{statusLabel(videoJob.status)}</StateTag>:<span className="text-[11px] text-[#8A8176]">{t('vid.waiting')}</span>}/>{!videoJob?<div className="flex w-full flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-[#D8CFC2] bg-[#FAF7F1] text-[12px] text-[#8A8176]" style={{aspectRatio:ratios[ratio].width+" / "+ratios[ratio].height,maxHeight:420}}><Clapperboard size={24}/>{t('vid.resultsEmpty')} <small className="text-[10px]">{t(ratios[ratio].label)} · {ratios[ratio].width}×{ratios[ratio].height}</small></div>:<div className="grid gap-3">{!terminal(videoJob.status)&&<><div className="flex justify-between text-[11px] text-[#7C7469]"><span>{videoJob.status==='queued'?t('vid.waitingJob'):videoJob.status==='downloading'?t('vid.savingResult'):t('vid.generating')}</span><span>{(videoJob.progress??0)}%</span></div><div className="h-2 overflow-hidden rounded-full bg-[#F3EEE5]"><div className="h-full bg-[#C96442]" style={{width:Math.max(4,(videoJob.progress??0))+'%'}}/></div></>}{videoJob.outputUrl&&<div className="flex justify-center rounded-xl bg-black p-2"><video className="max-h-[520px] w-auto max-w-full rounded-lg" src={videoJob.outputUrl} controls playsInline/></div>}{videoJob.status==='completed'&&videoJob.outputUrl&&<div className="grid gap-2"><div className="flex min-w-0 items-center gap-2 rounded bg-[#FAF7F1] p-2"><span className="min-w-0 flex-1 truncate text-[11px] text-[#5C544A]" title={videoJob.title}>{videoJob.title||"output.mp4"}</span><Button variant="ghost" className="!px-2 !py-1 !text-[10px]" onClick={()=>void reveal(videoJob.id)}>{t('vid.openFolder')}</Button></div><Action onClick={useResult} className="w-full"><Upload size={14}/> {t('vid.fillForm')}</Action></div>}{videoJob.costQuote&&<div className="rounded-lg border border-[#E5D2BC] bg-[#FFFBF4] p-3 text-[11px] leading-5">
        <strong>유료 작업 견적 · 약 {videoJob.costQuote.referenceKrw.toLocaleString('ko-KR')}원</strong>
        <p>Runway Dev {(videoJob.costQuote.creditsMicro/1_000_000).toLocaleString('ko-KR',{maximumFractionDigits:2})} 크레딧 · {videoJob.costQuote.model}</p>
        {typeof videoJob.attemptedExposureCreditsMicro==='number'&&videoJob.attemptedExposureCreditsMicro>0&&<p>제출 시도 {videoJob.attemptedSubmissions||0}건 · 잠재 과금 노출 {(videoJob.attemptedExposureCreditsMicro/1_000_000).toLocaleString('ko-KR',{maximumFractionDigits:2})} 크레딧</p>}
        <p className="text-[#8A8176]">제공자 실청구·환불 여부는 별도이며 이 수치는 추정치입니다. 실패·타임아웃 후 자동 재제출하지 않습니다.</p>
       </div>}{videoJob.result?.shotResults?.length&&<details className="rounded-xl border border-[#E7E0D5] p-3 text-[11px]">
        <summary className="cursor-pointer font-bold">🎞️ 장면별 검수 기록 · {videoJob.result.shotResults.length}장면</summary>
        <div className="mt-2 grid gap-2">{videoJob.result.shotResults.map(shot=><div key={shot.index} className="rounded-lg bg-[#FAF7F1] p-2">
         <strong>장면 {shot.index} · {shot.duration}초</strong>
         <p>{shot.reused?'기존 검증 장면 재사용':'새 장면 생성'} · 기술 검수 재생성 {shot.technicalRetries}회 · {fmtBytes(shot.bytes)}</p>
         {shot.qualityReceipt?.verified&&<p className="text-[#7C7469]">{shot.qualityReceipt.width}×{shot.qualityReceipt.height} · 재생 정보 검증</p>}
        </div>)}</div>
       </details>}{videoJob.result?.finalQuality&&<div className="rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-[11px] leading-5">
        <strong>{videoJob.result.finalQuality.verified?'✅ 파일 기술 검수 통과':'검수 미실행'} · 클립 {videoJob.result.clipQuality?.length||0}개</strong>
        {videoJob.result.finalQuality.verified&&<p className="text-[#6F675D]">{videoJob.result.finalQuality.durationSeconds?.toFixed(1)}초 · {videoJob.result.finalQuality.width}×{videoJob.result.finalQuality.height} · 오디오 {videoJob.result.finalQuality.audioStream?'포함':'없음'}</p>}
        {videoJob.result.finalQuality.reason&&<p className="text-[#8A8176]">사유: {videoJob.result.finalQuality.reason}</p>}
        <p className="text-[#8A8176]">컨테이너·시간·비율·오디오 스트림 검수이며 영상 내용·미학의 자동 판정은 아닙니다.</p>
       </div>}
       {(videoJob.status==='failed'||videoJob.status==='needs_review'||videoJob.status==='interrupted')&&<Notice message={videoJob.error||t('vid.generateFailed')}/>}</div>}</Panel>
    <Panel className="min-w-0 p-5"><Subheading title={t('vid.upload')} extra={<span className="text-[11px] text-[#8A8176]">{selectedOutput?t('vid.fileReady'):t('vid.pickFile')}</span>}/><div className="grid gap-3"><FormField label={t('vid.videoFile')} hint={t('vid.generatedNote')}><div className="flex flex-col gap-2">{outputs.value.length?<SelectInput value={selectedOutput?.id||''} onChange={e=>{
 const picked=outputs.value.find(item=>item.id===e.target.value);
 if(!picked)return;
 setUploadFile(picked.id);
 setUploadTitle(picked.title||'');
 setUploadDescription(picked.brief);
 }}>{outputs.value.map(item=><option key={item.id} value={item.id}>{item.title||'output.mp4'} · {fmtBytes(item.result?.bytes)}</option>)}</SelectInput>:<p className="text-[11px] text-[#8A8176]">{t('vid.noUploads')}</p>}</div></FormField><FormField label={t('vid.formTitle')}><TextInput value={uploadTitle} onChange={e=>setUploadTitle(e.target.value)} placeholder={t('vid.videoTitle')} maxLength={100}/></FormField><FormField label={t('vid.formDesc')}><TextArea rows={3} value={uploadDescription} onChange={e=>setUploadDescription(e.target.value)} placeholder={t('vid.formDesc')}/></FormField><div className="grid grid-cols-2 gap-3"><FormField label={t('vid.public')}><SelectInput value={uploadPrivacy} onChange={e=>setUploadPrivacy(e.target.value)}><option value="private">{t('vid.private')}</option><option value="unlisted">{t('vid.unlisted')}</option><option value="public">{t('vid.public')}</option></SelectInput></FormField><FormField label={t('vid.tags')}><TextInput value={uploadTags} onChange={e=>setUploadTags(e.target.value)} placeholder={t('vid.comma')}/></FormField></div><Action disabled={uploading||Boolean(uploadSubmission&&!terminal(uploadJob?.status))||!youtube.value.uploadAllowed||!selectedOutput||!uploadTitle.trim()} onClick={()=>void startUpload()} className="w-full">{uploading?<><LoaderCircle size={15} className="animate-spin"/> {t('vid.requesting')}</>:<><Upload size={15}/> {t('vid.upload')}</>}</Action>{uploadJob&&<div className="rounded-lg border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-[11px]"><div className="flex justify-between"><span>{uploadJob.status}</span><span>{uploadJob.progress??0}%</span></div>{uploadJob.result?.url&&<a className="mt-1 inline-flex items-center gap-1 font-semibold text-[#C96442]" href={uploadJob.result.url} target="_blank" rel="noreferrer">{t('vid.openYoutube')} <ExternalLink size={12}/></a>}{uploadJob.error&&<p className="mt-1 text-rose-600">{uploadJob.error}</p>}</div>}</div></Panel>
   </div>}
  </div>
 </div>
}
