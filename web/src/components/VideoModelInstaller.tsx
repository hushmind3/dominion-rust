import {useEffect,useState} from 'react'
import {CheckCircle2,Download,FolderOpen,HardDrive,LoaderCircle,PauseCircle,RefreshCw,ShieldCheck,TriangleAlert,Video} from 'lucide-react'
import {Button,TextInput} from './Primitives'
import {Notice,Panel,useRemote} from './UI'
import {post} from '../api'

type ModelAsset={
 name:string;folder:string;installed:boolean;
 expectedBytes:number;downloadedBytes:number
}
type Pack={
 id:string;name:string;label:string;origin:string;
 estimatedBytes:number;modelsDir?:string|null;installed:boolean;configured:boolean;
 state:string;currentFile:string;error:string;
 downloadedBytes:number;totalBytes:number;files:ModelAsset[];
 needsComfyRestart:boolean
}
function fmt(bytes:number){
 if(!bytes||bytes<0)return '0 GB'
 return (bytes/1_000_000_000).toFixed(2)+' GB'
}
export function VideoModelInstaller({onInstalled}:{onInstalled:()=>void}){
 const remote=useRemote<Pack>('/api/video/model-packs',{
  id:'wan22-ti2v-5b',name:'Wan 2.2 TI2V 5B',
  label:'8GB VRAM 오프로딩 영상 생성 모델',
  origin:'Comfy-Org',estimatedBytes:16_890_000_000,
  installed:false,configured:false,state:'idle',currentFile:'',error:'',
  downloadedBytes:0,totalBytes:16_890_000_000,files:[],needsComfyRestart:false
 },false)
 const [folder,setFolder]=useState('')
 const [showPath,setShowPath]=useState(false)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [message,setMessage]=useState('')
 const downloading=remote.value.state==='downloading'
 const installed=remote.value.installed&&remote.value.configured
 const pct=Math.max(0,Math.min(100,Math.round(100*(remote.value.downloadedBytes||0)/(remote.value.totalBytes||1))))
 useEffect(()=>{
  if(remote.value.modelsDir&&!folder)setFolder(remote.value.modelsDir)
 },[remote.value.modelsDir,folder])
 useEffect(()=>{
  if(!downloading)return
  const interval=window.setInterval(()=>void remote.reload(),1500)
  return()=>window.clearInterval(interval)
 },[downloading,remote.reload])
 useEffect(()=>{
  if(remote.value.state==='installed'&&remote.value.configured)onInstalled()
 },[remote.value.state,remote.value.configured,onInstalled])
 const install=async()=>{
  if(busy)return
  setBusy(true);setError('');setMessage('')
  try{
   const response=await post<{modelsDir?:string}>('/api/video/model-packs/install',{
    ...(showPath||!remote.value.modelsDir?{modelsDir:folder.trim()}: {})
   })
   if(response.modelsDir)setFolder(response.modelsDir)
   setMessage('공식 모델 3개를 설치하고 검증합니다. 중단 후 재개할 수 있습니다.')
   await remote.reload()
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const pause=async()=>{
  setBusy(true);setError('')
  try{await post('/api/video/model-packs/cancel',{});setMessage('중단 요청됨. 받은 부분은 이어받기에 사용됩니다.')}
  catch(e){setError((e as Error).message)}
  finally{setBusy(false);await remote.reload()}
 }
 return <Panel className="overflow-hidden">
  <div className="flex flex-wrap items-start justify-between gap-3 border-b border-[#ECE4D9] bg-[#FCF9F4] px-4 py-4 sm:px-5">
   <div className="flex items-center gap-3">
    <div className="flex size-10 items-center justify-center rounded-xl bg-[#F3E5D8] text-[#B57152]"><Video size={19}/></div>
    <div>
     <strong className="block text-[13px] text-[#3E352E]">AI 영상 모델 · 바로 설치</strong>
     <small className="mt-1 block text-[11px] text-[#938376]">Wan 2.2 TI2V 5B · ComfyUI</small>
    </div>
   </div>
   <span className={'rounded-full border px-2.5 py-1 text-[10px] font-bold '+(
    installed?'border-emerald-200 bg-emerald-50 text-emerald-700':
    downloading?'border-sky-200 bg-sky-50 text-sky-700':'border-[#E4D3C2] bg-white text-[#97725A]'
   )}>{installed?'설치 완료':downloading?'다운로드 중':remote.value.state==='paused'?'일시 중단':'설치 필요'}</span>
  </div>
  <div className="space-y-4 p-4 sm:p-5">
   <p className="text-[11px] leading-5 text-[#827468]">모델·텍스트 인코더·VAE <strong>3개를 묶음 설치</strong>합니다. 공식 Hugging Face 파일을 직접 받아 SHA-256으로 검사하고 ComfyUI 기본 API 워크플로에 연결합니다.</p>
   <div className="grid grid-cols-3 gap-2">
    <div className="rounded-xl bg-[#F5F1EB] p-3 text-center"><strong className="block text-[13px] tabular-nums">5B</strong><small className="text-[10px] text-[#8C8075]">영상 모델</small></div>
    <div className="rounded-xl bg-[#F5F1EB] p-3 text-center"><strong className="block text-[13px] tabular-nums">8GB</strong><small className="text-[10px] text-[#8C8075]">VRAM 오프로딩</small></div>
    <div className="rounded-xl bg-[#F5F1EB] p-3 text-center"><strong className="block text-[13px] tabular-nums">약 17GB</strong><small className="text-[10px] text-[#8C8075]">모델 다운로드</small></div>
   </div>
   {downloading&&<div className="space-y-2" role="status">
    <div className="flex items-center justify-between gap-2 text-[11px] text-[#71675C]">
     <span className="min-w-0 truncate">{remote.value.currentFile||'다운로드 준비 중'}</span>
     <span className="shrink-0 font-semibold tabular-nums">{pct}%</span>
    </div>
    <div className="h-2.5 overflow-hidden rounded-full bg-[#EDE5DA]">
     <div className="h-full rounded-full bg-[#C77B58] transition-[width]" style={{width:pct+'%'}}/>
    </div>
    <div className="flex items-center justify-between text-[10px] tabular-nums text-[#9B8E81]">
     <span>{fmt(remote.value.downloadedBytes)} / {fmt(remote.value.totalBytes)}</span>
     <span>파일 검증 후 설치 확정</span>
    </div>
   </div>}
   {remote.value.files.length>0&&<div className="space-y-2">
    {remote.value.files.map(file=><div key={file.folder+file.name}
     className="flex min-w-0 items-center gap-2 rounded-lg bg-[#F9F7F3] px-3 py-2">
     {file.installed?<CheckCircle2 size={14} className="shrink-0 text-emerald-600"/>:
      <HardDrive size={14} className="shrink-0 text-[#A48D78]"/>}
     <span className="min-w-0 flex-1 truncate text-[10px] text-[#6C5F53]" title={file.name}>{file.name}</span>
     <span className="shrink-0 text-[10px] text-[#9B8D80]">{file.installed?'완료':fmt(file.expectedBytes)}</span>
    </div>)}
   </div>}
   {remote.value.modelsDir&&<p className="flex min-w-0 items-start gap-1.5 text-[10px] leading-5 text-[#958578]">
    <FolderOpen size={13} className="mt-0.5 shrink-0"/>
    <span className="break-all">모델 경로: {remote.value.modelsDir}</span>
   </p>}
   <button type="button" onClick={()=>setShowPath(!showPath)}
    className="inline-flex items-center gap-1.5 text-[11px] font-semibold text-[#B66E4C] hover:underline">
    <FolderOpen size={14}/>{showPath?'경로 입력 접기':'ComfyUI 모델 폴더 지정·변경'}
   </button>
   {(showPath||!remote.value.modelsDir)&&<label className="block space-y-1.5">
    <span className="text-[11px] font-semibold text-[#63574C]">ComfyUI/models의 절대 경로</span>
    <TextInput value={folder} onChange={e=>setFolder(e.target.value)} placeholder="예: C:\ComfyUI_windows_portable\ComfyUI\models"/>
    <span className="block text-[10px] leading-5 text-[#9B8B7D]">이미 설치된 ComfyUI의 models 폴더입니다. 기존 폴더를 선택하며, 다운로드는 이 경로에 저장됩니다.</span>
   </label>}
   {(remote.value.error||error)&&<Notice message={error||remote.value.error}/>}
   {message&&<p role="status" className="text-[11px] leading-5 text-emerald-700">{message}</p>}
   {installed?<div className="flex items-start gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-[11px] leading-5 text-emerald-800">
    <ShieldCheck size={16} className="mt-0.5 shrink-0"/>
    <span>모델 묶음과 기본 워크플로 연결 완료. <strong>ComfyUI가 설치 중 켜져 있었다면 한 번 재시작</strong>해 모델 파일을 인식시켜 주세요.</span>
   </div>:
   <div className="flex gap-2">
    <Button variant="primary" className="flex-1" disabled={busy||downloading||(!remote.value.modelsDir&&!folder.trim())}
     onClick={()=>void install()}>
     {busy?<LoaderCircle size={15} className="animate-spin"/>:<Download size={15}/>}
     {remote.value.state==='paused'?'이어서 다운로드':remote.value.state==='failed'?'설치 재시도':'영상 모델 묶음 설치'}
    </Button>
    {downloading&&<Button variant="secondary" disabled={busy} onClick={()=>void pause()}><PauseCircle size={15}/> 중단</Button>}
   </div>}
   <button type="button" disabled={busy} onClick={()=>void remote.reload()} className="inline-flex items-center gap-1.5 text-[10px] text-[#948478] hover:underline">
    <RefreshCw size={12}/> 설치 상태 새로고침
   </button>
   <div className="flex items-start gap-2 rounded-xl border border-[#EADAC7] bg-[#FFF8EF] p-3 text-[10px] leading-5 text-[#957253]">
    <TriangleAlert size={15} className="mt-0.5 shrink-0"/>
    <p>ComfyUI 서버는 별도로 설치·실행돼 있어야 합니다. 모델 저장공간은 최소 20GB 이상 권장하며, 다운로드 후 GPU 메모리에 맞춰 오프로딩됩니다. 이 모델은 동영상 생성용으로 음성·음악을 함께 만들지 않습니다.</p>
   </div>
   <a href="https://docs.comfy.org/tutorials/video/wan/wan2_2" target="_blank" rel="noopener noreferrer"
    className="text-[10px] font-semibold text-[#9A654C] underline underline-offset-2">ComfyUI 공식 Wan 2.2 설치 안내</a>
  </div>
 </Panel>
}
