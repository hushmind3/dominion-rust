import {useEffect,useRef,useState} from 'react'
import {CheckCircle2,Download,FolderOpen,HardDrive,LoaderCircle,PauseCircle,RefreshCw,Video,ShieldCheck} from 'lucide-react'
import {Button,TextInput} from './Primitives'
import {Notice,Panel,useRemote} from './UI'
import {post} from '../api'

type Variant={
 id:string;label:string;name:string;format:'gguf'|'safetensors';
 modelBytes:number;downloadBytes:number;installed:boolean;
 requiresExtension:boolean;source:string
}
type Pack={
 id:string;name:string;kind:string;variants:Variant[];selectedVariant:string;
 modelsDir?:string|null;installed:boolean;configured:boolean;
 state:string;currentFile:string;error:string;
 downloadedBytes:number;totalBytes:number;files:{
 name:string;folder:string;installed:boolean;expectedBytes:number;downloadedBytes:number
 }[];
 requiresExtension:boolean;estimatedBytes:number
}
function gb(n:number){return (n/1_000_000_000).toFixed(2)+'GB'}
const initial:Pack={id:'wan22-ti2v-5b',name:'Wan 2.2 TI2V 5B',kind:'video',
 variants:[],selectedVariant:'q4_k_m',installed:false,configured:false,state:'idle',
 currentFile:'',error:'',downloadedBytes:0,totalBytes:0,files:[],
 requiresExtension:true,estimatedBytes:0}
export function VideoModelInstaller({onInstalled}:{onInstalled:()=>void}){
 const remote=useRemote<Pack>('/api/video/model-packs',initial,false)
 const [selected,setSelected]=useState('q4_k_m')
 const [advanced,setAdvanced]=useState(false)
 const [folder,setFolder]=useState('')
 const [changedFolder,setChangedFolder]=useState(false)
 const [busy,setBusy]=useState(false)
 const [message,setMessage]=useState('')
 const [error,setError]=useState('')
 const notified=useRef('')
 const pack=remote.value
 const downloading=pack.state==='downloading'
 const chosen=pack.variants.find(v=>v.id===selected)
 const ready=Boolean(chosen?.installed&&pack.configured&&pack.selectedVariant===selected)
 const pct=Math.max(0,Math.min(100,Math.round(100*(pack.downloadedBytes||0)/(pack.totalBytes||1))))
 useEffect(()=>{
  if(pack.modelsDir&&!folder)setFolder(pack.modelsDir)
 },[pack.modelsDir,folder])
 useEffect(()=>{
  if(!downloading)return
  const timer=window.setInterval(()=>void remote.reload(),1200)
  return()=>window.clearInterval(timer)
 },[downloading,remote.reload])
 useEffect(()=>{
  if(pack.state==='installed'&&pack.configured&&notified.current!==pack.selectedVariant){
   notified.current=pack.selectedVariant
   onInstalled()
  }
 },[pack.state,pack.configured,pack.selectedVariant,onInstalled])
 const install=async()=>{
  if(busy||downloading)return
  setBusy(true);setError('');setMessage('')
  try{
   await post('/api/video/model-packs/install',{
    variant:selected,...(changedFolder&&folder.trim()?{modelsDir:folder.trim()}: {})
   })
   setMessage('선택한 영상 모델을 받고 있습니다. 공통 VAE·인코더는 다시 받지 않습니다.')
   await remote.reload()
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 const pause=async()=>{
  setBusy(true);setError('')
  try{
   await post('/api/video/model-packs/cancel',{})
   setMessage('중단 요청을 보냈습니다. 이어받기를 지원합니다.')
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false);await remote.reload()}
 }
 const packTotal=chosen?.downloadBytes||0
 const extra=chosen?.modelBytes||0
 return <Panel className="overflow-hidden">
  <div className="flex flex-wrap items-center justify-between gap-3 border-b border-[#EEE5DA] bg-[#FBF8F2] px-4 py-3 sm:px-5">
   <div className="flex items-center gap-2.5">
    <span className="flex size-9 items-center justify-center rounded-xl bg-[#F4E5D5] text-[#B67355]"><Video size={19}/></span>
    <div><h3 className="text-[13px] font-bold text-[#3F342C]">영상 모델 라이브러리</h3>
     <p className="mt-0.5 text-[10px] text-[#9A8979]">영상 생성 전용 · 용량별 양자화 선택</p>
    </div>
   </div>
   <span className="rounded-full border border-[#E4D8C8] bg-white px-2.5 py-1 text-[10px] font-semibold text-[#926D52]">
    {downloading?'다운로드 중':ready?'선택 모델 준비됨':'영상 모델 선택'}</span>
  </div>
  <div className="space-y-4 p-4 sm:p-5">
   <p className="text-[11px] leading-5 text-[#85776B]">
    <strong className="text-[#514238]">Wan 2.2 TI2V 5B</strong> · 텍스트로 영상 생성. 원하는 용량만 선택하면 필요한 파일을 함께 내려받습니다.
   </p>
   <div className="grid grid-cols-2 gap-2 sm:grid-cols-3" role="group" aria-label="영상 모델 양자화 선택">
    {pack.variants.map(v=><button key={v.id} type="button" disabled={downloading}
     aria-pressed={selected===v.id} onClick={()=>{setSelected(v.id);setMessage('');setError('')}}
     className={'min-w-0 rounded-xl border-2 p-3 text-left transition-colors '+(
      selected===v.id?'border-[#C7805E] bg-[#FFF5ED]':'border-[#EEE7DF] bg-white hover:border-[#D9BCA7]'
     )}>
     <div className="flex items-center justify-between gap-1">
      <strong className="text-[12px] text-[#3C342D]">{v.label}</strong>
      {v.installed&&<CheckCircle2 size={15} className="shrink-0 text-emerald-600"/>}
     </div>
     <p className="mt-1 text-[16px] font-bold tabular-nums text-[#B36F4F]">{gb(v.modelBytes)}</p>
     <p className="mt-0.5 text-[10px] text-[#958679]">{v.format==='gguf'?'GGUF 양자화':'FP16 원본'}</p>
    </button>)}
   </div>
   {chosen&&<div className="flex flex-wrap items-start justify-between gap-2 rounded-xl bg-[#F8F5F0] px-3 py-3 text-[11px]">
    <div className="flex items-start gap-2">
     <HardDrive size={16} className="mt-0.5 shrink-0 text-[#987F68]"/>
     <span><strong>{chosen.label} 선택</strong>
      <small className="mt-1 block text-[10px] leading-5 text-[#928272]">모델 본체 {gb(extra)} · 최초 전체 파일 약 {gb(packTotal)}</small>
     </span>
    </div>
    <span className="text-[10px] text-[#A18973]">{chosen.installed?'다운로드됨':'다운로드 가능'}</span>
   </div>}
   {downloading&&<div role="status" className="space-y-2">
    <div className="flex justify-between gap-2 text-[11px] text-[#796959]">
     <span className="min-w-0 truncate">{pack.currentFile||'파일 준비 중'}</span><b>{pct}%</b>
    </div>
    <div className="h-2.5 overflow-hidden rounded-full bg-[#ECE4D9]">
     <div className="h-full rounded-full bg-[#C57B5C] transition-[width]" style={{width:pct+'%'}}/>
    </div>
    <p className="text-[10px] tabular-nums text-[#98887A]">{gb(pack.downloadedBytes)} / {gb(pack.totalBytes)}</p>
   </div>}
   {downloading&&pack.files.length>0&&<div className="space-y-1 text-[10px] text-[#907F72]">
    {pack.files.map(file=><p key={file.folder+file.name} className="flex gap-2">
     {file.installed?<CheckCircle2 size={13} className="text-emerald-600"/>:<HardDrive size={13}/>}
     <span className="truncate">{file.name}</span>
    </p>)}
   </div>}
   <div className="flex flex-wrap gap-2">
    <Button variant="primary" className="min-w-0 flex-1" disabled={busy||downloading||!chosen}
     onClick={()=>void install()}>
     {busy?<LoaderCircle size={16} className="animate-spin"/>:<Download size={16}/>}
     {ready?'모델 다시 검사':chosen?.installed?'이 모델 사용하기':
      pack.state==='paused'&&selected===pack.selectedVariant?'이어서 받기':'선택한 영상 모델 다운로드'}
    </Button>
    {downloading&&<Button variant="secondary" disabled={busy} onClick={()=>void pause()}>
     <PauseCircle size={15}/> 중단
    </Button>}
    <Button variant="secondary" disabled={busy} onClick={()=>void remote.reload()}>
     <RefreshCw size={15}/> <span className="sr-only">새로고침</span>
    </Button>
   </div>
   {chosen?.requiresExtension&&<p className="flex items-start gap-2 text-[10px] leading-5 text-[#987354]">
    <ShieldCheck size={14} className="mt-0.5 shrink-0"/> GGUF 양자화 영상은 ComfyUI-GGUF 로더가 필요합니다.
    모델은 다운로드 가능하며, 생성 준비 상태는 로더 설치 여부까지 검사합니다.
   </p>}
   {(pack.error||error)&&<Notice message={error||pack.error}/>}
   {message&&<p role="status" className="text-[11px] text-emerald-700">{message}</p>}
   <details open={advanced} onToggle={e=>setAdvanced(e.currentTarget.open)}
    className="rounded-xl border border-[#EFE8E0] p-3 text-[11px]">
    <summary className="cursor-pointer font-semibold text-[#947963]">고급 · 모델 저장 위치 변경</summary>
    <div className="mt-3 space-y-2">
     <label htmlFor="video-model-dir" className="block text-[10px] text-[#847366]">
      별도 ComfyUI/models 경로를 사용하는 경우에만 입력
     </label>
     <div className="flex items-center gap-2"><FolderOpen size={16}/>
      <TextInput id="video-model-dir" value={folder}
       onChange={e=>{setFolder(e.target.value);setChangedFolder(true)}}
       placeholder="예: C:\\ComfyUI\\models"/>
     </div>
     <p className="text-[10px] text-[#9A8B7F]">경로를 지정하지 않으면 도미니언이 모델 저장공간을 자동으로 만듭니다.</p>
    </div>
   </details>
  </div>
 </Panel>
}
