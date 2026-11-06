import {useEffect,useState} from 'react'
import {Check,Clapperboard,ExternalLink,LoaderCircle,Play,Power,RefreshCw,Upload} from 'lucide-react'
import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {Action,FormField,Headline,Notice,Panel,StateTag,Subheading,useRemote} from '../components/UI'
import {openOAuth,post} from '../api'
import {jobFinished,useJob,type JobState} from '../hooks/useJob'
import {t} from '../i18n'

type Workflow={id:string;name:string;bytes:number}
type VideoJob=JobState & {progress?:number;kind:'video';title?:string;brief:string;outputUrl?:string;result?:{bytes:number}}
type VideoEngine='native'|'comfyui'
type VideoStatus={config:{engine:VideoEngine;comfyUrl:string};native?:{available:boolean};comfy:{reachable:boolean;available?:boolean};workflows:Workflow[];jobs:VideoJob[]}
type UploadJob=JobState & {progress?:number;result?:{url:string}}
type YoutubeStatus={connected:boolean;uploadAllowed:boolean;account?:{displayName:string;scopes:string}|null;uploads:UploadJob[]}
const ratios={'9:16':{width:360,height:640,label:'쇼츠 · 9:16'},'16:9':{width:640,height:360,label:'가로 · 16:9'},'1:1':{width:480,height:480,label:'정사각 · 1:1'}} as const
const terminal=jobFinished
const statusLabel=(status:string)=>status==='completed'?'완료':status==='running'?'생성 중':status==='queued'?'대기':status==='downloading'?'저장 중':status==='failed'?'실패':status
const fmtBytes=(bytes=0)=>bytes>=1024**3?(bytes/1024**3).toFixed(1)+' GB':bytes>=1024**2?Math.round(bytes/1024**2)+' MB':'0 MB'

export function VideoPage({navigate}:{navigate:(page:any)=>void}){
 const video=useRemote<VideoStatus>('/api/video/status',{config:{engine:'comfyui',comfyUrl:''},comfy:{reachable:false},workflows:[],jobs:[]},false)
 const youtube=useRemote<YoutubeStatus>('/api/youtube/status',{connected:false,uploadAllowed:false,account:null,uploads:[]})
 const oauth=useRemote<{id:string;configured:boolean;callback?:string}[]>('/api/oauth/providers',[])
 const outputs=useRemote<VideoJob[]>('/api/video/outputs',[])
 const [workflow,setWorkflow]=useState(''),[title,setTitle]=useState(''),[prompt,setPrompt]=useState(''),[negative,setNegative]=useState(''),[ratio,setRatio]=useState<keyof typeof ratios>('9:16'),[seconds,setSeconds]=useState(3),[fps,setFps]=useState(16),[seed,setSeed]=useState('-1')
 const [engineChoice,setEngine]=useState<'native'|'comfyui'>(),[comfyUrl,setComfyUrl]=useState(''),[savingConfig,setSavingConfig]=useState(false)
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
 const engine=engineChoice??(video.value.config.engine==='native'?'native':'comfyui')
 const usingComfy=engine==='comfyui'
 const ready=usingComfy?Boolean(video.value.comfy.available&&workflow):Boolean(video.value.native?.available)
 const maxDuration=usingComfy?9:180
 useEffect(()=>{setComfyUrl(video.value.config.comfyUrl)},[video.value.config.comfyUrl])
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
  try{await openOAuth('youtube')}catch(e){setError('YouTube 연결 실패: '+(e as Error).message)}
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
   if(file.size>1024*1024)throw Error('워크플로는 1MiB 이하로 저장하세요.')
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
   setError('영상 길이, FPS, 시드 값을 확인하세요.');return;
  }
  setGenerating(true);setError('');
  try{
   setVideoSubmission(await post<VideoJob>('/api/video/jobs',{
    engine,title,brief:prompt,aspectRatio:ratio,duration:seconds,
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
  if(!youtube.value.uploadAllowed){setError('Google 계정을 YouTube 업로드 권한으로 먼저 연결하세요.');return}
  if(!selectedOutput||!uploadTitle.trim()||uploading)return;
  setUploading(true);setError('');
  try{
   setUploadSubmission(await post<UploadJob>('/api/video/jobs/'+selectedOutput.id+'/upload',{
    title:uploadTitle,description:uploadDescription,privacyStatus:uploadPrivacy,tags:uploadTags,
   }));
  }catch(e){setError((e as Error).message)}finally{setUploading(false)}
 }
 return <div className="mx-auto w-full max-w-[1400px] overflow-x-hidden px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
  <Headline eyebrow="VIDEO PRODUCTION" title={t('video.title')} description={t('video.description')} actions={<div className="flex items-center gap-2"><Button variant="secondary" onClick={()=>void connectGoogle()}>YouTube 계정 연동</Button><Button variant="secondary" disabled={refreshing} onClick={async()=>{setRefreshing(true);try{await Promise.allSettled([video.reload(),youtube.reload()])}finally{setRefreshing(false)}}}><RefreshCw size={16} className={refreshing?'animate-spin':''}/> {refreshing?t('video.refreshing'):t('video.refresh')}</Button></div>}/>
  {(error||video.error||outputs.error||activeVideo.error||activeUpload.error)&&<Notice message={error||video.error||outputs.error||activeVideo.error||activeUpload.error}/>}
  <div className="mb-3 flex flex-wrap items-center gap-2"><Button variant={tab==='create'?'primary':'ghost'} onClick={()=>setTab('create')}>{t('video.tab.create')}</Button><Button variant={tab==='settings'?'primary':'ghost'} onClick={()=>setTab('settings')}>{t('video.tab.settings')}</Button></div>
  {tab==='settings'&&<>
  <Panel className="mb-4 p-4">
   <div className="flex flex-wrap items-center justify-between gap-3">
    <div className="flex items-center gap-2"><Clapperboard size={18} className="text-[#C96442]"/><strong className="text-[13px] text-[#24211D]">영상 엔진</strong></div>
    <div className="flex flex-wrap items-center gap-2">
     <SelectInput aria-label="영상 엔진" value={engine} onChange={e=>setEngine(e.target.value as 'native'|'comfyui')} className="!w-auto !text-[12px]"><option value="native">자막 카드 영상 (FFmpeg)</option><option value="comfyui">ComfyUI (로컬 · 무료)</option></SelectInput>
    </div>
   </div>
  </Panel>
  <div className="mb-4 grid grid-cols-1 items-start gap-3 md:grid-cols-2">
   {engine==='comfyui'&&<Panel className={'p-4 '+(video.value.comfy.reachable?'border-emerald-200 bg-emerald-50/50':'border-amber-200 bg-amber-50/50')}>
    <div className="flex items-center justify-between gap-3"><div className="flex items-center gap-2"><Clapperboard size={18} className={video.value.comfy.reachable?'text-emerald-700':'text-amber-700'}/><strong className="text-[13px] text-[#24211D]">ComfyUI 영상 엔진</strong></div><StateTag online={video.value.comfy.reachable}>{video.value.comfy.reachable?'접속 확인됨':'접속 확인 안 됨'}</StateTag></div>
    <p className="mt-2 text-sm text-[#5C544A]">별도로 실행 중인 ComfyUI의 주소를 저장하세요. 모델 파일과 서버 시작·종료는 ComfyUI에서 관리합니다.</p>
    <div className="mt-2 flex flex-wrap gap-2"><TextInput aria-label="ComfyUI 서버 주소" value={comfyUrl} onChange={e=>setComfyUrl(e.target.value)} placeholder="http://127.0.0.1:8188"/><Button variant="secondary" disabled={savingConfig||!comfyUrl.trim()} onClick={()=>void saveComfy()}>{savingConfig?'저장 중':'저장'}</Button></div>
   </Panel>}
  </div>
   {usingComfy&&<div className="mb-4 rounded-xl border border-[#EED9CC] bg-[#FFF9F5] p-4">
     <strong className="text-[12px] text-[#3A342D]">{video.value.workflows.length?'저장된 워크플로 · '+video.value.workflows[0].name:'영상 워크플로가 아직 없습니다'}</strong>
     <ol className="mt-2 grid gap-1 text-[11px] leading-5 text-[#5C544A]">
      <li>1. ComfyUI에서 영상 만드는 작업(노드 연결)을 엽니다</li>
      <li>2. <b>Save (API Format)</b>으로 저장합니다 (일반 Save는 인식되지 않습니다)</li>
      <li>3. <b>JSON 파일 열기</b>로 등록합니다. 새 파일을 선택하면 이전 워크플로가 교체됩니다.</li>
     </ol>
     <div className="mt-2 flex flex-wrap items-center gap-2"><label className="inline-flex cursor-pointer items-center gap-2 rounded-xl border border-[#E1D9CC] bg-[#FFFDF9] px-3 py-2 text-[12px] font-semibold text-[#3A342D] hover:bg-[#F7F2EA]">
           JSON 파일 열기
           <input type="file" className="hidden" accept=".json,application/json" disabled={savingConfig} onChange={e=>{
             const file=e.currentTarget.files?.[0];e.currentTarget.value='';
             if(file)void importWorkflow(file);
           }}/>
         </label><Button variant="ghost" onClick={()=>void video.reload()}>다시 확인</Button></div>
    </div>}
  </>}
  <div className="grid grid-cols-1 items-start gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(320px,380px)]">
   <div className="grid min-w-0 content-start gap-4">
    {tab==='create'&&<Panel className="min-w-0 p-5">
     <Subheading title="영상 생성" extra={<span className="text-[11px] text-[#8A8176]">{usingComfy?video.value.workflows.length+"개 워크플로":"FFmpeg 자막 카드"}</span>}/>
     <div className="grid gap-4">
      <FormField label="영상 제목" hint="이 제목이 생성 파일 이름과 YouTube 제목 기본값이 됩니다."><TextInput value={title} onChange={e=>setTitle(e.target.value)} placeholder="예: 비 오는 서울 골목 산책"/></FormField>
      <FormField label="프롬프트"><TextArea rows={4} value={prompt} onChange={e=>setPrompt(e.target.value)} placeholder="예: 비 오는 서울 골목, 시네마틱, 천천히 걷는 고양이"/></FormField>
      {usingComfy&&<FormField label="네거티브 프롬프트" hint="영상에 나오면 안 되는 요소를 적습니다. 예: blurry, watermark, low quality, text"><TextInput value={negative} onChange={e=>setNegative(e.target.value)} placeholder="low quality, blurry, watermark"/></FormField>}
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3"><FormField label="화면 비율" hint={`${ratios[ratio].width}×${ratios[ratio].height}px`}><SelectInput value={ratio} onChange={e=>setRatio(e.target.value as keyof typeof ratios)}>{(Object.keys(ratios) as Array<keyof typeof ratios>).map(key=><option key={key} value={key}>{ratios[key].label}</option>)}</SelectInput></FormField><FormField label="길이(초)" hint={"최대 "+maxDuration+"초"}><TextInput type="number" min={3} max={maxDuration} value={seconds} onChange={e=>setSeconds(Number(e.target.value))}/></FormField>{usingComfy&&<FormField label="FPS" hint="초당 프레임 수. 높을수록 부드럽지만 느립니다."><TextInput type="number" min={1} max={60} value={fps} onChange={e=>setFps(Number(e.target.value))}/></FormField>}</div>
      {usingComfy&&<FormField label="시드" hint="같은 숫자를 넣으면 같은 영상이 다시 나옵니다. -1은 생성할 때마다 새 랜덤 값."><div className="flex gap-2"><TextInput value={seed} onChange={e=>setSeed(e.target.value)} placeholder="-1 (랜덤)"/><Button variant="secondary" className="shrink-0" onClick={()=>setSeed('-1')}>랜덤</Button></div></FormField>}
      <div className="flex flex-wrap items-center justify-between gap-3">{usingComfy?<p className="text-[11px] leading-5 text-[#8A8176]">워크플로에 {`{{prompt}}`} {`{{width}}`} {`{{height}}`} {`{{frames}}`} {`{{fps}}`} {`{{seed}}`} 자리표시자가 있는 값만 이 화면의 설정으로 바뀝니다.</p>:<p className="text-[11px] leading-5 text-[#8A8176]">{ready?"입력한 내용으로 무음 자막 카드 영상을 만듭니다.":"FFmpeg가 설치되어 있어야 합니다."}</p>}<Action disabled={generating||Boolean(videoSubmission&&!terminal(activeVideo.value?.status))||!ready||!prompt.trim()} onClick={()=>void generate()}>{generating?<><LoaderCircle size={15} className="animate-spin"/> 요청 중</>:<><Play size={15}/> 영상 생성</>}</Action></div>
     </div>
    </Panel>}
    {tab==='settings'&&<Panel className="min-w-0 p-5">
     <div className="mt-4 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3">
     <div className="flex items-center justify-between gap-2"><strong className="text-[12px] text-[#3A342D]">YouTube 연결</strong><StateTag online={gReady}>{gReady?'앱 설정됨':'앱 설정 필요'}</StateTag></div>
     <p className="mt-1 text-[10px] leading-4 text-[#8A8176]">Google Cloud OAuth 클라이언트의 <b>승인된 리디렉션 URI</b>에 아래 주소를 그대로 등록하세요. 화면 주소가 아니라 인증 전용 콜백 주소입니다.</p>{gCallback&&<div className="mt-1 flex items-center gap-2 rounded border border-[#E7E0D5] bg-[#FFFDF9] p-2"><code className="min-w-0 flex-1 break-all text-[10px] text-[#3A342D]">{gCallback}</code><Button variant="ghost" className="!px-2 !py-0.5 !text-[10px]" onClick={async()=>{try{await navigator.clipboard.writeText(gCallback);setError('콜백 주소를 복사했습니다.')}catch(e){setError((e as Error).message)}}}>복사</Button></div>}
     <div className="mt-2 grid gap-2 sm:grid-cols-2"><TextInput value={gid} onChange={e=>setGid(e.target.value)} placeholder="Client ID"/><TextInput type="password" value={gsecret} onChange={e=>setGsecret(e.target.value)} placeholder="Client Secret"/></div>
     <div className="mt-2 flex flex-wrap gap-2"><Button variant="secondary" disabled={gBusy||!gid.trim()||!gsecret.trim()} onClick={()=>void saveGoogle()}>{gBusy?'저장 중':'OAuth 앱 저장'}</Button><Button variant="secondary" onClick={()=>void connectGoogle()}>계정 연결</Button></div>
    </div>
    </Panel>}
   </div>
   {tab==='create'&&<div className="grid min-w-0 content-start gap-4">
    <Panel className="min-w-0 p-5"><Subheading title="생성 결과" extra={videoJob?<StateTag online={videoJob.status==='completed'}>{statusLabel(videoJob.status)}</StateTag>:<span className="text-[11px] text-[#8A8176]">대기</span>}/>{!videoJob?<div className="flex w-full flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-[#D8CFC2] bg-[#FAF7F1] text-[12px] text-[#8A8176]" style={{aspectRatio:ratios[ratio].width+" / "+ratios[ratio].height,maxHeight:420}}><Clapperboard size={24}/>영상을 생성하면 여기에 표시됩니다. <small className="text-[10px]">{ratios[ratio].label} · {ratios[ratio].width}×{ratios[ratio].height}</small></div>:<div className="grid gap-3">{!terminal(videoJob.status)&&<><div className="flex justify-between text-[11px] text-[#7C7469]"><span>{videoJob.status==='queued'?'작업 대기 중':videoJob.status==='downloading'?'결과 저장 중':'생성 중'}</span><span>{(videoJob.progress??0)}%</span></div><div className="h-2 overflow-hidden rounded-full bg-[#F3EEE5]"><div className="h-full bg-[#C96442]" style={{width:Math.max(4,(videoJob.progress??0))+'%'}}/></div></>}{videoJob.outputUrl&&<div className="flex justify-center rounded-xl bg-black p-2"><video className="max-h-[520px] w-auto max-w-full rounded-lg" src={videoJob.outputUrl} controls playsInline/></div>}{videoJob.status==='completed'&&videoJob.outputUrl&&<div className="grid gap-2"><div className="flex min-w-0 items-center gap-2 rounded bg-[#FAF7F1] p-2"><span className="min-w-0 flex-1 truncate text-[11px] text-[#5C544A]" title={videoJob.title}>{videoJob.title||"output.mp4"}</span><Button variant="ghost" className="!px-2 !py-1 !text-[10px]" onClick={()=>void reveal(videoJob.id)}>폴더 열기</Button></div><Action onClick={useResult} className="w-full"><Upload size={14}/> 업로드 폼에 채우기</Action></div>}{(videoJob.status==='failed'||videoJob.status==='needs_review'||videoJob.status==='interrupted')&&<Notice message={videoJob.error||'영상 생성에 실패했습니다.'}/>}</div>}</Panel>
    <Panel className="min-w-0 p-5"><Subheading title="YouTube 업로드" extra={<span className="text-[11px] text-[#8A8176]">{selectedOutput?'파일 준비됨':'파일 선택'}</span>}/><div className="grid gap-3"><FormField label="영상 파일" hint="DOMINION에서 생성이 끝난 영상입니다."><div className="flex flex-col gap-2">{outputs.value.length?<SelectInput value={selectedOutput?.id||''} onChange={e=>{
 const picked=outputs.value.find(item=>item.id===e.target.value);
 if(!picked)return;
 setUploadFile(picked.id);
 setUploadTitle(picked.title||'');
 setUploadDescription(picked.brief);
 }}>{outputs.value.map(item=><option key={item.id} value={item.id}>{item.title||'output.mp4'} · {fmtBytes(item.result?.bytes)}</option>)}</SelectInput>:<p className="text-[11px] text-[#8A8176]">아직 업로드할 생성 영상이 없습니다.</p>}</div></FormField><FormField label="제목"><TextInput value={uploadTitle} onChange={e=>setUploadTitle(e.target.value)} placeholder="영상 제목" maxLength={100}/></FormField><FormField label="설명"><TextArea rows={3} value={uploadDescription} onChange={e=>setUploadDescription(e.target.value)} placeholder="설명"/></FormField><div className="grid grid-cols-2 gap-3"><FormField label="공개"><SelectInput value={uploadPrivacy} onChange={e=>setUploadPrivacy(e.target.value)}><option value="private">비공개</option><option value="unlisted">일부 공개</option><option value="public">공개</option></SelectInput></FormField><FormField label="태그"><TextInput value={uploadTags} onChange={e=>setUploadTags(e.target.value)} placeholder="쉼표 구분"/></FormField></div><Action disabled={uploading||Boolean(uploadSubmission&&!terminal(uploadJob?.status))||!youtube.value.uploadAllowed||!selectedOutput||!uploadTitle.trim()} onClick={()=>void startUpload()} className="w-full">{uploading?<><LoaderCircle size={15} className="animate-spin"/> 요청 중</>:<><Upload size={15}/> YouTube 업로드</>}</Action>{uploadJob&&<div className="rounded-lg border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-[11px]"><div className="flex justify-between"><span>{uploadJob.status}</span><span>{uploadJob.progress??0}%</span></div>{uploadJob.result?.url&&<a className="mt-1 inline-flex items-center gap-1 font-semibold text-[#C96442]" href={uploadJob.result.url} target="_blank" rel="noreferrer">YouTube에서 열기 <ExternalLink size={12}/></a>}{uploadJob.error&&<p className="mt-1 text-rose-600">{uploadJob.error}</p>}</div>}</div></Panel>
   </div>}
  </div>
 </div>
}
