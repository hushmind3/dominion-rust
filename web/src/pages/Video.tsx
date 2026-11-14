import {useEffect,useState} from 'react'
import {Check,Clapperboard,ExternalLink,LoaderCircle,Play,Power,RefreshCw,Upload} from 'lucide-react'
import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {Action,FormField,Headline,Notice,Panel,StateTag,Subheading,useRemote} from '../components/UI'
import {openOAuth,post} from '../api'
import {jobFinished,useJob,type JobState} from '../hooks/useJob'
import {t,useLocale} from '../i18n'
import {FilmRetake} from './video/FilmRetake'

type Workflow={id:string;name:string;bytes:number}
type VideoJob=JobState & {progress?:number;kind:'video';title?:string;brief:string;outputUrl?:string;
 shotCount?:number;shotProgress?:number;paidSubmissionAttempts?:number;
 film?:{clips?:unknown[]};
 result?:{bytes:number;attemptedGenerations?:number;
 technicalQuality?:{verified:boolean;width:number;height:number;durationSeconds:number};
 shotResults?:{index:number;reused:boolean;technicalRetries:number}[]}}
type VideoEngine='native'|'comfyui'|'ltx'
type VideoStatus={config:{engine:VideoEngine;comfyUrl:string;hasLtxKey?:boolean;videoModel?:string;videoQuality?:string};ltx?:{available:boolean};native?:{available:boolean};comfy:{reachable:boolean;available?:boolean};workflows:Workflow[];jobs:VideoJob[]}
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
 const [workflow,setWorkflow]=useState(''),[title,setTitle]=useState(''),[prompt,setPrompt]=useState(''),[negative,setNegative]=useState(''),[ratio,setRatio]=useState<keyof typeof ratios>('9:16'),[seconds,setSeconds]=useState(8),[fps,setFps]=useState(16),[seed,setSeed]=useState('-1')
 const [engineChoice,setEngine]=useState<VideoEngine>(),[comfyUrl,setComfyUrl]=useState(''),[savingConfig,setSavingConfig]=useState(false)
 const [ltxKey,setLtxKey]=useState(''),[ltxModel,setLtxModel]=useState('ltx-2-5-pro'),[ltxQuality,setLtxQuality]=useState('1080p'),[ltxAudio,setLtxAudio]=useState(true)
 const [cinematic,setCinematic]=useState(true),[shotNotes,setShotNotes]=useState(''),[paidMultiShot,setPaidMultiShot]=useState(false),[ltxFps,setLtxFps]=useState(24)
 const [comfyCinematic,setComfyCinematic]=useState(false),[comfyRetryConsent,setComfyRetryConsent]=useState(false)
 const [tab,setTab]=useState<'create'|'settings'>('create'),[refreshing,setRefreshing]=useState(false),[gid,setGid]=useState(''),[gsecret,setGsecret]=useState(''),[gBusy,setGBusy]=useState(false)
 const [videoSubmission,setVideoSubmission]=useState<VideoJob|null>(null),[generating,setGenerating]=useState(false),[error,setError]=useState('')
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
 const engine=engineChoice??video.value.config.engine??'ltx'
 const usingComfy=engine==='comfyui',usingLtx=engine==='ltx'
 const ready=usingLtx?Boolean(video.value.ltx?.available):usingComfy?Boolean(video.value.comfy.available&&workflow):Boolean(video.value.native?.available)
 const ltxClipMax=ltxModel==='ltx-2-5-pro'||ltxFps>=48||['1440p','4k'].includes(ltxQuality)?10:20
 const maxDuration=usingLtx?ltxClipMax*6:usingComfy?(comfyCinematic?120:9):180
 const comfyShots=usingComfy&&comfyCinematic?Math.min(15,Math.ceil(seconds/8)):1
 const paidCalls=usingLtx?Math.ceil(seconds/ltxClipMax):0
 const needsMultiShot=usingLtx&&paidCalls>1
 useEffect(()=>{setComfyUrl(video.value.config.comfyUrl)},[video.value.config.comfyUrl])
 useEffect(()=>{if(video.value.config.videoModel)setLtxModel(video.value.config.videoModel);if(video.value.config.videoQuality)setLtxQuality(video.value.config.videoQuality)},[video.value.config.videoModel,video.value.config.videoQuality])
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
  setEngine(next);if(next==='ltx')setSeconds(8);setSavingConfig(true);setError('');
  try{await post('/api/video/config',{engine:next,comfyUrl:comfyUrl.trim()||video.value.config.comfyUrl});await video.reload()}
  catch(e){setError((e as Error).message)}finally{setSavingConfig(false)}
 }
 const saveLtx=async()=>{
  if(savingConfig||!ltxKey.trim())return;
  setSavingConfig(true);setError('');
  try{await post('/api/video/config',{engine:'ltx',ltxApiKey:ltxKey.trim(),videoModel:ltxModel,videoQuality:ltxQuality});setLtxKey('');await video.reload()}
  catch(e){setError((e as Error).message)}finally{setSavingConfig(false)}
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
  if(!Number.isInteger(seconds)||seconds<(usingLtx?6:3)||seconds>maxDuration||(usingLtx&&(seconds%2!==0||(needsMultiShot&&!paidMultiShot)))||
   (usingComfy&&(!seed.trim()||!Number.isSafeInteger(requestedSeed)||requestedSeed < -1||!Number.isInteger(fps)||fps<1||fps>60))){
   setError(t('vid.checkValues'));return;
  }
  setGenerating(true);setError('');
  try{
   setVideoSubmission(await post<VideoJob>('/api/video/jobs',{
    engine,title,brief:prompt,aspectRatio:ratio,duration:seconds,
    ...(usingComfy?{negativePrompt:negative,fps,seed:requestedSeed,
      cinematic:comfyCinematic,badShotRetries:comfyRetryConsent?1:0,
      confirmRetryCosts:comfyRetryConsent}:{}),
     ...(usingLtx?{videoModel:ltxModel,videoQuality:ltxQuality,generateAudio:ltxAudio,fps:ltxFps,
      cinematic,shotNotes,confirmPaidMultiShot:needsMultiShot&&paidMultiShot}:{}),
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
     <SelectInput aria-label={t('vid.engine')} value={engine} onChange={e=>void changeEngine(e.target.value as VideoEngine)} disabled={savingConfig} className="!w-auto !text-[12px]"><option value="native">{t('vid.engine.native')}</option><option value="comfyui">{t('vid.engine.comfy')}</option><option value="ltx">LTX 2.5 · 고품질 AI 영상</option></SelectInput>
    </div>
   </div>
  </Panel>
  <div className="mb-4 grid grid-cols-1 items-start gap-3 md:grid-cols-2">
   {engine==='ltx'&&<Panel className="border-[#E7D8C5] bg-[#FFFCF7] p-4">
     <div className="flex items-center justify-between gap-3"><strong className="text-[13px]">🎬 LTX 2.5 · 원클릭 시네마틱 영상</strong><StateTag online={Boolean(video.value.ltx?.available)}>{video.value.ltx?.available?'연결됨':'API 키 필요'}</StateTag></div>
     <p className="mt-2 text-[12px] leading-5">로컬 GPU·ComfyUI 워크플로 없이 고품질 영상과 오디오를 만듭니다. 외부 API 사용료가 발생합니다.</p>
     <div className="mt-2 flex flex-wrap items-center gap-2"><TextInput type="password" aria-label="LTX API Key" value={ltxKey} onChange={e=>setLtxKey(e.target.value)} placeholder={video.value.config.hasLtxKey?'API 키 등록됨 · 교체 시 입력':'LTX API Key'}/><Button variant="secondary" disabled={savingConfig||!ltxKey.trim()} onClick={()=>void saveLtx()}>암호화 저장</Button></div>
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
     <Subheading title={t('vid.generate')} extra={<span className="text-[11px] text-[#8A8176]">{usingLtx?'LTX V2 · 장면 연속성 + 오디오':usingComfy?t('vid.workflowCount',{count:video.value.workflows.length}):t('vid.ffmpegCard')}</span>}/>
     <div className="grid gap-4">
      <FormField label={t('vid.videoTitle')} hint={t('vid.videoTitleHint')}><TextInput value={title} onChange={e=>setTitle(e.target.value)} placeholder={t('vid.videoTitlePlaceholder')}/></FormField>
      <FormField label={t('vid.prompt')}><TextArea rows={4} value={prompt} onChange={e=>setPrompt(e.target.value)} placeholder={t('vid.promptPlaceholder')}/></FormField>
      {usingLtx&&<div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
        <FormField label="모델"><SelectInput value={ltxModel} onChange={e=>{setLtxModel(e.target.value);setSeconds(8)}}><option value="ltx-2-5-pro">Pro · 고품질</option><option value="ltx-2-5-fast">Fast · 비용 절감</option></SelectInput></FormField>
        <FormField label="화질"><SelectInput value={ltxQuality} onChange={e=>setLtxQuality(e.target.value)}><option value="720p">720p</option><option value="1080p">1080p</option><option value="1440p">1440p</option><option value="4k">4K</option></SelectInput></FormField>
        <FormField label="FPS"><SelectInput value={ltxFps} onChange={e=>setLtxFps(Number(e.target.value))}><option value={24}>24 · 시네마</option><option value={25}>25 · TV</option><option value={48}>48 · 부드러운 영상</option><option value={50}>50 · 고프레임</option></SelectInput></FormField>
        <FormField label="사운드"><SelectInput value={ltxAudio?'on':'off'} onChange={e=>setLtxAudio(e.target.value==='on')}><option value="on">오디오 생성</option><option value="off">무음</option></SelectInput></FormField>
       </div>}
       {usingLtx&&<div className="grid gap-3 rounded-xl border border-[#E7D8C5] bg-[#FFFCF7] p-3 text-[12px]">
       <label className="flex cursor-pointer items-center gap-2"><input type="checkbox" checked={cinematic} onChange={e=>setCinematic(e.target.checked)}/><strong>🎬 시네마틱 디렉터 · 장면 연결과 카메라 연출</strong></label>
       <FormField label="장면별 지시 (선택)" hint="줄마다 1개 장면 · 최대 6개"><TextArea rows={2} value={shotNotes} onChange={e=>setShotNotes(e.target.value)} placeholder="첫 장면: 인물과 공간을 소개&#10;다음 장면: 움직임을 강조" /></FormField>
       {needsMultiShot&&<label className="flex cursor-pointer items-start gap-2 text-[#7C4A2E]"><input type="checkbox" checked={paidMultiShot} onChange={e=>setPaidMultiShot(e.target.checked)}/><span>긴 영상은 LTX 유료 생성 <strong>{paidCalls}회</strong>를 차례로 실행하고 FFmpeg로 무손실 연결합니다. 장면별 과금에 동의합니다.</span></label>}
       <p className="text-[11px] text-[#8A8176]">Pro 최대 60초 · Fast 저해상도 24/25FPS 최대 120초 · 1440p/4K 또는 고FPS 최대 60초 · 장면마다 별도 과금</p>
      </div>}
      {usingComfy&&<div className="space-y-2 rounded-xl border border-[#E7D8C5] bg-[#FFFCF7] p-3 text-[12px]">
       <label className="flex cursor-pointer items-center gap-2">
        <input type="checkbox" checked={comfyCinematic} onChange={e=>{setComfyCinematic(e.target.checked);setComfyRetryConsent(false)}}/>
        <strong>🎞️ ComfyUI 멀티샷 · 장면별 검수와 1080p 편집</strong>
       </label>
       {comfyCinematic&&<>
        <p className="text-[11px] leading-5 text-[#7C7469]">
         목표 {seconds}초 · 최대 {comfyShots}개 장면, 장면당 최대 8초.
         워크플로에서 {"{{prompt}}"}와 {"{{duration}}"} 변수를 사용해야 합니다.
         통과한 장면은 보관해 다음 작업에서 재사용합니다.
        </p>
        <label className="flex cursor-pointer items-start gap-2">
         <input type="checkbox" checked={comfyRetryConsent} onChange={e=>setComfyRetryConsent(e.target.checked)}/>
         <span>기술 검수 불합격 장면마다 최대 1회 추가 생성 동의
          <span className="block text-[11px] text-[#8A8176]">
           외부 유료 ComfyUI라면 최대 {comfyShots}회 추가 과금 가능 · 기본 재시도 0회
          </span>
         </span>
        </label>
       </>}
      </div>}
      {usingComfy&&<FormField label={t('vid.negative')} hint={t('vid.negativeHint')}><TextInput value={negative} onChange={e=>setNegative(e.target.value)} placeholder="low quality, blurry, watermark"/></FormField>}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3"><FormField label={t('vid.ratio')} hint={usingLtx?ltxQuality:`${ratios[ratio].width}×${ratios[ratio].height}px`}><SelectInput value={ratio} onChange={e=>setRatio(e.target.value as keyof typeof ratios)}>{(Object.keys(ratios) as Array<keyof typeof ratios>).map(key=><option key={key} value={key}>{t(ratios[key].label)}</option>)}</SelectInput></FormField><FormField label={t('vid.seconds')} hint={t('vid.max',{count:usingLtx?ltxClipMax*6:maxDuration})}><TextInput type="number" min={usingLtx?6:3} max={maxDuration} step={usingLtx?2:1} value={seconds} onChange={e=>setSeconds(Number(e.target.value))}/></FormField>{usingComfy&&<FormField label="FPS" hint={t('vid.fpsHint')}><TextInput type="number" min={1} max={60} value={fps} onChange={e=>setFps(Number(e.target.value))}/></FormField>}</div>
      {usingComfy&&<FormField label={t('vid.seed')} hint={t('vid.seedHint')}><div className="flex gap-2"><TextInput value={seed} onChange={e=>setSeed(e.target.value)} placeholder={t('vid.randomValue')}/><Button variant="secondary" className="shrink-0" onClick={()=>setSeed('-1')}>{t('vid.random')}</Button></div></FormField>}
      <div className="flex flex-wrap items-center justify-between gap-3">{usingLtx?<p className="text-[11px] leading-5 text-[#8A8176]">{ready?'장편은 2~6회 유료 호출 · 프롬프트 동결 · 24/25/48/50FPS':'설정에서 LTX API 키를 등록하세요.'}</p>:usingComfy?<p className="text-[11px] leading-5 text-[#8A8176]">{comfyCinematic?'장면별 기술 검수 · 성공 장면 재사용 · FFmpeg 편집':t('vid.placeholderNote')}</p>:<p className="text-[11px] leading-5 text-[#8A8176]">{ready?t('vid.silentNote'):t('vid.ffmpegRequired')}</p>}<Action disabled={generating||Boolean(videoSubmission&&!terminal(activeVideo.value?.status))||!ready||!prompt.trim()} onClick={()=>void generate()}>{generating?<><LoaderCircle size={15} className="animate-spin"/> {t('vid.requesting')}</>:<><Play size={15}/> {t('vid.generate')}</>}</Action></div>
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
    <Panel className="min-w-0 p-5"><Subheading title={t('vid.results')} extra={videoJob?<StateTag online={videoJob.status==='completed'}>{statusLabel(videoJob.status)}</StateTag>:<span className="text-[11px] text-[#8A8176]">{t('vid.waiting')}</span>}/>{!videoJob?<div className="flex w-full flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-[#D8CFC2] bg-[#FAF7F1] text-[12px] text-[#8A8176]" style={{aspectRatio:ratios[ratio].width+" / "+ratios[ratio].height,maxHeight:420}}><Clapperboard size={24}/>{t('vid.resultsEmpty')} <small className="text-[10px]">{t(ratios[ratio].label)} · {ratios[ratio].width}×{ratios[ratio].height}</small></div>:<div className="grid gap-3">{!terminal(videoJob.status)&&<><div className="flex justify-between text-[11px] text-[#7C7469]"><span>{videoJob.status==='queued'?t('vid.waitingJob'):videoJob.status==='downloading'?t('vid.savingResult'):t('vid.generating')}</span><span>{(videoJob.progress??0)}%</span></div><div className="h-2 overflow-hidden rounded-full bg-[#F3EEE5]"><div className="h-full bg-[#C96442]" style={{width:Math.max(4,(videoJob.progress??0))+'%'}}/></div></>}{Boolean(videoJob.shotCount)&&<p className="text-[11px] text-[#7C7469]">
       장면 제작 {videoJob.shotProgress||0}/{videoJob.shotCount} · 완료한 장면은 유지됩니다.
      </p>}
      {videoJob.result?.technicalQuality?.verified&&<p className="text-[11px] text-[#6D6257]">
       <Check size={13} className="mr-1 inline"/> 기술 검수 통과 ·
       {videoJob.result.technicalQuality.width}×{videoJob.result.technicalQuality.height} ·
       {videoJob.result.technicalQuality.durationSeconds.toFixed(1)}초
      </p>}
      {typeof videoJob.paidSubmissionAttempts==='number'&&videoJob.paidSubmissionAttempts>0&&
       <p className="text-[11px] text-[#7C7469]">
        LTX 유료 전송 시도 {videoJob.paidSubmissionAttempts}회 · 실제 청구 여부와 원화 금액은 제공자 청구서에서 확인
       </p>}
      {videoJob.result?.attemptedGenerations!==undefined&&<p className="text-[11px] text-[#7C7469]">
       영상 생성 {videoJob.result.attemptedGenerations}회 · 실제 제작비는 연결된 환경에 따라 달라집니다.
      </p>}
      {videoJob.outputUrl&&<div className="flex justify-center rounded-xl bg-black p-2"><video className="max-h-[520px] w-auto max-w-full rounded-lg" src={videoJob.outputUrl} controls playsInline/></div>}{videoJob.status==='completed'&&videoJob.outputUrl&&<div className="grid gap-2"><div className="flex min-w-0 items-center gap-2 rounded bg-[#FAF7F1] p-2"><span className="min-w-0 flex-1 truncate text-[11px] text-[#5C544A]" title={videoJob.title}>{videoJob.title||"output.mp4"}</span><Button variant="ghost" className="!px-2 !py-1 !text-[10px]" onClick={()=>void reveal(videoJob.id)}>{t('vid.openFolder')}</Button></div><Action onClick={useResult} className="w-full"><Upload size={14}/> {t('vid.fillForm')}</Action></div>}{(videoJob.status==='failed'||videoJob.status==='needs_review'||videoJob.status==='interrupted')&&<Notice message={videoJob.error||t('vid.generateFailed')}/>}</div>}</Panel>
    {videoJob?.film?.clips&&videoJob.film.clips.length>1&&
     <FilmRetake jobId={videoJob.id} onCreated={job=>{
      setVideoSubmission(job as VideoJob);
      void video.reload();
     }}/>}
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
