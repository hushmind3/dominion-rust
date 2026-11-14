import {useState} from 'react'
import {DialogBackdrop,DialogPanel} from '../../components/Dialog'
import {Button} from '../../components/Primitives'
import {get} from '../../api'
import {Notice} from '../../components/UI'
import {Copy,ExternalLink,FileCheck2,X} from 'lucide-react'

type StepReceipt={stepId?:string;name:string;agentId?:string;providerId?:string;model?:string;content?:string;messageId?:string;
 committedToolReceiptIds?:string[];toolErrorCount?:number}
type Artifact={id:string;workflowName:string;sessionId:string;finalContent:string;runInput:string;independentlyVerified?:boolean;
 verification:string;modelUsage?:{totalTokens?:number;estimatedCalls?:number;cachedPromptTokens?:number};
 steps?:StepReceipt[]}
export function useWorkflowDeliverable(){
 const [item,setItem]=useState<Artifact|null>(null)
 const [error,setError]=useState('')
 const [busy,setBusy]=useState(false)
 const open=async(id:string)=>{
  setBusy(true);setError('')
  try{setItem(await get<Artifact>('/api/workflows/deliverables/'+encodeURIComponent(id)))}
  catch(e){setError((e as Error).message)}
  finally{setBusy(false)}
 }
 return {item,error,busy,open,close:()=>setItem(null)}
}
export function DeliverableDialog({item,onClose,openSession}:{
 item:Artifact;onClose:()=>void;openSession:(id:string)=>void}){
 const [copied,setCopied]=useState(false)
 const copy=async()=>{await navigator.clipboard.writeText(item.finalContent||'');setCopied(true)}
 return <DialogBackdrop onDismiss={onClose}><DialogPanel size="wide">
  <div className="flex items-center justify-between border-b border-[#EFE9DF] p-5">
   <div><small>영속 작업 결과 · {item.id}</small><h2 className="text-lg font-semibold">{item.workflowName||'워크플로 최종 산출물'}</h2></div>
   <Button variant="ghost" size="icon" onClick={onClose}><X size={18}/></Button>
  </div>
  <div className="grid gap-4 p-5">
   <div className="flex flex-wrap items-center gap-2 text-xs text-[#756A60]">
    <FileCheck2 size={16}/> 실행된 단계 {item.steps?.length||0}개
    <span>· 관측 토큰 {item.modelUsage?.totalTokens?.toLocaleString('ko-KR')??'미측정'}</span>
    <span>· 사용량 추정 호출 {item.modelUsage?.estimatedCalls??'미측정'}</span>
   </div>
   <p className="text-xs text-amber-800">완성된 모델 산출물과 실행 도구 영수증입니다. 외부 독립 품질검사는 수행되지 않았습니다.</p>
   <pre className="max-h-80 overflow-auto whitespace-pre-wrap break-words rounded-xl bg-[#FAF7F1] p-4 text-sm text-[#453D35]">{item.finalContent||'최종 출력이 비어 있습니다.'}</pre>
   <div className="flex flex-wrap gap-2">
    <Button onClick={()=>void copy()}><Copy size={14}/>{copied?' 복사됨':' 산출물 복사'}</Button>
    {item.sessionId&&<Button variant="secondary" onClick={()=>openSession(item.sessionId)}><ExternalLink size={14}/> 전체 실행 대화</Button>}
   </div>
   <div className="space-y-2 border-t border-[#EFE9DF] pt-3">
    <strong className="text-sm">실제 단계 실행 기록</strong>
    {(item.steps||[]).map((step,i)=><div key={step.stepId||i} className="rounded-lg border border-[#EEE5D9] p-3 text-xs">
      <strong>{i+1}. {step.name}</strong><p>에이전트 {step.agentId||'미지정'} · 모델 {step.model||'미기록'}</p>
      <p>영속 도구 영수증 {step.committedToolReceiptIds?.length||0}건 · 실행 중 오류 {step.toolErrorCount||0}건</p>
      {(step.committedToolReceiptIds||[]).length>0&&<p className="break-all text-[#867B70]">영수증: {step.committedToolReceiptIds?.join(', ')}</p>}
    </div>)}
   </div>
  </div>
 </DialogPanel></DialogBackdrop>
}
