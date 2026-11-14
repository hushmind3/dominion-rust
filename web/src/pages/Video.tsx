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
 result?:{bytes:number;shots?:number;newPaidCallsThisRun?:number;technicalQuality?:{status:string;reason?:string};
 clipReceipts?:{index:number;quality?:{status:string}}[]};
 videoQuality?:string;videoModel?:string;shotMode?:string;generateAudio?:boolean;
 technicalGate?:boolean;film?:{paidCallCount?:number;clips?:unknown[]};storyboard?:string[]}
type VideoEngine='native'|'comfyui'|'runway'|'ltx'
type PaidQuote={quoteDigest:string;totalUsdCents:number;referenceKrw:number;
 billableSeconds:number;paidCalls:number;model:string;quality:string;
 pricingAsOf:string;fxAsOf:string;pricingUrl:string}
type VideoStatus={config:{engine:VideoEngine;comfyUrl:string;hasRunwayKey?:boolean;hasLtxKey?:boolean};native?:{available:boolean};comfy:{reachable:boolean;available?:boolean};runway?:{available:boolean};ltx?:{available:boolean};workflows:Workflow[];jobs:VideoJob[]}
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
 const [engineChoice,setEngine]=useState<VideoEngine>(),[comfyUrl,setComfyUrl]=useState(''),[runwayKey,setRunwayKey]=useState(''),[ltxKey,setLtxKey]=useState(''),[savingConfig,setSavingConfig]=useState(false)
 const [videoQuality,setVideoQuality]=useState<'720p'|'1080p'|'1440p'|'4k'>('1080p'),[ltxModel,setLtxModel]=useState<'ltx-2-5-fast'|'ltx-2-5-pro'>('ltx-2-5-pro'),[shotMode,setShotMode]=useState<'auto'|'custom'>('auto'),[customShots,setCustomShots]=useState(''),[generateAudio,setGenerateAudio]=useState(true)
 const [ltxFilm,setLtxFilm]=useState(false),[confirmPaidMultiShot,setConfirmPaidMultiShot]=useState(false),[technicalGate,setTechnicalGate]=useState(false)
 const [paidQuote,setPaidQuote]=useState<PaidQuote|null>(null),[approvePaid,setApprovePaid]=useState(false),[quoting,setQuoting]=useState(false)
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
 const engine=engineChoice??video.value.config.engine
 const usingComfy=engine==='comfyui'
 const usingRunway=engine==='runway'
 const usingLtx=engine==='ltx'
 const ltxClipMax=ltxModel==='ltx-2-5-fast'&&['720p','1080p'].includes(videoQuality)?20:10
 const ltxDurations=ltxFilm?
  Array.from({length:(ltxClipMax*6-ltxClipMax)/2},(_,i)=>ltxClipMax+2+i*2):
  ltxModel==='ltx-2-5-pro'||videoQuality==='1440p'||videoQuality==='4k'?[6,8,10]:[6,8,10,12,14,16,18,20]
 const estimatedFilmShots=ltxFilm?Math.ceil(seconds/ltxClipMax):1
 const ready=usingComfy?Boolean(video.value.comfy.available&&workflow):usingRunway?Boolean(video.value.runway?.available):usingLtx?Boolean(video.value.ltx?.available):Boolean(video.value.native?.available)
 const maxDuration=usingComfy?9:usingRunway?15:usingLtx?ltxDurations[ltxDurations.length-1]:180
 // Approval is bound to an exact server-side snapshot, not just clip count.
 useEffect(()=>{setPaidQuote(null);setApprovePaid(false)},
  [usingLtx,videoQuality,ltxModel,ltxFilm,seconds,ratio,generateAudio])
 const previewPaidQuote=async()=>{
  setQuoting(true);setError('');setApprovePaid(false);setPaidQuote(null)
  try{
   setPaidQuote(await post<PaidQuote>('/api/video/quote',{
    engine:'ltx',videoModel:ltxModel,videoQuality,duration:seconds,
    aspectRatio:ratio,fps:24,ltxFilm,generateAudio
   }))
  }catch(e){setError((e as Error).message)}finally{setQuoting(false)}
 }
 useEffect(()=>{setComfyUrl(video.value.config.comfyUrl)},[video.value.config.comfyUrl])
 useEffect(()=>{
  if(usingRunway&&['1440p','4k'].includes(videoQuality))setVideoQuality('1080p')
  if(usingRunway&&![5,10,15].includes(seconds))setSeconds(10)
  else if(usingLtx&&!ltxDurations.includes(seconds))setSeconds(ltxFilm?22:8)
  else if(usingComfy&&seconds>9)setSeconds(9)
 },[usingRunway,usingLtx,usingComfy,seconds,ltxModel,videoQuality,ltxFilm])
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
  setEngine(next);if(next==='runway')setSeconds(10);if(next==='ltx')setSeconds(8);setSavingConfig(true);setError('');
  try{await post('/api/video/config',{engine:next,comfyUrl:comfyUrl.trim()||video.value.config.comfyUrl});await video.reload()}
  catch(e){setError((e as Error).message)}finally{setSavingConfig(false)}
 }
 const saveComfy=async()=>{
  if(savingConfig)return;
  setSavingConfig(true);setError('');
  try{await post('/api/video/config',{engine:'comfyui',comfyUrl:comfyUrl.trim()});await video.reload()}
  catch(e){setError((e as Error).message)}finally{setSavingConfig(false)}
 }
 const saveRunway=async()=>{
  if(savingConfig||!runwayKey.trim())return
  setSavingConfig(true);setError('')
  try{await post('/api/video/config',{runwayApiKey:runwayKey.trim()});setRunwayKey('');await video.reload()}
  catch(e){setError((e as Error).message)}finally{setSavingConfig(false)}
 }
 const saveLtx=async()=>{
  if(savingConfig||!ltxKey.trim())return
  setSavingConfig(true);setError('')
  try{await post('/api/video/config',{ltxApiKey:ltxKey.trim()});setLtxKey('');await video.reload()}
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
  if(usingLtx&&(!paidQuote||!approvePaid)){
   setError('모델·해상도·초수의 서버 견적을 확인하고 금액을 승인해 주세요.');return;
  }
  const requestedSeed=Number(seed);
  const scenes=customShots.split(/\r?\n/).map(scene=>scene.trim()).filter(Boolean);
  if(usingRunway&&shotMode==='custom'&&(scenes.length<3||scenes.length>5||
    scenes.some(scene=>Array.from(scene).length<3||Array.from(scene).length>512))){
   setError('커스텀 샷을 줄마다 1개씩 3~5개 입력하세요. 각 샷은 3~512자입니다.');return;
  }
  if(!Number.isInteger(seconds)||seconds<3||seconds>maxDuration||(usingRunway&&![5,10,15].includes(seconds))||(usingLtx&&!ltxDurations.includes(seconds))||(usingRunway&&['1440p','4k'].includes(videoQuality))||
   (usingComfy&&(!seed.trim()||!Number.isSafeInteger(requestedSeed)||requestedSeed < -1||!Number.isInteger(fps)||fps<1||fps>60))){
   setError(t('vid.checkValues'));return;
  }
  setGenerating(true);setError('');
  try{
   setVideoSubmission(await post<VideoJob>('/api/video/jobs',{
    engine,title,brief:prompt,aspectRatio:ratio,duration:seconds,
    ...(usingRunway?{videoQuality,shotMode,scenes:shotMode==='custom'?scenes:[],generateAudio}:{}),
    ...(usingLtx?{videoModel:ltxModel,videoQuality,generateAudio,fps:24,
     ltxFilm,confirmPaidMultiShot,technicalGate,
     approvedPaidQuote:true,maxUsdCents:paidQuote?.totalUsdCents,
     quoteDigest:paidQuote?.quoteDigest}:{}),
    ...(usingComfy?{negativePrompt:negative,fps,seed:requestedSeed}:{}),
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
     <SelectInput aria-label={t('vid.engine')} value={engine} onChange={e=>void changeEngine(e.target.value as VideoEngine)} disabled={savingConfig} className="!w-auto !text-[12px]"><option value="native">{t('vid.engine.native')}</option><option value="comfyui">{t('vid.engine.comfy')}</option><option value="runway">Runway · AI 멀티샷 + 사운드</option><option value="ltx">LTX-2.5 · 최대 4K + 사운드</option></SelectInput>
    </div>
   </div>
  </Panel>
  <div className="mb-4 grid grid-cols-1 items-start gap-3 md:grid-cols-2">
   {engine==='runway'&&<Panel className={'p-4 '+(video.value.runway?.available?'border-emerald-200 bg-emerald-50/50':'border-amber-200 bg-amber-50/50')}>
    <div className="flex items-center justify-between gap-2"><strong className="text-[13px]">Runway · 원클릭 AI 영상</strong><StateTag online={Boolean(video.value.runway?.available)}>{video.value.runway?.available?'API 연결 설정 완료':'API 키 필요'}</StateTag></div>
    <p className="mt-2 text-xs leading-5 text-[#5C544A]">2026-06 멀티샷 레시피로 5개 컷과 오디오를 생성하고 결과 MP4를 자동 저장합니다. 생성할 때 Runway API 사용료가 발생합니다.</p>
    <div className="mt-3 flex gap-2"><TextInput aria-label="Runway API 키" type="password" value={runwayKey} onChange={e=>setRunwayKey(e.target.value)} placeholder={video.value.config.hasRunwayKey?'새 API 키로 교체':'Runway API 키'}/><Button variant="secondary" disabled={savingConfig||!runwayKey.trim()} onClick={()=>void saveRunway()}>키 저장</Button></div>
   </Panel>}
   {engine==='ltx'&&<Panel className={'p-4 '+(video.value.ltx?.available?'border-emerald-200 bg-emerald-50/50':'border-amber-200 bg-amber-50/50')}>
    <div className="flex items-center justify-between gap-2"><strong className="text-[13px]">LTX-2.5 · 턴키 4K 영상</strong><StateTag online={Boolean(video.value.ltx?.available)}>{video.value.ltx?.available?'API 키 등록됨':'API 키 필요'}</StateTag></div>
    <p className="mt-2 text-xs leading-5 text-[#5C544A]">2026년 V2 비동기 API로 최대 4K 영상과 AI 오디오를 생성합니다. GPU·로컬 모델이 없어도 동작하며, 별도 유료 API 크레딧이 소모됩니다.</p>
    <div className="mt-3 flex gap-2"><TextInput aria-label="LTX API 키" type="password" value={ltxKey} onChange={e=>setLtxKey(e.target.value)} placeholder={video.value.config.hasLtxKey?'새 API 키로 교체':'LTX API 키'}/><Button variant="secondary" disabled={savingConfig||!ltxKey.trim()} onClick={()=>void saveLtx()}>키 저장</Button></div>
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
     <Subheading title={t('vid.generate')} extra={<span className="text-[11px] text-[#8A8176]">{usingComfy?t('vid.workflowCount',{count:video.value.workflows.length}):usingRunway?'Runway 멀티샷 · 네이티브 오디오':usingLtx?'LTX-2.5 · 클라우드 고화질 영상':t('vid.ffmpegCard')}</span>}/>
     <div className="grid gap-4">
      <FormField label={t('vid.videoTitle')} hint={t('vid.videoTitleHint')}><TextInput value={title} onChange={e=>setTitle(e.target.value)} placeholder={t('vid.videoTitlePlaceholder')}/></FormField>
      <FormField label={t('vid.prompt')}><TextArea rows={4} value={prompt} onChange={e=>setPrompt(e.target.value)} placeholder={t('vid.promptPlaceholder')}/></FormField>
      {usingComfy&&<FormField label={t('vid.negative')} hint={t('vid.negativeHint')}><TextInput value={negative} onChange={e=>setNegative(e.target.value)} placeholder="low quality, blurry, watermark"/></FormField>}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3"><FormField label={t('vid.ratio')} hint={`${ratios[ratio].width}×${ratios[ratio].height}px`}><SelectInput value={ratio} onChange={e=>setRatio(e.target.value as keyof typeof ratios)}>{(Object.keys(ratios) as Array<keyof typeof ratios>).map(key=><option key={key} value={key}>{t(ratios[key].label)}</option>)}</SelectInput></FormField><FormField label={t('vid.seconds')} hint={t('vid.max',{count:maxDuration})}>{usingRunway?<SelectInput value={seconds} onChange={e=>setSeconds(Number(e.target.value))}><option value={5}>5초</option><option value={10}>10초</option><option value={15}>15초</option></SelectInput>:usingLtx?<SelectInput value={seconds} onChange={e=>setSeconds(Number(e.target.value))}>{ltxDurations.map(duration=><option key={duration} value={duration}>{duration}초</option>)}</SelectInput>:<TextInput type="number" min={3} max={maxDuration} value={seconds} onChange={e=>setSeconds(Number(e.target.value))}/>}</FormField>{usingComfy&&<FormField label="FPS" hint={t('vid.fpsHint')}><TextInput type="number" min={1} max={60} value={fps} onChange={e=>setFps(Number(e.target.value))}/></FormField>}</div>
      {usingLtx&&<div className="grid gap-3 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3">
       <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <FormField label="생성 모델"><SelectInput value={ltxModel} onChange={e=>setLtxModel(e.target.value as 'ltx-2-5-fast'|'ltx-2-5-pro')}><option value="ltx-2-5-pro">LTX-2.5 Pro · 품질 우선</option><option value="ltx-2-5-fast">LTX-2.5 Fast · 속도·비용 우선</option></SelectInput></FormField>
        <FormField label="해상도" hint="4K·1440p는 크레딧 사용료가 높습니다."><SelectInput value={videoQuality} onChange={e=>setVideoQuality(e.target.value as '720p'|'1080p'|'1440p'|'4k')}><option value="1080p">1080p</option><option value="720p">720p</option><option value="1440p">1440p</option><option value="4k">4K</option></SelectInput></FormField>
       </div>
       <label className="flex items-center gap-2 text-xs text-[#5C544A]"><input type="checkbox" checked={generateAudio} onChange={e=>setGenerateAudio(e.target.checked)}/> AI 음향·음성 함께 생성</label>
       <label className="flex items-center gap-2 text-xs text-[#5C544A]"><input type="checkbox" checked={ltxFilm}
        onChange={e=>{setLtxFilm(e.target.checked);setSeconds(e.target.checked?22:8);setConfirmPaidMultiShot(false)}}/> 장편 멀티샷 제작 (장면별 독립 생성)</label>
       {ltxFilm&&<div className="rounded-lg border border-[#E7E0D5] bg-white p-3 text-[11px] text-[#6B6258]">
        <p>현재 길이에 필요한 예상 유료 생성 <strong>{estimatedFilmShots}회</strong> · 최대 6개 장면. 각 장면의 실제 기술 검수를 기록하고 재시작 시 완료 영수증을 재사용합니다. 추가 모델 호출이나 시각적 연속성 보장은 포함하지 않습니다.</p>
        <label className="mt-2 flex items-center gap-2"><input type="checkbox" checked={confirmPaidMultiShot}
         onChange={e=>setConfirmPaidMultiShot(e.target.checked)}/> 위 장면 수만큼 유료 API가 각각 호출되는 데 동의합니다.</label>
       </div>}
       <label className="flex items-center gap-2 text-xs text-[#5C544A]"><input type="checkbox" checked={technicalGate}
        onChange={e=>setTechnicalGate(e.target.checked)}/> FFprobe 기술 검수 필수 (로컬 FFprobe 설치 필요)</label>
       <div className="rounded-lg border border-[#D8CFC2] bg-white p-3 text-xs text-[#5C544A]">
        <div className="flex flex-wrap items-center justify-between gap-2">
         <strong>유료 영상 예상 원가 · 서버 견적</strong>
         <Button variant="secondary" size="sm" disabled={quoting} onClick={()=>void previewPaidQuote()}>
          {quoting?'견적 계산 중':'현재 설정으로 견적 확인'}
         </Button>
        </div>
        {paidQuote?<div className="mt-2 grid gap-2">
         <p>예상 상한 <strong className="text-base">{paidQuote.referenceKrw.toLocaleString('ko-KR')}원</strong>
          {' '}· {paidQuote.paidCalls}회 생성 · 총 {paidQuote.billableSeconds}초</p>
         <p className="text-[11px] leading-5 text-[#7C7469]">2026년 {paidQuote.pricingAsOf} 기준 공개 가격 · 환율 기준일 {paidQuote.fxAsOf}. 실제 청구액·세금·환율 변동은 별도이며 계획 모델 호출료는 포함하지 않습니다.</p>
         <label className="flex items-start gap-2 text-[11px]">
          <input type="checkbox" checked={approvePaid} onChange={e=>setApprovePaid(e.target.checked)}/>
          <span>위 설정의 예상 유료 원가와 금액 상한을 확인했습니다. 서버가 가격·영상 설정이 달라졌다고 판단하면 생성이 차단됩니다.</span>
         </label>
        </div>:<p className="mt-2 text-[11px] text-[#7C7469]">유료 API 호출 전에 모델·초수·해상도별 금액을 확인해야 합니다. 금액은 요금표 기준 예상치입니다.</p>}
       </div>
      </div>}
      {usingRunway&&<div className="grid gap-3 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3">
       <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
        <FormField label="영상 품질" hint={videoQuality==='1080p'?'고화질 Pro 렌더링 · 추가 API 비용 발생 가능':'표준 720p 렌더링'}><SelectInput value={videoQuality} onChange={e=>setVideoQuality(e.target.value as '720p'|'1080p')}><option value="1080p">1080p · Pro</option><option value="720p">720p · Standard</option></SelectInput></FormField>
        <FormField label="샷 구성"><SelectInput value={shotMode} onChange={e=>setShotMode(e.target.value as 'auto'|'custom')}><option value="auto">자동 연출 · 5개 컷</option><option value="custom">직접 구성 · 3~5개 컷</option></SelectInput></FormField>
       </div>
       {shotMode==='custom'&&<FormField label="샷별 연출" hint="각 줄이 한 샷입니다. 총 길이에 맞춰 시간을 자동 배분합니다."><TextArea rows={5} value={customShots} onChange={e=>setCustomShots(e.target.value)} placeholder={'장면 1: 도시 야경을 비추는 와이드샷\n장면 2: 인물 클로즈업\n장면 3: 추적 카메라와 마무리 장면'}/></FormField>}
       <label className="flex items-center gap-2 text-xs text-[#5C544A]"><input type="checkbox" checked={generateAudio} onChange={e=>setGenerateAudio(e.target.checked)}/> AI 음향·음성 함께 생성</label>
      </div>}
      {usingComfy&&<FormField label={t('vid.seed')} hint={t('vid.seedHint')}><div className="flex gap-2"><TextInput value={seed} onChange={e=>setSeed(e.target.value)} placeholder={t('vid.randomValue')}/><Button variant="secondary" className="shrink-0" onClick={()=>setSeed('-1')}>{t('vid.random')}</Button></div></FormField>}
      <div className="flex flex-wrap items-center justify-between gap-3">{usingComfy?<p className="text-[11px] leading-5 text-[#8A8176]">{t('vid.placeholderNote')}</p>:usingRunway?<p className="text-[11px] leading-5 text-[#8A8176]">{ready?'Runway 유료 API로 영상·사운드를 생성합니다. 결과만 로컬에 저장됩니다.':'설정에서 Runway API 키를 등록하세요.'}</p>:usingLtx?<p className="text-[11px] leading-5 text-[#8A8176]">{ready?'LTX 유료 API에서 영상·오디오를 생성하고 완성된 MP4만 저장합니다.':'설정에서 LTX API 키를 등록하세요.'}</p>:<p className="text-[11px] leading-5 text-[#8A8176]">{ready?t('vid.silentNote'):t('vid.ffmpegRequired')}</p>}<Action disabled={generating||Boolean(videoSubmission&&!terminal(activeVideo.value?.status))||!ready||!prompt.trim()||(usingLtx&&(!paidQuote||!approvePaid||(ltxFilm&&!confirmPaidMultiShot)))} onClick={()=>void generate()}>{generating?<><LoaderCircle size={15} className="animate-spin"/> {t('vid.requesting')}</>:<><Play size={15}/> {t('vid.generate')}</>}</Action></div>
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
    <Panel className="min-w-0 p-5"><Subheading title={t('vid.results')} extra={videoJob?<StateTag online={videoJob.status==='completed'}>{statusLabel(videoJob.status)}</StateTag>:<span className="text-[11px] text-[#8A8176]">{t('vid.waiting')}</span>}/>{!videoJob?<div className="flex w-full flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-[#D8CFC2] bg-[#FAF7F1] text-[12px] text-[#8A8176]" style={{aspectRatio:ratios[ratio].width+" / "+ratios[ratio].height,maxHeight:420}}><Clapperboard size={24}/>{t('vid.resultsEmpty')} <small className="text-[10px]">{t(ratios[ratio].label)} · {ratios[ratio].width}×{ratios[ratio].height}</small></div>:<div className="grid gap-3">{!terminal(videoJob.status)&&<><div className="flex justify-between text-[11px] text-[#7C7469]"><span>{videoJob.status==='queued'?t('vid.waitingJob'):videoJob.status==='downloading'?t('vid.savingResult'):t('vid.generating')}</span><span>{(videoJob.progress??0)}%</span></div><div className="h-2 overflow-hidden rounded-full bg-[#F3EEE5]"><div className="h-full bg-[#C96442]" style={{width:Math.max(4,(videoJob.progress??0))+'%'}}/></div></>}{videoJob.result?.technicalQuality&&<p className="text-[11px] text-[#7C7469]">
       실제 영상 기술 검수: <strong>{videoJob.result.technicalQuality.status==='technical_pass'?'FFprobe 통과':
       '검수되지 않음'}</strong> · 기술적 재생 검증과 영상 시청 품질은 별개입니다.
      </p>}
      {videoJob.result?.shots&&<p className="text-[11px] text-[#7C7469]">완성 장면 {videoJob.result.shots}개 · 이번 실행 신규 유료 요청 {videoJob.result.newPaidCallsThisRun??0}건</p>}
      {(videoJob.shotMode||videoJob.videoModel&&videoJob.videoQuality)&&<p className="text-[11px] text-[#7C7469]">제작 설정: {videoJob.videoQuality||'1080p'} · {videoJob.shotMode?(videoJob.shotMode==='custom'?'사용자 샷 구성':'자동 멀티샷'):videoJob.videoModel} · {videoJob.generateAudio===false?'무음':'AI 오디오'}</p>}{videoJob.storyboard&&videoJob.storyboard.length>0&&<details className="text-xs text-[#5C544A]"><summary className="cursor-pointer font-medium">샷 구성 확인</summary><ol className="mt-2 list-decimal space-y-1 pl-5">{videoJob.storyboard.map((scene,i)=><li key={i}>{scene}</li>)}</ol></details>}{videoJob.film?.clips?.length&&terminal(videoJob.status)&&
     <FilmRetake jobId={videoJob.id} onCreated={job=>setVideoSubmission(job as unknown as VideoJob)}/>}
     {videoJob.outputUrl&&<div className="flex justify-center rounded-xl bg-black p-2"><video className="max-h-[520px] w-auto max-w-full rounded-lg" src={videoJob.outputUrl} controls playsInline/></div>}{videoJob.status==='completed'&&videoJob.outputUrl&&<div className="grid gap-2"><div className="flex min-w-0 items-center gap-2 rounded bg-[#FAF7F1] p-2"><span className="min-w-0 flex-1 truncate text-[11px] text-[#5C544A]" title={videoJob.title}>{videoJob.title||"output.mp4"}</span><Button variant="ghost" className="!px-2 !py-1 !text-[10px]" onClick={()=>void reveal(videoJob.id)}>{t('vid.openFolder')}</Button></div><Action onClick={useResult} className="w-full"><Upload size={14}/> {t('vid.fillForm')}</Action></div>}{(videoJob.status==='failed'||videoJob.status==='needs_review'||videoJob.status==='interrupted')&&<Notice message={videoJob.error||t('vid.generateFailed')}/>}</div>}</Panel>
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
