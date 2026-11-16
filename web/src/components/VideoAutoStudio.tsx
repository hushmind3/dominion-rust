import {useEffect,useState} from 'react'
import {
 ArrowRight,BookOpenCheck,CheckCircle2,Clapperboard,Clock3,CloudUpload,
 Download,ExternalLink,FileText,Film,Layers3,LoaderCircle,Play,
 RefreshCw,ShieldCheck,Sparkles,TriangleAlert,WandSparkles
} from 'lucide-react'
import {Button,SelectInput,TextArea,TextInput} from './Primitives'
import {FormField,Notice,Panel,useRemote} from './UI'
import {downloadApiVideo,getApiVideoPreview,post} from '../api'
import {VideoModelInstaller} from './VideoModelInstaller'
import {useJob} from '../hooks/useJob'

type Engine='native'|'comfyui'
type Ratio='9:16'|'16:9'|'1:1'
type Shot={heading:string;narration:string;visualPrompt:string;seconds:number}
type Production={
 id:string;topic:string;title:string;audience:string;description:string;
 status:string;stage:string;stageIndex:number;stageTotal:number;
 visualEngine:Engine;aspectRatio:Ratio;duration:number;autoPublish:boolean;
 privacyStatus:'private'|'unlisted'|'public';costKrw:number;
 sourceNotesProvided:boolean;factsVerified:boolean;
 script:string;shots:Shot[];tags:string[];
 videoId?:string|null;uploadId?:string|null;uploadUrl?:string|null;
 artifactBytes?:number|null;measuredSeconds?:number|null;
 error?:string;outputUrl?:string|null;createdAt?:string
}
const STAGES=[
 {id:'planning',name:'기획',detail:'주제와 제작 방향 정리',icon:Sparkles},
 {id:'storyboard',name:'대본·콘티',detail:'AI 장면과 내레이션 생성',icon:FileText},
 {id:'rendering',name:'영상 제작',detail:'실제 FFmpeg / ComfyUI 실행',icon:Film},
 {id:'quality_check',name:'품질 검사',detail:'코덱·길이·파일 재생성 확인',icon:BookOpenCheck},
 {id:'publishing',name:'게시',detail:'요청한 경우에만 YouTube 전송',icon:CloudUpload},
] as const
function remembered(){
 try{return sessionStorage.getItem('dominion.video.production')||''}catch{return ''}
}
function label(status:string){
 return ({running:'진행 중',ready:'제작 완료',published:'게시 완료',
  failed:'실패',needs_review:'확인 필요'} as Record<string,string>)[status]||status
}
function bytes(value?:number|null){
 if(!value)return '-'
 return value>1024*1024?(value/1024/1024).toFixed(1)+'MB':Math.round(value/1024)+'KB'
}
export function VideoAutoStudio({
 youtubeConnected,comfyReady,nativeReady,onSettings,onReview,onPublish,onModelInstalled
}:{
 youtubeConnected:boolean;comfyReady:boolean;nativeReady:boolean;
 onSettings:()=>void;onReview:(id:string)=>void;onPublish:(id:string)=>void;onModelInstalled:()=>void
}){
 const jobs=useRemote<Production[]>('/api/video/productions',[],false)
 const [chosen,setChosen]=useState(remembered)
 const active=useJob<Production>(chosen?'/api/video/productions/'+chosen:null,
  jobs.value.find(item=>item.id===chosen)||null)
 const current=active.value?.id===chosen?active.value:jobs.value.find(item=>item.id===chosen)
 const [topic,setTopic]=useState('')
 const [audience,setAudience]=useState('일반 시청자')
 const [notes,setNotes]=useState('')
 const [engine,setEngine]=useState<Engine>('comfyui')
 const [ratio,setRatio]=useState<Ratio>('9:16')
 const [duration,setDuration]=useState(6)
 const [narration,setNarration]=useState(true)
 const [autoPublish,setAutoPublish]=useState(false)
 const [privacy,setPrivacy]=useState<'private'|'unlisted'|'public'>('private')
 const [maxCost,setMaxCost]=useState(3000)
 const [sending,setSending]=useState(false)
 const [error,setError]=useState('')
 const [message,setMessage]=useState('')
 const [preview,setPreview]=useState('')
 const [previewError,setPreviewError]=useState('')
 const [previewBusy,setPreviewBusy]=useState(false)
 const [showScript,setShowScript]=useState(false)
 const producing=jobs.value.some(item=>item.status==='running')||current?.status==='running'
 const canRun=topic.trim().length>0&&!sending&&
  (engine==='native'?nativeReady:comfyReady)&&(!autoPublish||youtubeConnected)
 useEffect(()=>{
  if(!jobs.loading&&!chosen&&jobs.value[0])setChosen(jobs.value[0].id)
 },[jobs.loading,jobs.value,chosen])
 useEffect(()=>{
  try{if(chosen)sessionStorage.setItem('dominion.video.production',chosen)}catch{/* disabled */}
 },[chosen])
 useEffect(()=>{
  if(engine==='comfyui'&&duration>9)setDuration(6)
  if(engine==='native'&&duration<6)setDuration(30)
 },[engine,duration])
 useEffect(()=>{
  if(!producing)return
  const timer=window.setInterval(()=>void jobs.reload(),4000)
  return()=>window.clearInterval(timer)
 },[producing,jobs.reload])
 useEffect(()=>{
  if(!current?.outputUrl||!['ready','published'].includes(current.status)){
   setPreview('');setPreviewError('');setPreviewBusy(false);return
  }
  const controller=new AbortController()
  let url=''
  setPreview('');setPreviewError('');setPreviewBusy(true)
  void getApiVideoPreview(current.outputUrl,controller.signal).then(next=>{
   if(controller.signal.aborted){URL.revokeObjectURL(next);return}
   url=next;setPreview(next)
  }).catch((e:unknown)=>{
   if(!controller.signal.aborted)setPreviewError((e as Error).message)
  }).finally(()=>{if(!controller.signal.aborted)setPreviewBusy(false)})
  return()=>{controller.abort();if(url)URL.revokeObjectURL(url)}
 },[current?.videoId,current?.status,current?.outputUrl])
 const start=async()=>{
  if(!canRun)return
  setSending(true);setError('');setMessage('')
  try{
   const job=await post<Production>('/api/video/productions',{
    topic:topic.trim(),audience:audience.trim(),sourceNotes:notes.trim(),
    visualEngine:engine,aspectRatio:ratio,duration,
    narration:engine==='native'&&narration,
    autoPublish,privacyStatus:privacy,maxCostKrw:maxCost
   })
   setChosen(job.id)
   await jobs.reload()
   setMessage('자율 제작이 시작됐습니다. 단계별 진행 상황을 아래에서 확인할 수 있습니다.')
  }catch(e){setError((e as Error).message)}
  finally{setSending(false)}
 }
 const download=async()=>{
  if(!current?.videoId)return
  setError('')
  try{
   const result=await post<{downloadUrl?:string}>('/api/video/jobs/'+current.videoId+'/reveal',{})
   if(!result.downloadUrl)throw Error('영상 파일을 찾지 못했습니다.')
   await downloadApiVideo(result.downloadUrl,current.videoId)
  }catch(e){setError((e as Error).message)}
 }
 const done=current?.status==='ready'||current?.status==='published'
 const stageIndex=Math.max(0,STAGES.findIndex(s=>s.id===current?.stage))
 return <div className="grid items-start gap-5 2xl:grid-cols-[minmax(0,1.05fr)_minmax(390px,.95fr)]">
  <div className="space-y-5">
   <Panel className="overflow-hidden">
    <div className="border-b border-[#EAE3D9] bg-[#FDFBF7] px-5 py-6 sm:px-7">
     <span className="inline-flex items-center gap-1.5 rounded-full bg-[#F7E5D8] px-3 py-1.5 text-[10px] font-bold tracking-wide text-[#A86143]"><WandSparkles size={13}/> AUTONOMOUS PRODUCTION</span>
     <h2 className="mt-3 text-[23px] font-bold tracking-[-.045em] text-[#2F2B26]">주제 하나로, 완성 영상까지.</h2>
     <p className="mt-2 max-w-xl text-[12px] leading-6 text-[#8A7F74]">AI가 대본과 장면을 설계하고, 실제 렌더러가 영상 파일을 제작합니다. 완료 후 코덱·재생 길이를 검사하며 선택 시 YouTube 업로드까지 이어집니다.</p>
    </div>
    <div className="space-y-6 p-5 sm:p-7">
     <FormField label="무엇에 관한 영상을 제작할까요?">
      <TextArea rows={4} value={topic} maxLength={600}
       onChange={e=>setTopic(e.target.value)}
       placeholder="예: 100년 후 서울의 모습. 미래 도시를 여행하는 시네마틱 쇼츠를 만들어 주세요."
       className="!min-h-32 !text-[14px] !leading-6"/>
     </FormField>
     <div className="grid gap-4 sm:grid-cols-2">
      <FormField label="시청 대상 / 톤">
       <TextInput value={audience} onChange={e=>setAudience(e.target.value)} maxLength={200}
        placeholder="예: 20~40대 건축 관심층"/>
      </FormField>
      <FormField label="비디오 방식">
       <SelectInput value={engine} onChange={e=>setEngine(e.target.value as Engine)}>
        <option value="native">FFmpeg · 스토리 카드 + 음성</option>
        <option value="comfyui">ComfyUI · 실제 AI 장면 영상</option>
       </SelectInput>
      </FormField>
     </div>
     {engine==='comfyui'&&<VideoModelInstaller onInstalled={onModelInstalled}/>}
     <div className="space-y-3">
      <p className="text-[12px] font-semibold text-[#40372F]">화면 비율</p>
      <div className="grid grid-cols-3 gap-2">
       {([
        ['9:16','세로 쇼츠','Shorts · Reels'],
        ['16:9','가로 영상','YouTube'],
        ['1:1','정사각형','Feed']
       ] as const).map(([id,name,detail])=><button key={id} type="button" aria-pressed={ratio===id}
        onClick={()=>setRatio(id)}
        className={'rounded-2xl border-2 px-2 py-3 text-center transition-colors '+(ratio===id?'border-[#C67B5B] bg-[#FFF4EA] text-[#A75F43]':'border-[#E6DED2] bg-[#FBFAF7] text-[#796F63] hover:border-[#D3B9A8]')}>
        <strong className="block text-[13px]">{name}</strong>
        <small className="mt-1 block text-[10px] opacity-75">{detail}</small>
       </button>)}
      </div>
     </div>
     <div className="grid gap-4 sm:grid-cols-2">
      <FormField label="영상 길이">
       <SelectInput value={duration} onChange={e=>setDuration(Number(e.target.value))}>
        {(engine==='native'?[15,30,45,60,90,120,180]:[3,6,9]).map(sec=>
         <option key={sec} value={sec}>{sec}초</option>)}
       </SelectInput>
      </FormField>
      <FormField label="대본·모델 API 예상 비용 상한 (원)">
       <TextInput type="number" min={0} max={1000000} value={maxCost}
        onChange={e=>setMaxCost(Number(e.target.value))}/>
      </FormField>
     </div>
     <details className="group rounded-2xl border border-[#E8E1D5] bg-[#FAF8F4] p-4">
      <summary className="cursor-pointer text-[12px] font-semibold text-[#5F554C]">참고 자료 · 사실성 가이드 (선택)</summary>
      <p className="mt-3 text-[11px] leading-5 text-[#8A7F74]">제작 과정에서 실제 웹 조사를 수행했다고 주장하지 않습니다. 정보성 콘텐츠는 검토한 자료를 입력하면 AI가 이를 대본 작성의 근거로 사용합니다.</p>
      <TextArea rows={5} value={notes} maxLength={8000} onChange={e=>setNotes(e.target.value)}
       className="mt-3 !text-[12px]" placeholder="확인된 사실, 출처 요약, 참고할 콘셉트 등을 붙여넣어 주세요."/>
     </details>
     {engine==='native'&&<label className="flex cursor-pointer items-center justify-between rounded-xl border border-[#EEE4D8] bg-[#FAF8F4] px-4 py-3 text-[12px]">
      <span><strong className="block">자동 내레이션</strong><small className="mt-1 block text-[11px] text-[#94877A]">음성 엔진 설치 시 장면마다 합성</small></span>
      <input type="checkbox" checked={narration} onChange={e=>setNarration(e.target.checked)}
       className="size-5 accent-[#BF7656]"/>
     </label>}
     {engine==='comfyui'&&<div className="flex items-start gap-2 rounded-xl border border-[#E8DAC7] bg-[#FFF8ED] p-3 text-[11px] leading-5 text-[#8D6E4B]">
      <TriangleAlert size={16} className="mt-0.5 shrink-0"/>
      <p>ComfyUI는 연결된 GPU 모델로 3~9초의 실제 영상 클립을 생성합니다. 이 모드에 자동 내레이션 합성은 아직 연결되지 않았습니다. 긴 영상은 FFmpeg 카드 방식으로 제작합니다.</p>
     </div>}
     <div className="rounded-2xl border border-[#E9E1D6] bg-[#FCFAF7] p-4">
      <label className="flex cursor-pointer items-center justify-between gap-3">
       <span className="inline-flex items-center gap-2 text-[12px] font-bold"><CloudUpload size={17} className="text-[#BE7455]"/> 제작 완료 후 YouTube 자동 업로드</span>
       <input type="checkbox" className="size-5 accent-[#C77550]" checked={autoPublish} onChange={e=>setAutoPublish(e.target.checked)}/>
      </label>
      {autoPublish&&<div className="mt-4 space-y-3 border-t border-[#E8E0D5] pt-4">
       <FormField label="공개 범위 (기본 비공개)">
        <SelectInput value={privacy} onChange={e=>setPrivacy(e.target.value as typeof privacy)}>
         <option value="private">비공개 · 먼저 검토</option>
         <option value="unlisted">일부 공개 · 링크 전용</option>
         <option value="public">전체 공개 · 자동 게시</option>
        </SelectInput>
       </FormField>
       <p className="text-[11px] leading-5 text-[#8E8275]">{youtubeConnected
        ?'계정 연결됨. 파일 검증 성공 후 선택한 공개 범위로 자동 업로드합니다.'
        :'YouTube 계정 연결이 필요합니다. 설정에서 OAuth 로그인을 완료해 주세요.'}</p>
      </div>}
     </div>
     {((engine==='native'&&!nativeReady)||(engine==='comfyui'&&!comfyReady)||(autoPublish&&!youtubeConnected))&&
      <div className="flex flex-wrap items-center justify-between gap-3 rounded-xl bg-amber-50 p-3 text-[11px] text-amber-900">
       <span>{engine==='comfyui'&&!comfyReady?'ComfyUI 서버와 API 워크플로 연결이 필요합니다.':engine==='native'&&!nativeReady?'FFmpeg 설치 또는 실행 환경을 확인해 주세요.':'YouTube 계정을 연결해 주세요.'}</span>
       <Button size="sm" onClick={onSettings}>설정 열기 <ArrowRight size={12}/></Button>
      </div>}
     <Button variant="primary" disabled={!canRun}
      className="!min-h-14 !w-full !rounded-2xl !text-[14px] !shadow-[0_8px_24px_rgba(194,107,75,.2)]"
      onClick={()=>void start()}>
      {sending?<LoaderCircle className="animate-spin" size={19}/>:<WandSparkles size={19}/>}
      {sending?'제작 실행 중...':'기획부터 완성까지 자율 제작 시작'}<ArrowRight size={17} className="ml-auto"/>
     </Button>
     <p className="text-center text-[10px] leading-5 text-[#9A8E82]">새 작업은 별도 기록으로 저장되며, 재시작 후 불확실한 업로드를 중복 실행하지 않습니다.</p>
    </div>
   </Panel>
  </div>

  <div className="space-y-5">
   <Panel className="overflow-hidden">
    <div className="flex flex-wrap items-center justify-between gap-3 border-b border-[#ECE4D9] px-5 py-5 sm:px-6">
     <div><p className="text-[10px] font-semibold tracking-[.14em] text-[#B4785B]">LIVE PIPELINE</p>
      <h3 className="mt-1 text-[18px] font-bold text-[#37312B]">자율 제작 진행 상황</h3></div>
     <Button size="sm" disabled={jobs.loading} onClick={()=>void jobs.reload()}>
      <RefreshCw size={14}/> 새로고침
     </Button>
    </div>
    <div className="space-y-5 p-5 sm:p-6">
     {current?<div className="space-y-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
       <div className="min-w-0 flex-1">
        <p className="text-[13px] font-semibold leading-5 text-[#3C352D]">{current.title||current.topic}</p>
        <p className="mt-1 text-[11px] text-[#96897D]">{current.visualEngine==='comfyui'?'AI 장면 영상':'로컬 스토리 영상'} · {current.duration}초 · {current.aspectRatio}</p>
       </div>
       <span className={'rounded-full border px-3 py-1 text-[11px] font-bold '+(
        done?'border-emerald-200 bg-emerald-50 text-emerald-700':
        current.status==='failed'?'border-rose-200 bg-rose-50 text-rose-700':
        current.status==='needs_review'?'border-amber-200 bg-amber-50 text-amber-800':
        'border-sky-200 bg-sky-50 text-sky-700'
       )}>{label(current.status)}</span>
      </div>
      <ol className="space-y-2" aria-label="자율 제작 단계">
       {STAGES.map((stage,index)=>{
        const complete=done||index<stageIndex
        const active=current.status==='running'&&current.stage===stage.id
        const skipped=index===4&&!current.autoPublish
        return <li key={stage.id} className={'flex items-center gap-3 rounded-xl border p-3 '+(
         active?'border-[#D6A78D] bg-[#FFF5EC]':complete&&!skipped?'border-emerald-100 bg-emerald-50/50':'border-[#EEE9E3] bg-[#FBFAF8]'
        )}>
         <div className={'flex size-9 shrink-0 items-center justify-center rounded-lg '+(
          active?'bg-[#F7DECE] text-[#B66C50]':
          complete&&!skipped?'bg-[#E5F4E9] text-emerald-700':'bg-[#F0EDE8] text-[#A4998F]'
         )}>
          {complete&&!skipped?<CheckCircle2 size={17}/>:active?<LoaderCircle size={17} className="animate-spin"/>:<stage.icon size={17}/>}
         </div>
         <div className="min-w-0 flex-1"><strong className="text-[12px] text-[#51463C]">{stage.name}</strong>
          <p className="text-[10px] text-[#96897D]">{skipped?'자동 게시 선택 안 함':stage.detail}</p></div>
         <span className="text-[10px] text-[#A4988A]">{skipped?'생략':active?'진행':complete?'완료':String(index+1).padStart(2,'0')}</span>
        </li>
       })}
      </ol>
      {!!current.error&&<Notice message={current.error}/>}
      {!!current.shots.length&&<div className="rounded-xl bg-[#FAF7F1] p-4">
       <div className="flex items-center justify-between">
        <strong className="text-[12px] text-[#403830]">AI가 설계한 장면</strong>
        <span className="text-[10px] text-[#938577]">{current.shots.length}장면</span>
       </div>
       <div className="mt-3 space-y-2">
        {current.shots.map((shot,index)=><div key={index} className="flex items-start gap-3 border-t border-[#E9E1D7] pt-2">
         <span className="flex size-7 shrink-0 items-center justify-center rounded-lg bg-white text-[11px] font-bold text-[#C06F50]">{index+1}</span>
         <span className="min-w-0 flex-1"><strong className="block text-[11px]">{shot.heading}</strong>
          <small className="mt-1 block text-[10px] leading-5 text-[#86796D]">{shot.narration}</small></span>
         <span className="shrink-0 text-[10px] text-[#A18F81]">{shot.seconds}초</span>
        </div>)}
       </div>
       <button type="button" onClick={()=>setShowScript(!showScript)} className="mt-4 text-[11px] font-semibold text-[#B56C4E] hover:underline">
        {showScript?'대본 접기':'전체 내레이션 대본 보기'}
       </button>
       {showScript&&<pre className="mt-2 max-h-60 overflow-auto whitespace-pre-wrap rounded-xl bg-white p-3 font-sans text-[11px] leading-6 text-[#665A4F]">{current.script}</pre>}
      </div>}
      {done&&<div className="space-y-3">
       <div className="overflow-hidden rounded-2xl bg-[#253239] p-3">
        {preview?<video src={preview} controls playsInline preload="metadata"
         aria-label="완성된 자율 제작 영상"
         className="mx-auto max-h-[350px] w-auto max-w-full rounded-xl"/>:
         <div className="flex min-h-44 flex-col items-center justify-center gap-2 text-center text-[#B7C8C9]">
          {previewBusy?<LoaderCircle size={24} className="animate-spin"/>:<Clapperboard size={24}/>}
          <p className="text-[12px]">{previewBusy?'미리보기 불러오는 중':previewError||'영상 파일이 완성됐습니다. 다운로드로 확인할 수 있습니다.'}</p>
         </div>}
       </div>
       <div className="flex flex-wrap items-center justify-between gap-2 text-[11px] text-[#8B8075]">
        <span>검사 완료 · {bytes(current.artifactBytes)} · {current.measuredSeconds?.toFixed(1)||'-'}초</span>
        <span>모델 대본 비용 {Math.max(0,current.costKrw||0).toLocaleString()}원</span>
       </div>
       <div className="grid grid-cols-2 gap-2">
        <Button variant="secondary" onClick={()=>void download()} disabled={!current.videoId}><Download size={14}/> 영상 저장</Button>
        <Button variant="primary" onClick={()=>current.videoId&&onPublish(current.videoId)} disabled={!current.videoId||current.status==='published'}><CloudUpload size={14}/> 수동 게시</Button>
       </div>
       {!!current.uploadUrl&&<a href={current.uploadUrl} target="_blank" rel="noopener noreferrer"
        className="inline-flex items-center gap-1.5 text-[12px] font-semibold text-[#B96B4B] hover:underline">
        <ExternalLink size={14}/> 게시된 영상 열기
       </a>}
      </div>}
      {!current.factsVerified&&<div className="flex items-start gap-2 text-[10px] leading-5 text-[#93887E]">
       <ShieldCheck size={14} className="mt-0.5 shrink-0"/>
       <p>영상 파일 무결성과 재생 길이 검사는 실제 도구로 수행합니다. 대본의 사실관계는 별도 외부 검증을 완료한 상태가 아닙니다.</p>
      </div>}
     </div>:<div className="flex min-h-56 flex-col items-center justify-center gap-3 text-center">
      <div className="flex size-12 items-center justify-center rounded-2xl bg-[#F4EDE5] text-[#B57255]"><Layers3 size={23}/></div>
      <strong className="text-[13px]">실행 중인 자율 제작이 없습니다</strong>
      <p className="max-w-xs text-[11px] leading-5 text-[#968A7E]">왼쪽에서 주제를 입력하고 실행하면 AI 기획부터 실제 영상 렌더링까지 작업 단계가 나타납니다.</p>
     </div>}
    </div>
   </Panel>

   <Panel className="p-5 sm:p-6">
    <div className="mb-4 flex items-center justify-between">
     <div><p className="text-[10px] font-semibold tracking-wider text-[#B37B5C]">PROJECT HISTORY</p>
      <h3 className="mt-1 text-[16px] font-bold">자율 제작 기록</h3></div>
     <span className="text-[11px] text-[#95887D]">{jobs.value.length}건</span>
    </div>
    {jobs.value.length?<div className="max-h-80 space-y-2 overflow-auto pr-1">
     {jobs.value.map(item=><button key={item.id} type="button"
      onClick={()=>{setChosen(item.id);setError('');setMessage('')}}
      className={'flex w-full min-w-0 items-center gap-3 rounded-xl border p-3 text-left '+(chosen===item.id?'border-[#DAB397] bg-[#FFF7EF]':'border-[#E9E1D7] bg-[#FFFEFB] hover:bg-[#FAF6F1]')}>
      <span className="flex size-9 shrink-0 items-center justify-center rounded-xl bg-[#F4EBE1] text-[#B87555]">
       {item.status==='running'?<LoaderCircle size={17} className="animate-spin"/>:item.status==='ready'||item.status==='published'?<CheckCircle2 size={17}/>:<Clock3 size={17}/>}
      </span>
      <span className="min-w-0 flex-1"><strong className="block truncate text-[11px]">{item.title||item.topic}</strong>
       <small className="mt-1 block text-[10px] text-[#948679]">{item.duration}초 · {label(item.status)}</small></span>
      <ArrowRight size={14} className="shrink-0 text-[#AB9B8B]"/>
     </button>)}
    </div>:<p className="rounded-xl bg-[#F9F7F3] p-4 text-center text-[12px] text-[#928579]">제작 기록이 없습니다.</p>}
   </Panel>
   {message&&<div role="status" className="flex items-start gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-[12px] text-emerald-800"><CheckCircle2 size={15}/>{message}</div>}
   {error&&<Notice message={error}/>}
  </div>
 </div>
}
