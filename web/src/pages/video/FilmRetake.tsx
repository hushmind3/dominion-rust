import {useState} from 'react'
import {RefreshCw,RotateCcw} from 'lucide-react'
import {post} from '../../api'
import {Button,TextArea} from '../../components/Primitives'
import {Notice,useRemote} from '../../components/UI'

type Shot={index:number;duration:number;outputSha256?:string;isAvailable:boolean;
 receiptVerified:boolean;qualityStatus?:string;canRetake:boolean;expectedPaidCalls:number;
 expectedQuote?:{referenceKrw:number;fingerprint:string;pricingAsOf:string}}
type SceneManifest={clips?:Shot[];terminal?:boolean;unknownEffect?:boolean}
type RetakeResult={id:string;status:string;kind:'video';[key:string]:unknown}

/** One click creates a new job, not a hidden replay of the prior paid call. */
export function FilmRetake({jobId,onCreated}:{jobId:string;onCreated:(job:RetakeResult)=>void}){
 const data=useRemote<SceneManifest>('/api/video/jobs/'+encodeURIComponent(jobId)+'/scenes',{})
 const [selected,setSelected]=useState<number|null>(null)
 const [prompt,setPrompt]=useState('')
 const [accepted,setAccepted]=useState(false)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [result,setResult]=useState('')
 const eligible=(data.value.clips||[]).filter(clip=>clip.canRetake)
 const shot=eligible.find(clip=>clip.index===selected)??eligible[0]
 const retake=async()=>{
  if(!shot||!accepted||busy||!shot.outputSha256||!shot.expectedQuote)return
  const message='장면 '+(shot.index+1)+'을 다시 생성합니다. 새 유료 호출 '+shot.expectedPaidCalls+
   '회 · 약 '+shot.expectedQuote.referenceKrw.toLocaleString('ko-KR')+
   '원(참고 견적)이 승인 한도입니다. 정상 장면은 영수증 검증 후 재사용합니다. 계속할까요?'
  if(!window.confirm(message))return
  setBusy(true);setError('');setResult('')
  try{
   const job=await post<RetakeResult>('/api/video/jobs/'+encodeURIComponent(jobId)+'/retake',{
    index:shot.index,expectedSha256:shot.outputSha256,
    confirmedPaidCallCount:shot.expectedPaidCalls,
    confirmNewPaidCalls:true,
    approvedPaidQuote:true,approvedQuoteFingerprint:shot.expectedQuote.fingerprint,
    maxApprovedKrw:shot.expectedQuote.referenceKrw,
    ...(prompt.trim()?{prompt:prompt.trim()}:{})
   })
   setResult('새 영상 작업을 등록했습니다. 기존 성공 장면은 재구매하지 않습니다.')
   onCreated(job)
  }catch(e){setError((e as Error).message)}
  finally{setBusy(false);setAccepted(false)}
 }
 return <div className="rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3">
  <div className="flex items-center justify-between gap-2">
   <strong className="text-xs">장면별 영수증 · 부분 재생성</strong>
   <Button variant="ghost" size="sm" onClick={()=>void data.reload()}><RefreshCw size={13}/> 새로고침</Button>
  </div>
  <p className="mt-2 text-[11px] leading-5 text-[#7C7469]">영상의 기술 검수와 시각적 만족도는 별개입니다. 실패하거나 마음에 들지 않는 장면만 새 유료 작업으로 선택할 수 있습니다.</p>
  {data.error&&<Notice message={data.error}/>}
  {error&&<Notice message={error}/>}
  {result&&<p className="mt-2 text-xs text-emerald-700">{result}</p>}
  {(data.value.clips||[]).map(clip=><label key={clip.index}
   className="mt-2 flex cursor-pointer items-center gap-2 rounded border border-[#E7E0D5] bg-white p-2 text-[11px]">
   <input type="radio" name={'shot-'+jobId} disabled={!clip.canRetake}
    checked={shot?.index===clip.index} onChange={()=>{setSelected(clip.index);setAccepted(false)}}/>
   <span>장면 {clip.index+1} · {clip.duration}초
    <span className="ml-2 text-[#7C7469]">
     {clip.receiptVerified?'기존 검증 영수증':clip.isAvailable?'출력 있음·기술 검수 미완료':'아직 생성 안 됨'}
    </span>
    {clip.outputSha256&&<small className="block text-[10px] text-[#8A8176]">SHA-256: {clip.outputSha256.slice(0,20)}…</small>}
   </span>
  </label>)}
  {shot&&<div className="mt-3 grid gap-2">
   <span className="text-xs font-medium">선택 장면 {shot.index+1} · 예상 신규 유료 호출 {shot.expectedPaidCalls}회</span>
   <p className="text-[11px] text-[#6D6257]">예상 추가 비용 약 {shot.expectedQuote?.referenceKrw.toLocaleString('ko-KR')??'미확인'}원 · 가격 기준 {shot.expectedQuote?.pricingAsOf||'확인 필요'}</p>
   {shot.isAvailable&&<video controls playsInline preload="metadata" className="max-h-[360px] w-full rounded-lg bg-black" src={'/api/video/jobs/'+encodeURIComponent(jobId)+'/scenes/'+shot.index+'/content'}/>}
   <TextArea rows={2} value={prompt} onChange={e=>setPrompt(e.target.value)}
    placeholder="선택: 이 장면의 새 연출 프롬프트. 비워 두면 기존 장면 프롬프트 재사용"
    maxLength={5000}/>
   <label className="flex items-start gap-2 text-[11px]">
    <input type="checkbox" checked={accepted} onChange={e=>setAccepted(e.target.checked)}/>
    <span>정상 장면은 재사용하고 나머지 {shot.expectedPaidCalls}개의 유료 호출이 필요함을 확인했습니다.</span>
   </label>
   <div className="flex justify-end">
    <Button size="sm" disabled={busy||!accepted||!shot.expectedQuote} onClick={()=>void retake()}>
     <RotateCcw size={13}/> 선택 장면만 다시 제작
    </Button>
   </div>
  </div>}
  {!eligible.length&&!data.loading&&<p className="mt-2 text-[11px] text-[#8A8176]">현재는 안전한 장면 단위 재생성을 수행할 수 없습니다. 불명확한 유료 결과를 먼저 확인해 주세요.</p>}
 </div>
}
