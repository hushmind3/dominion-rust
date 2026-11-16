import {useEffect,useState} from 'react'
import {LoaderCircle,RotateCcw} from 'lucide-react'
import {post} from '../api'
import {Button,TextArea} from './Primitives'
import {Notice} from './UI'

type Shot={heading:string;visualPrompt:string;seconds:number}
export type RevisionProject={
 id:string;status:string;visualEngine:string;shots:Shot[];
 visualBible?:string;revisionCount?:number;revisionError?:string|null;
 regeneratingShot?:number|null;pendingRevisionJobId?:string|null;
 pendingRevisionShot?:number|null;pendingRevisionPrompt?:string;
 artifactRevisionId?:string|null;
}

/** Shot revisions preserve the prior verified MP4 until all new clips pass QA.
 * The Rust production journal remains authoritative for every transition. */
export function VideoShotRevision({job,onRefresh}:{
 job:RevisionProject;onRefresh:()=>Promise<unknown>
}){
 const [selected,setSelected]=useState<number|null>(null)
 const [draft,setDraft]=useState('')
 const [sending,setSending]=useState(false)
 const [error,setError]=useState('')
 const [message,setMessage]=useState('')
 useEffect(()=>{setSelected(null);setError('');setMessage('')},[job.id])
 const editing=selected!==null&&job.status==='ready'
 const canEdit=job.visualEngine==='comfyui'&&job.status==='ready'&&job.regeneratingShot==null
 const submit=async()=>{
  if(!canEdit||selected===null||!draft.trim()||sending)return
  setSending(true);setError('');setMessage('')
  try{
   await post('/api/video/productions/'+encodeURIComponent(job.id)+
    '/shots/'+selected+'/regenerate',{visualPrompt:draft.trim()})
   setSelected(null)
   setMessage('선택 장면을 재생성합니다. 기존 MP4는 새 결과가 검증될 때까지 유지됩니다.')
   await onRefresh()
  }catch(e){setError((e as Error).message)}
  finally{setSending(false)}
 }
 return <div className="mt-3 space-y-3" aria-label="장면별 재생성">
  {!!job.visualBible&&<details className="rounded-xl bg-white px-3 py-2 text-[11px] leading-5 text-[#756A5C]">
   <summary className="cursor-pointer font-semibold text-[#69543F]">공통 비주얼 설정 · 장면 연속성 기준</summary>
   <p className="mt-2 whitespace-pre-wrap">{job.visualBible}</p>
  </details>}
  {job.visualEngine==='comfyui'&&job.status==='ready'&&<div className="space-y-2">
   <p className="text-[11px] font-semibold text-[#74685C]">개별 장면 재생성 · 기존 장면은 재사용</p>
   <div className="flex flex-wrap gap-2">
    {job.shots.map((shot,index)=><button key={index} type="button"
     disabled={!canEdit||sending} aria-pressed={selected===index}
     onClick={()=>{setSelected(index);setDraft(job.pendingRevisionShot===index&&job.pendingRevisionPrompt?job.pendingRevisionPrompt:shot.visualPrompt);setError('')}}
     className={'inline-flex items-center gap-1 rounded-lg border px-3 py-2 text-[11px] font-semibold disabled:opacity-40 '+
      (selected===index?'border-[#C78362] bg-[#FFF1E8] text-[#9D5539]':'border-[#DDCBBB] bg-white text-[#86674F]')}>
     <RotateCcw size={12}/>{index+1}번 장면
    </button>)}
   </div>
  </div>}
  {editing&&<div className="space-y-2 rounded-xl border border-[#D6B9A6] bg-white p-3">
   <strong className="text-[12px]">{selected+1}번 장면 프롬프트</strong>
   <TextArea rows={4} maxLength={1400} value={draft}
    onChange={e=>setDraft(e.target.value)} aria-label="재생성할 장면 설명"
    className="!text-[12px]"/>
   <p className="text-[10px] leading-5 text-[#918274]">선택한 1개 장면만 GPU로 다시 생성합니다. 검증에 성공하면 최종 영상을 새로운 버전으로 조립합니다. GPU 비용은 별도입니다.</p>
   <div className="flex gap-2">
    <Button size="sm" variant="primary" onClick={()=>void submit()} disabled={!canEdit||!draft.trim()||sending}>
     {sending?<LoaderCircle size={13} className="animate-spin"/>:<RotateCcw size={13}/>}
     {sending?'제출 중':'장면만 다시 제작'}
    </Button>
    <Button size="sm" onClick={()=>setSelected(null)}>취소</Button>
   </div>
  </div>}
  {job.regeneratingShot!=null&&<div role="status" aria-live="polite"
   className="rounded-xl border border-[#D6B9A6] bg-[#FFF1E8] px-3 py-2 text-[11px] font-semibold text-[#9B6044]">
   <LoaderCircle size={13} className="mr-2 inline animate-spin"/>
   {job.regeneratingShot+1}번 장면 재생성 중 · 기존 MP4 보존
  </div>}
  {!!job.revisionError&&<Notice message={job.revisionError}/>}
  {!!error&&<Notice message={error}/>}
  {!!message&&<p role="status" className="text-[11px] text-[#776A5D]">{message}</p>}
  {job.visualEngine==='comfyui'&&<p className="text-[10px] text-[#A08D7D]">저장된 영상 수정 {job.revisionCount||0}회</p>}
 </div>
}
