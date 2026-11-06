import {useEffect,useState} from 'react'
import {Check,Clapperboard,ExternalLink,LoaderCircle,Play,Power,RefreshCw,Upload} from 'lucide-react'
import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {Action,FormField,Headline,Notice,Panel,StateTag,Subheading,useRemote} from '../components/UI'
import {get,post} from '../api'
import {t} from '../i18n'

type Workflow={id:string;name:string;bytes:number}
type VideoJob={id:string;status:string;progress:number;title?:string;prompt:string;width:number;height:number;frames:number;fps:number;seconds?:number;seed:number;error?:string;result?:{file:string;filename:string;url:string}}
type VideoModel={type:string;name:string;present:boolean;path:string;sizeBytes?:number;download?:string}
type VideoStatus={config:{engine:'comfyui'|'openai'|'custom'|'huggingface'|'replicate';videoApiUrl?:string;videoModel?:string;hasVideoKey?:boolean;comfyUrl:string;comfyDir:string;workflowDir:string;outputDir:string};comfy:{reachable:boolean;error?:string};installed?:boolean;workflows:Workflow[];models?:VideoModel[];modelsReady?:boolean;jobs:VideoJob[]}
type UploadJob={id:string;status:string;progress:number;sent:number;total:number;title:string;privacyStatus:string;error?:string;result?:{id:string;url:string;title:string;status:string;privacyStatus:string}}
type YoutubeStatus={connected:boolean;uploadAllowed:boolean;account?:{displayName:string;scopes:string}|null;uploads:UploadJob[]}
const ratios={'9:16':{width:360,height:640,label:'쇼츠 · 9:16'},'16:9':{width:640,height:360,label:'가로 · 16:9'},'1:1':{width:480,height:480,label:'정사각 · 1:1'}} as const
const terminal=(status:string)=>['completed','failed','cancelled'].includes(status)
const statusLabel=(status:string)=>status==='completed'?'완료':status==='running'?'생성 중':status==='queued'?'대기':status==='downloading'?'저장 중':status==='failed'?'실패':status
const fmtBytes=(bytes=0)=>bytes>=1024**3?(bytes/1024**3).toFixed(1)+' GB':bytes>=1024**2?Math.round(bytes/1024**2)+' MB':'0 MB'
const prettyPath=(value:string)=>String(value||'').replace(/^\/Users\/[^/]+/,'~').replace(/^\/home\/[^/]+/,'~');

export function VideoPage({navigate}:{navigate:(page:any)=>void}){
 const video=useRemote<VideoStatus>('/api/video/status',{config:{engine:'comfyui',comfyUrl:'',comfyDir:'',workflowDir:'',outputDir:''},comfy:{reachable:false},workflows:[],models:[],jobs:[]})
 const youtube=useRemote<YoutubeStatus>('/api/youtube/status',{connected:false,uploadAllowed:false,account:null,uploads:[]})
 const [workflow,setWorkflow]=useState(''),[title,setTitle]=useState(''),[prompt,setPrompt]=useState(''),[negative,setNegative]=useState(''),[ratio,setRatio]=useState<keyof typeof ratios>('9:16'),[seconds,setSeconds]=useState(2),[fps,setFps]=useState(16),[seed,setSeed]=useState('-1')
 const [engine,setEngine]=useState<'comfyui'|'openai'|'custom'|'huggingface'|'replicate'>('comfyui'),[videoApiUrl,setVideoApiUrl]=useState(''),[videoModel,setVideoModel]=useState(''),[videoApiKey,setVideoApiKey]=useState(''),[comfyUrl,setComfyUrl]=useState(''),[comfyDir,setComfyDir]=useState(''),[workflowDir,setWorkflowDir]=useState(''),[outputDir,setOutputDir]=useState(''),[showPaths,setShowPaths]=useState(false),[savingConfig,setSavingConfig]=useState(false),[starting,setStarting]=useState(false),[stopping,setStopping]=useState(false)
 const [tab,setTab]=useState<'create'|'settings'>('create'),[refreshing,setRefreshing]=useState(false),[hfq,setHfq]=useState(''),[hf,setHf]=useState<{id:string;url:string;downloads:number;pipeline:string}[]>([]),[hfBusy,setHfBusy]=useState(false),[hfOpen,setHfOpen]=useState(''),[hfFiles,setHfFiles]=useState<{path:string;size:number}[]>([]),[hfFilesBusy,setHfFilesBusy]=useState(false),[dlJob,setDlJob]=useState<any>(null),[gid,setGid]=useState(''),[gsecret,setGsecret]=useState(''),[gBusy,setGBusy]=useState(false),[gReady,setGReady]=useState(false),[gCallback,setGCallback]=useState(''),[locs,setLocs]=useState<{comfy:string[];workflows:string[];outputs:string[]}>({comfy:[],workflows:[],outputs:[]}),[modelOptions,setModelOptions]=useState<{value:string;label:string;provider:string;model:string;baseUrl:string}[]>([]),[outputs,setOutputs]=useState<{file:string;name:string;bytes:number;modified:string}[]>([]),[videoJob,setVideoJob]=useState<VideoJob|null>(null),[generating,setGenerating]=useState(false),[error,setError]=useState('')
 const [uploadFile,setUploadFile]=useState(''),[uploadTitle,setUploadTitle]=useState(''),[uploadDescription,setUploadDescription]=useState(''),[uploadPrivacy,setUploadPrivacy]=useState('private'),[uploadTags,setUploadTags]=useState('')
 const [uploadJob,setUploadJob]=useState<UploadJob|null>(null),[uploading,setUploading]=useState(false)
 const modelTotal=(video.value.models||[]).reduce((sum,model)=>sum+(model.sizeBytes||0),0)
 useEffect(()=>{get<any[]>('/api/video/outputs').then(rows=>{setOutputs(rows||[]);setUploadFile(old=>old||(rows?.[0]?.file||''))}).catch(()=>{})},[videoJob?.status])
 useEffect(()=>{if(engine==='comfyui')return;void get<any[]>('/api/video/models').then(rows=>setModelOptions(rows||[])).catch(()=>{})},[engine])
 useEffect(()=>{if(engine!=='huggingface'||hf.length)return;void get<any[]>('/api/video/catalog?q=wan').then(setHf).catch(()=>{})},[engine,hf.length])
 useEffect(()=>{get<any[]>('/api/oauth/providers').then(rows=>{const g=(rows||[]).find(x=>x.id==='google');if(g){setGReady(Boolean(g.configured));setGCallback(String(g.callback||''))}}).catch(()=>{})},[])
 useEffect(()=>{const c=video.value.config;if(c.engine)setEngine(c.engine);if(c.videoApiUrl)setVideoApiUrl(c.videoApiUrl);if(c.videoModel)setVideoModel(c.videoModel);if(c.comfyUrl)setComfyUrl(c.comfyUrl);if(c.comfyDir)setComfyDir(prettyPath(c.comfyDir));if(c.workflowDir)setWorkflowDir(prettyPath(c.workflowDir));if(c.outputDir)setOutputDir(prettyPath(c.outputDir))},[video.value.config.comfyUrl,video.value.config.comfyDir,video.value.config.workflowDir,video.value.config.outputDir])
 useEffect(()=>{if(!workflow&&video.value.workflows[0])setWorkflow(video.value.workflows[0].id)},[video.value.workflows,workflow])
 useEffect(()=>{if(videoJob)return;const latest=[...video.value.jobs].reverse().find(job=>job.status==='completed');if(latest)setVideoJob(latest)},[video.value.jobs])
 useEffect(()=>{
  if(!videoJob||terminal(videoJob.status))return
  const timer=setInterval(async()=>{
   try{const next=await get<VideoJob>('/api/video/jobs/'+videoJob.id);setVideoJob(next);if(terminal(next.status)){void video.reload();clearInterval(timer)}}catch{}
  },2000)
  return()=>clearInterval(timer)
 },[videoJob?.id,videoJob?.status])
 useEffect(()=>{
  if(!uploadJob||terminal(uploadJob.status))return
  const timer=setInterval(async()=>{
   try{const next=await get<UploadJob>('/api/youtube/uploads/'+uploadJob.id);setUploadJob(next);if(terminal(next.status)){void youtube.reload();clearInterval(timer)}}catch{}
  },1500)
  return()=>clearInterval(timer)
 },[uploadJob?.id,uploadJob?.status])
 const openRepo=async(repo:string)=>{if(hfOpen===repo){setHfOpen('');return}setHfOpen(repo);setHfFilesBusy(true);setHfFiles([]);try{setHfFiles(await get<any[]>('/api/video/files?repo='+encodeURIComponent(repo)))}catch(e){setError((e as Error).message)}finally{setHfFilesBusy(false)}}
 const downloadFile=async(repo:string,file:string)=>{setError('');try{const job=await post<any>('/api/video/download',{repo,file});setDlJob(job);const timer=setInterval(async()=>{try{const next=await get<any>('/api/video/download/'+job.id);setDlJob(next);if(['completed','failed'].includes(next.status)){clearInterval(timer);void video.reload()}}catch{clearInterval(timer)}},1000)}catch(e){setError((e as Error).message)}}
 const saveGoogle=async()=>{setGBusy(true);setError('');try{await post('/api/oauth/providers/google',{clientId:gid.trim(),clientSecret:gsecret.trim()});setGReady(true);setGsecret('')}catch(e){setError((e as Error).message)}finally{setGBusy(false)}}
 const connectGoogle=async()=>{setError('');try{const r=await get<{url:string}>('/api/oauth/google/start');const w=window.open(r.url,'dominion_oauth','width=650,height=780');if(!w)setError('팝업이 차단되었습니다. 팝업 허용 후 다시 시도하세요.')}catch(e){setError('YouTube 연결 실패: '+(e as Error).message)}}
 const chooseDir=(setter:(value:string)=>void)=>async()=>{setError('');try{const r=await post<{path:string}>('/api/system/pick-folder',{prompt:'폴더 선택'});if(r?.path)setter(prettyPath(r.path))}catch(e){setError((e as Error).message)}}
 const searchHf=async()=>{setHfBusy(true);setError('');try{setHf(await get<any[]>('/api/video/catalog?q='+encodeURIComponent(hfq.trim()||'wan')))}catch(e){setError((e as Error).message)}finally{setHfBusy(false)}}
 const saveComfy=async()=>{setSavingConfig(true);setError('');try{await post('/api/video/config',{engine,videoApiUrl,videoModel,videoApiKey,comfyUrl,comfyDir,workflowDir,outputDir});setVideoApiKey('');await video.reload()}catch(e){setError((e as Error).message)}finally{setSavingConfig(false)}}
 const startComfy=async()=>{setStarting(true);setError('');try{await post('/api/video/start',{});setTimeout(()=>{void video.reload();setStarting(false)},5000)}catch(e){setError((e as Error).message);setStarting(false)}}
 const stopComfy=async()=>{setStopping(true);setError('');try{await post('/api/video/stop',{});await video.reload()}catch(e){setError((e as Error).message)}finally{setStopping(false)}}
 const reveal=async(file:string)=>{
  const bridge=(window as any).dominionDesktop
  try{
   if(bridge?.showItem){const result=await bridge.showItem(file);if(result&&!result.ok)setError(result.error||'폴더를 열 수 없습니다.');return}
   await post('/api/video/reveal',{file})
  }catch(e){setError((e as Error).message)}
 }
 const openModels=async()=>{
  const bridge=(window as any).dominionDesktop
  try{
   if(bridge?.openPath){const result=await bridge.openPath(video.value.config.comfyDir.replace(/\\+$/,'')+'/models');if(result&&!result.ok)setError(result.error||'모델 폴더를 열 수 없습니다.');return}
   await post('/api/video/open-models',{})
  }catch(e){setError((e as Error).message)}
 }
 const generate=async()=>{
  if(!workflow){setError('영상 모델(워크플로)을 선택하세요.');return}
  if(!prompt.trim()){setError('영상 프롬프트를 입력하세요.');return}
  setGenerating(true);setError('');setUploadFile('');setUploadTitle('');setUploadDescription('');
  try{
   const dimensions=ratios[ratio]
   setVideoJob(await post<VideoJob>('/api/video/generate',{workflow,title,prompt,negative,width:dimensions.width,height:dimensions.height,seconds,fps,seed:Number(seed)}))
  }catch(e){setError((e as Error).message)}finally{setGenerating(false)}
 }
 const useResult=()=>{
  if(!videoJob?.result)return
  setUploadFile(videoJob.result.file)
  setUploadTitle((videoJob.title||videoJob.prompt).slice(0,90))
  setUploadDescription(videoJob.prompt)
 }
 const startUpload=async()=>{
  if(!youtube.value.uploadAllowed){setError('Google 계정을 YouTube 업로드 권한으로 먼저 연결하세요.');return}
  if(!uploadFile.trim()||!uploadTitle.trim()){setError('영상 파일과 제목을 입력하세요.');return}
  setUploading(true);setError('')
  try{setUploadJob(await post<UploadJob>('/api/youtube/upload',{path:uploadFile,title:uploadTitle,description:uploadDescription,privacyStatus:uploadPrivacy,tags:uploadTags}));void youtube.reload()}
  catch(e){setError((e as Error).message)}finally{setUploading(false)}
 }
 return <div className="mx-auto w-full max-w-[1400px] overflow-x-hidden px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
  <Headline eyebrow="VIDEO PRODUCTION" title={t('video.title')} description={t('video.description')} actions={<div className="flex items-center gap-2"><Button variant="secondary" onClick={()=>void connectGoogle()}>YouTube 계정 연동</Button><Button variant="secondary" disabled={refreshing} onClick={async()=>{setRefreshing(true);try{await Promise.allSettled([video.reload(),youtube.reload()])}finally{setRefreshing(false)}}}><RefreshCw size={16} className={refreshing?'animate-spin':''}/> {refreshing?t('video.refreshing'):t('video.refresh')}</Button></div>}/>
  {error&&<Notice message={error}/>}
  <div className="mb-3 flex flex-wrap items-center gap-2"><Button variant={tab==='create'?'primary':'ghost'} onClick={()=>setTab('create')}>{t('video.tab.create')}</Button><Button variant={tab==='settings'?'primary':'ghost'} onClick={()=>setTab('settings')}>{t('video.tab.settings')}</Button></div>
  {tab==='settings'&&<>
  <Panel className="mb-4 p-4">
   <div className="flex flex-wrap items-center justify-between gap-3">
    <div className="flex items-center gap-2"><Clapperboard size={18} className="text-[#C96442]"/><strong className="text-[13px] text-[#24211D]">영상 엔진</strong></div>
    <div className="flex flex-wrap items-center gap-2">
     <SelectInput aria-label="영상 엔진" value={engine} onChange={e=>setEngine(e.target.value as any)} className="!w-auto !text-[12px]"><option value="comfyui">ComfyUI (로컬 · 무료)</option><option value="openai">OpenAI 호환 API (Sora 등)</option><option value="custom">직접 지정 API</option><option value="huggingface">Hugging Face (무료 토큰)</option><option value="replicate">Replicate (모델 다수 · 유료)</option></SelectInput>
     {engine!=='comfyui'&&<Button variant="secondary" disabled={savingConfig} onClick={()=>void saveComfy()}>{savingConfig?'저장 중':'엔진 저장'}</Button>}
    </div>
   </div>
   {engine!=='comfyui'&&<div className="mt-3 grid gap-2 sm:grid-cols-3">
    {engine!=='huggingface'&&<FormField label="API 주소" hint="OpenAI 호환이면 /v1 까지"><TextInput value={videoApiUrl} onChange={e=>setVideoApiUrl(e.target.value)} placeholder="https://api.openai.com/v1"/></FormField>}
    <FormField label="모델" hint="모델 관리에 연결된 제공자와 Hugging Face 모델을 합쳐서 보여줍니다">
      <SelectInput value={modelOptions.some(o=>o.model===videoModel)?(modelOptions.find(o=>o.model===videoModel)?.value||''):''} onChange={e=>{const picked=modelOptions.find(o=>o.value===e.target.value);if(!picked)return;setVideoModel(picked.model);if(picked.provider==='huggingface'){setEngine('huggingface')}else{setEngine('openai');setVideoApiUrl(picked.baseUrl)}}}>
       <option value="">모델 선택</option>
       {modelOptions.map(o=><option key={o.value} value={o.value}>{o.label}</option>)}
      </SelectInput>
     </FormField>
    <FormField label={engine==='huggingface'?'HF 토큰':'API 키'} hint={video.value.config.hasVideoKey?'저장됨 · 변경할 때만 입력':'필수'}><TextInput type="password" value={videoApiKey} onChange={e=>setVideoApiKey(e.target.value)} placeholder="sk-..."/></FormField>
   </div>}
  </Panel>
  <div className="mb-4 grid grid-cols-1 items-start gap-3 md:grid-cols-2">
   {engine==='comfyui'&&<Panel className={'p-4 '+(video.value.comfy.reachable?'border-emerald-200 bg-emerald-50/50':'border-amber-200 bg-amber-50/50')}>
    <div className="flex items-center justify-between gap-3"><div className="flex items-center gap-2"><Clapperboard size={18} className={video.value.comfy.reachable?'text-emerald-700':'text-amber-700'}/><strong className="text-[13px] text-[#24211D]">ComfyUI 영상 엔진</strong></div><StateTag online={video.value.comfy.reachable}>{video.value.comfy.reachable?'실행 중':'꺼짐'}</StateTag></div>
    <p className="mt-2 text-[11px] leading-5 text-[#5C544A]">{video.value.comfy.reachable?'영상 생성할 때만 켜두면 됩니다. 앱을 닫아도 이 엔진은 따로 실행되므로 아래 버튼으로 종료하세요.':'영상 생성할 때만 시작하세요. 앱과 별도 프로세스라 켜져 있는 동안 GPU 메모리를 사용합니다. 이 PC의 ComfyUI 폴더는 약 14.5GB를 사용하며, 필요 없으면 폴더째 삭제해도 앱의 다른 기능은 그대로 동작합니다.'}</p>
    {video.value.comfy.reachable
     ? <div className="mt-2 flex flex-wrap items-center gap-2"><Button variant="secondary" disabled={stopping} onClick={()=>void stopComfy()}><Power size={14}/> {stopping?'종료 중':'ComfyUI 종료'}</Button><span className="text-[10px] leading-4 text-[#8A8176]">종료하면 VRAM이 바로 반환됩니다.</span></div>
     : <div className="mt-2 flex flex-wrap gap-2"><TextInput value={comfyUrl} onChange={e=>setComfyUrl(e.target.value)} placeholder="http://127.0.0.1:8188"/><Button variant="secondary" disabled={savingConfig||!comfyUrl.trim()} onClick={()=>void saveComfy()}>{savingConfig?'저장 중':'저장'}</Button>{video.value.installed&&<Button variant="primary" disabled={starting} onClick={()=>void startComfy()}>{starting?'시작 중':'ComfyUI 시작'}</Button>}</div>}
    <Button type="button" variant="ghost" onClick={()=>{const next=!showPaths;setShowPaths(next);if(next)void get<any>('/api/video/locations').then(r=>setLocs(r)).catch(()=>{})}} className="!h-7 !px-2 !text-[11px] text-[#5C544A]">{showPaths?'▾':'▸'} 고급: 폴더 경로 변경</Button>
    {showPaths&&<Panel className="mt-2 grid gap-2 p-4">
     <FormField label="ComfyUI 설치 폴더" hint="main.py와 .venv가 있는 폴더"><div className="flex gap-2"><TextInput value={comfyDir} onChange={e=>setComfyDir(e.target.value)}/><Button variant="secondary" className="shrink-0 !px-3 !text-[11px]" onClick={()=>void chooseDir(setComfyDir)()}>찾기</Button></div></FormField>
     <FormField label="워크플로 폴더" hint="API 형식 JSON을 넣는 곳"><div className="flex gap-2"><TextInput value={workflowDir} onChange={e=>setWorkflowDir(e.target.value)}/><Button variant="secondary" className="shrink-0 !px-3 !text-[11px]" onClick={()=>void chooseDir(setWorkflowDir)()}>찾기</Button></div></FormField>
     <FormField label="출력 폴더" hint="생성된 영상 저장 위치"><div className="flex gap-2"><TextInput value={outputDir} onChange={e=>setOutputDir(e.target.value)}/><Button variant="secondary" className="shrink-0 !px-3 !text-[11px]" onClick={()=>void chooseDir(setOutputDir)()}>찾기</Button></div></FormField>
     <div className="flex justify-end"><Button variant="secondary" className="!h-8 !px-3 !text-[11px]" disabled={savingConfig} onClick={()=>void saveComfy()}>{savingConfig?'저장 중':'폴더 저장'}</Button></div>
    </Panel>}
   </Panel>}
  </div>
   {(!video.value.workflows.length)&&<div className="mb-4 rounded-xl border border-[#EED9CC] bg-[#FFF9F5] p-4">
     <strong className="text-[12px] text-[#3A342D]">영상 워크플로가 아직 없습니다</strong>
     <ol className="mt-2 grid gap-1 text-[11px] leading-5 text-[#5C544A]">
      <li>1. ComfyUI에서 영상 만드는 작업(노드 연결)을 엽니다</li>
      <li>2. <b>Save (API Format)</b>으로 저장합니다 (일반 Save는 인식되지 않습니다)</li>
      <li>3. 받은 .json 파일을 아래 폴더에 넣고 <b>다시 확인</b>을 누릅니다</li>
     </ol>
     <div className="mt-2 flex flex-wrap items-center gap-2"><label className="inline-flex cursor-pointer items-center gap-2 rounded-xl border border-[#E1D9CC] bg-[#FFFDF9] px-3 py-2 text-[12px] font-semibold text-[#3A342D] hover:bg-[#F7F2EA]">
           JSON 파일 열기
           <input type="file" className="hidden" accept=".json,application/json" onChange={async(e)=>{
             const file=e.target.files?.[0];if(!file)return;
             try{
               const up=await fetch('/api/upload?name='+encodeURIComponent(file.name),{method:'POST',body:file}).then(r=>r.json());
               await post('/api/video/workflows/import',{source:up.path});
               await video.reload();
             }catch(err){setError((err as Error).message)}
           }}/>
         </label><Button variant="ghost" onClick={()=>void video.reload()}>다시 확인</Button><span className="text-[10px] text-[#8A8176]">{prettyPath(video.value.config.workflowDir)}</span></div>
    </div>}
  </>}
  <div className="grid grid-cols-1 items-start gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(320px,380px)]">
   <div className="grid min-w-0 content-start gap-4">
    {tab==='create'&&<Panel className="min-w-0 p-5">
     <Subheading title="영상 생성" extra={<span className="text-[11px] text-[#8A8176]">{video.value.workflows.length}개 워크플로</span>}/>
     <div className="grid gap-4">
      <FormField label="영상 제목" hint="이 제목이 생성 파일 이름과 YouTube 제목 기본값이 됩니다."><TextInput value={title} onChange={e=>setTitle(e.target.value)} placeholder="예: 비 오는 서울 골목 산책"/></FormField>
      <FormField label="프롬프트"><TextArea rows={4} value={prompt} onChange={e=>setPrompt(e.target.value)} placeholder="예: 비 오는 서울 골목, 시네마틱, 천천히 걷는 고양이"/></FormField>
      <FormField label="네거티브 프롬프트" hint="영상에 나오면 안 되는 요소를 적습니다. 예: blurry, watermark, low quality, text"><TextInput value={negative} onChange={e=>setNegative(e.target.value)} placeholder="low quality, blurry, watermark"/></FormField>
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3"><FormField label="화면 비율" hint={`${ratios[ratio].width}×${ratios[ratio].height}px`}><SelectInput value={ratio} onChange={e=>setRatio(e.target.value as keyof typeof ratios)}>{(Object.keys(ratios) as Array<keyof typeof ratios>).map(key=><option key={key} value={key}>{ratios[key].label}</option>)}</SelectInput></FormField><FormField label="길이(초)" hint="최대 9초"><TextInput type="number" min={1} max={9} value={seconds} onChange={e=>setSeconds(Number(e.target.value))}/></FormField><FormField label="FPS" hint="초당 프레임 수. 높을수록 부드럽지만 느립니다."><TextInput type="number" min={1} max={60} value={fps} onChange={e=>setFps(Number(e.target.value))}/></FormField></div>
      <FormField label="시드" hint="같은 숫자를 넣으면 같은 영상이 다시 나옵니다. -1은 생성할 때마다 새 랜덤 값."><div className="flex gap-2"><TextInput value={seed} onChange={e=>setSeed(e.target.value)} placeholder="-1 (랜덤)"/><Button variant="secondary" className="shrink-0" onClick={()=>setSeed('-1')}>랜덤</Button></div></FormField>
      <div className="flex flex-wrap items-center justify-between gap-3"><p className="text-[11px] leading-5 text-[#8A8176]">워크플로에 {`{{prompt}}`} {`{{width}}`} {`{{height}}`} {`{{frames}}`} {`{{fps}}`} {`{{seed}}`} 자리표시자가 필요합니다.</p><Action disabled={generating||!video.value.comfy.reachable||!workflow||!prompt.trim()} onClick={()=>void generate()}>{generating?<><LoaderCircle size={15} className="animate-spin"/> 요청 중</>:<><Play size={15}/> 영상 생성</>}</Action></div>
     </div>
    </Panel>}
    {tab==='settings'&&<><Panel className="min-w-0 p-5"><Subheading title="영상 모델 · 워크플로" extra={<span className="text-[11px] text-[#8A8176]">{video.value.workflows.length}개</span>}/><p className="mb-2 text-[11px] leading-5 text-[#5C544A]">워크플로는 <b>영상을 만드는 순서를 적은 설계도 파일(.json)</b>입니다. ComfyUI에서 노드를 연결해 만든 뒤 <b>Save (API Format)</b>으로 저장하면 생깁니다.</p><SelectInput value={workflow} onChange={e=>setWorkflow(e.target.value)}>{video.value.workflows.map(item=><option key={item.id} value={item.id}>{item.name}</option>)}</SelectInput>{!video.value.workflows.length&&<p className="mt-2 text-[11px] text-[#8A8176]">워크플로 JSON을 폴더에 넣으면 여기에 나타납니다.</p>}</Panel>
    {engine==='comfyui'&&<Panel className="min-w-0 p-5">
     <Subheading title="영상 모델 파일" extra={<StateTag online={Boolean(video.value.modelsReady)}>{video.value.modelsReady?'준비됨':(video.value.models||[]).length?'확인 필요':'워크플로 필요'}</StateTag>}/>
     <p className="text-[11px] leading-5 text-[#5C544A]">ComfyUI가 아래 폴더의 모델 파일을 자동으로 인식합니다. 워크플로가 참조하는 파일 이름과 같아야 합니다.</p>
     <code className="mt-2 block break-all rounded-lg bg-[#FAF7F1] p-2 text-[11px] text-[#A94E30]">{prettyPath((video.value.config.comfyDir||'~/ComfyUI').replace(/\\+$/,'')+'/models')}</code>
     {(video.value.models||[]).length?<div className="mt-3 grid gap-2">{video.value.models!.map(model=><div key={model.type+model.name} className="flex flex-wrap items-center justify-between gap-2 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] px-3 py-2"><div className="min-w-0"><strong className="block break-all text-[11px] text-[#3A342D]">{model.name}</strong><small className="text-[10px] text-[#8A8176]">{model.type} · {model.present?'있음':'없음'}{model.present&&model.sizeBytes?' · '+fmtBytes(model.sizeBytes):''}</small></div>{model.present?<Check size={15} className="text-emerald-600"/>:model.download?<a className="inline-flex items-center gap-1 text-[11px] font-semibold text-[#C96442] hover:underline" href={model.download} target="_blank" rel="noreferrer">다운로드 <ExternalLink size={12}/></a>:<span className="text-[10px] text-amber-700">파일 필요</span>}</div>)}</div>:<p className="mt-3 text-[11px] text-[#8A8176]">워크플로를 추가하면 필요한 모델 파일 목록이 여기에 표시됩니다.</p>}
     <p className="mt-2 text-[10px] leading-4 text-[#8A8176]">모델 파일 합계 {fmtBytes(modelTotal)} · Python·PyTorch가 포함된 ComfyUI 설치 폴더는 모델 외에 약 5GB를 더 사용합니다. 앱에는 포함되지 않으며 이 PC에만 있습니다.</p>
     <div className="mt-4 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3">
     <div className="flex items-center justify-between gap-2"><strong className="text-[12px] text-[#3A342D]">YouTube 연결</strong><StateTag online={gReady}>{gReady?'앱 설정됨':'앱 설정 필요'}</StateTag></div>
     <p className="mt-1 text-[10px] leading-4 text-[#8A8176]">Google Cloud OAuth 클라이언트의 <b>승인된 리디렉션 URI</b>에 아래 주소를 그대로 등록하세요. 화면 주소가 아니라 인증 전용 콜백 주소입니다.</p>{gCallback&&<div className="mt-1 flex items-center gap-2 rounded border border-[#E7E0D5] bg-[#FFFDF9] p-2"><code className="min-w-0 flex-1 break-all text-[10px] text-[#3A342D]">{gCallback}</code><Button variant="ghost" className="!px-2 !py-0.5 !text-[10px]" onClick={() => { navigator.clipboard?.writeText(gCallback); setError("콜백 주소를 복사했습니다."); }}>복사</Button></div>}
     <div className="mt-2 grid gap-2 sm:grid-cols-2"><TextInput value={gid} onChange={e=>setGid(e.target.value)} placeholder="Client ID"/><TextInput type="password" value={gsecret} onChange={e=>setGsecret(e.target.value)} placeholder="Client Secret"/></div>
     <div className="mt-2 flex flex-wrap gap-2"><Button variant="secondary" disabled={gBusy||!gid.trim()||!gsecret.trim()} onClick={()=>void saveGoogle()}>{gBusy?'저장 중':'OAuth 앱 저장'}</Button><Button variant="secondary" onClick={()=>void connectGoogle()}>계정 연결</Button></div>
    </div>
    <div className="mt-4 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3">
     <strong className="text-[12px] text-[#3A342D]">허깅페이스에서 영상 모델 찾기</strong>
     <div className="mt-2 flex gap-2"><TextInput value={hfq} onChange={e=>setHfq(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'){e.preventDefault();void searchHf()}}} placeholder="예: wan2.1, ltx-video"/><Button variant="secondary" className="shrink-0 whitespace-nowrap" disabled={hfBusy} onClick={()=>void searchHf()}>{hfBusy?'검색 중':'검색'}</Button></div>
     {hf.length>0&&<div className="mt-2 grid max-h-72 gap-1 overflow-y-auto">{hf.map(item=><div key={item.id} className="rounded-lg bg-[#FFFDF9] px-2 py-1.5 text-[11px] text-[#3A342D]"><button type="button" className="flex w-full items-center justify-between gap-2 text-left" onClick={()=>void openRepo(item.id)}><span className="min-w-0 truncate">{item.id}</span><span className="shrink-0 text-[10px] text-[#8A8176]">{item.pipeline||'video'} · ↓{item.downloads.toLocaleString()}</span></button>{hfOpen===item.id&&<div className="mt-1 grid gap-1 border-t border-[#EFE9DF] pt-1">{hfFilesBusy&&<span className="text-[10px] text-[#8A8176]">파일 목록 불러오는 중…</span>}{hfFiles.map(f=><div key={f.path} className="flex items-center justify-between gap-2"><span className="min-w-0 truncate text-[10px] text-[#5C544A]" title={f.path}>{f.path}</span><span className="shrink-0 text-[10px] text-[#8A8176]">{fmtBytes(f.size)}</span><Button variant="secondary" className="!px-2 !py-0.5 !text-[10px]" onClick={()=>void downloadFile(item.id,f.path)}>받기</Button></div>)}</div>}</div>)}</div>}
     {dlJob&&<div className="mt-2 rounded-lg border border-[#E7E0D5] bg-[#FFFDF9] p-2 text-[10px] text-[#5C544A]"><div className="flex justify-between"><span className="truncate">{dlJob.name}</span><span>{dlJob.status==='completed'?'완료':dlJob.status==='failed'?'실패':Math.round((dlJob.downloaded||0)/Math.max(1,dlJob.total||1)*100)+'%'}</span></div>{dlJob.status==='downloading'&&<div className="mt-1 h-1.5 overflow-hidden rounded-full bg-[#F3EEE5]"><div className="h-full bg-[#C96442]" style={{width:Math.max(2,Math.round((dlJob.downloaded||0)/Math.max(1,dlJob.total||1)*100))+'%'}}/></div>}{dlJob.error&&<p className="text-rose-600">{dlJob.error}</p>}</div>}
     {!hf.length&&!hfBusy&&<p className="mt-2 text-[10px] text-[#8A8176]">검색하면 영상 모델만 표시됩니다. 받은 모델은 ComfyUI/models 폴더에 넣으세요.</p>}
    </div>
    <div className="mt-3 flex flex-wrap gap-2"><label className="inline-flex cursor-pointer items-center gap-2 rounded-xl border border-[#E1D9CC] bg-[#FFFDF9] px-3 py-2 text-[12px] font-semibold text-[#3A342D] hover:bg-[#F7F2EA]">
          모델 파일 열기
          <input type="file" className="hidden" multiple accept=".safetensors,.gguf,.ckpt,.pt,.bin" onChange={async(e)=>{
            const files=Array.from(e.target.files||[]);
            for(const file of files){
              try{
                const up=await fetch('/api/upload?name='+encodeURIComponent(file.name),{method:'POST',body:file}).then(r=>r.json());
                await post('/api/video/models/import',{source:up.path});
              }catch(err){setError((err as Error).message)}
            }
            await video.reload();
          }}/>
        </label><Button variant="ghost" onClick={()=>void video.reload()}>다시 확인</Button></div>
    </Panel>}</>}

   </div>
   {tab==='create'&&<div className="grid min-w-0 content-start gap-4">
    <Panel className="min-w-0 p-5"><Subheading title="생성 결과" extra={videoJob?<StateTag online={videoJob.status==='completed'}>{statusLabel(videoJob.status)}</StateTag>:<span className="text-[11px] text-[#8A8176]">대기</span>}/>{!videoJob?<div className="flex w-full flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-[#D8CFC2] bg-[#FAF7F1] text-[12px] text-[#8A8176]" style={{aspectRatio:ratios[ratio].width+" / "+ratios[ratio].height,maxHeight:420}}><Clapperboard size={24}/>영상을 생성하면 여기에 표시됩니다. <small className="text-[10px]">{ratios[ratio].label} · {ratios[ratio].width}×{ratios[ratio].height}</small></div>:<div className="grid gap-3">{!terminal(videoJob.status)&&<><div className="flex justify-between text-[11px] text-[#7C7469]"><span>{videoJob.status==='queued'?'ComfyUI 대기 중':videoJob.status==='downloading'?'결과 저장 중':'생성 중'}</span><span>{videoJob.progress}%</span></div><div className="h-2 overflow-hidden rounded-full bg-[#F3EEE5]"><div className="h-full bg-[#C96442]" style={{width:Math.max(4,videoJob.progress)+'%'}}/></div></>}{videoJob.result&&<div className="flex justify-center rounded-xl bg-black p-2"><video className="max-h-[520px] w-auto max-w-full rounded-lg" src={videoJob.result.url} controls playsInline/></div>}{videoJob.status==='completed'&&videoJob.result&&<div className="grid gap-2"><div className="flex min-w-0 items-center gap-2 rounded bg-[#FAF7F1] p-2"><span className="min-w-0 flex-1 truncate text-[11px] text-[#5C544A]" title={videoJob.result.file}>{videoJob.result.filename}</span><Button variant="ghost" className="!px-2 !py-1 !text-[10px]" onClick={()=>void reveal(videoJob.result!.file)}>폴더 열기</Button></div><Action onClick={useResult} className="w-full"><Upload size={14}/> 업로드 폼에 채우기</Action></div>}{videoJob.status==='failed'&&<Notice message={videoJob.error||'영상 생성에 실패했습니다.'}/>}</div>}</Panel>
    <Panel className="min-w-0 p-5"><Subheading title="YouTube 업로드" extra={<span className="text-[11px] text-[#8A8176]">{uploadFile?'파일 준비됨':'파일 선택'}</span>}/><div className="grid gap-3"><FormField label="영상 파일" hint="출력 폴더에서 자동으로 찾은 영상입니다"><div className="flex flex-col gap-2">{outputs.length?<SelectInput value={uploadFile} onChange={e=>{setUploadFile(e.target.value);const picked=outputs.find(o=>o.file===e.target.value);if(picked&&!uploadTitle)setUploadTitle(picked.name.replace(/\.[^.]+$/,''))}}>{outputs.map(o=><option key={o.file} value={o.file}>{o.name} · {fmtBytes(o.bytes)}</option>)}</SelectInput>:<p className="text-[11px] text-[#8A8176]">아직 생성된 영상이 없습니다. 아래에서 직접 선택할 수 있습니다.</p>}<label className="inline-flex cursor-pointer items-center justify-center gap-2 rounded-xl border border-[#E1D9CC] bg-[#FFFDF9] px-3 py-2 text-[12px] font-semibold text-[#3A342D] hover:bg-[#F7F2EA]">
                파일 열기
                <input
                  type="file"
                  className="hidden"
                  accept="video/*"
                  onChange={async (e) => {
                    const file = e.target.files?.[0];
                    if (!file) return;
                    try {
                      const response = await fetch("/api/upload?name=" + encodeURIComponent(file.name), { method: "POST", body: file });
                      const data = await response.json();
                      if (data.path) {
                        setUploadFile(data.path);
                        if (!uploadTitle) setUploadTitle(file.name.replace(/\.[^.]+$/, ""));
                      }
                    } catch (err) {
                      setError((err as Error).message);
                    }
                  }}
                />
              </label></div></FormField><FormField label="제목"><TextInput value={uploadTitle} onChange={e=>setUploadTitle(e.target.value)} placeholder="영상 제목" maxLength={100}/></FormField><FormField label="설명"><TextArea rows={3} value={uploadDescription} onChange={e=>setUploadDescription(e.target.value)} placeholder="설명"/></FormField><div className="grid grid-cols-2 gap-3"><FormField label="공개"><SelectInput value={uploadPrivacy} onChange={e=>setUploadPrivacy(e.target.value)}><option value="private">비공개</option><option value="unlisted">일부 공개</option><option value="public">공개</option></SelectInput></FormField><FormField label="태그"><TextInput value={uploadTags} onChange={e=>setUploadTags(e.target.value)} placeholder="쉼표 구분"/></FormField></div><Action disabled={uploading||!uploadFile.trim()||!uploadTitle.trim()} onClick={()=>void startUpload()} className="w-full">{uploading?<><LoaderCircle size={15} className="animate-spin"/> 요청 중</>:<><Upload size={15}/> YouTube 업로드</>}</Action>{uploadJob&&<div className="rounded-lg border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-[11px]"><div className="flex justify-between"><span>{uploadJob.status}</span><span>{uploadJob.progress}%</span></div>{uploadJob.result?.url&&<a className="mt-1 inline-flex items-center gap-1 font-semibold text-[#C96442]" href={uploadJob.result.url} target="_blank" rel="noreferrer">YouTube에서 열기 <ExternalLink size={12}/></a>}{uploadJob.error&&<p className="mt-1 text-rose-600">{uploadJob.error}</p>}</div>}</div></Panel>
   </div>}
  </div>
 </div>
}
