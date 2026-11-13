import {useState} from 'react'
import {Play,RefreshCw,Square,RotateCcw,FileCode2,FolderTree,CheckCircle2,AlertTriangle} from 'lucide-react'
import {get,post} from '../api'
import {Button,SelectInput,TextArea,TextInput} from './Primitives'
import {Panel,useRemote} from './UI'

type Agent={id:string;name:string;providerId?:string;model?:string}
type Artifact={path:string;bytes:number;sha256?:string}
type Goal={
 id:string;goal:string;status:string;phase?:string;plan?:string;planScore?:{total?:number};createdAt?:string;
 jobId?:string;iterations?:number;maxIterations?:number;maxExecutorTokens?:number;
 gaps?:string[];deliverables?:{files?:Artifact[];writeEvidence?:number};
 workspaceInspection?:{entries?:{name:string;kind:string}[];projectFiles?:{file:string}[]};
 resumeHistory?:{at:string;resolutionNote:string}[];
}
const running=(status:string)=>['queued','running'].includes(status)

export function GoalConsole(){
 const goals=useRemote<Goal[]>('/api/goals',[])
 const agents=useRemote<Agent[]>('/api/agents',[])
 const [goalText,setGoalText]=useState('')
 const [agentId,setAgentId]=useState('')
 const [checkPath,setCheckPath]=useState('')
 const [maxTokens,setMaxTokens]=useState(80000)
 const [selected,setSelected]=useState('')
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState(''),[notice,setNotice]=useState('')
 const [reviewNote,setReviewNote]=useState('')
 const [resumeTokens,setResumeTokens]=useState(80000)
 const [editingPlan,setEditingPlan]=useState(false),[planDraft,setPlanDraft]=useState('')
 const [acknowledged,setAcknowledged]=useState(false)
 const [diff,setDiff]=useState<{id:string;text:string}|null>(null)
 const chosen=goals.value.find(g=>g.id===selected)||[...goals.value].sort((a,b)=>(b.createdAt||'').localeCompare(a.createdAt||''))[0]
 const available=agents.value.filter(a=>a.providerId&&a.model)
 const executor=available.find(a=>a.id===agentId)?.id||available[0]?.id||''
 const reload=async()=>{await goals.reload()}
 const start=async()=>{
  if(!goalText.trim()||busy||!executor)return
  setBusy(true);setError('');setNotice('')
  try{
   const criteria=checkPath.trim()?[{type:'file_exists',path:checkPath.trim()}]:[]
   const result=await post<Goal>('/api/goals',{goal:goalText.trim(),executor:{agentId:executor},
    maxIterations:16,maxReplans:6,replanAfter:2,maxExecutorTokens:maxTokens,successCriteria:criteria})
   setSelected(result.id);setGoalText('');setCheckPath('');setNotice('목표를 자율 실행 대기열에 등록했습니다.')
   await reload()
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const stop=async()=>{
  if(!chosen?.jobId||busy)return
  setBusy(true);setError('')
  try{await post('/api/jobs/'+encodeURIComponent(chosen.jobId)+'/cancel',{});await reload()}
  catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const resume=async()=>{
  if(!chosen||busy)return
  setBusy(true);setError('');setNotice('')
  try{
   const response=await post<{jobId:string}>('/api/goals/'+encodeURIComponent(chosen.id)+'/resume',{
    acknowledgeExternalEffects:acknowledged,resolutionNote:reviewNote.trim(),
    maxExecutorTokens:Math.max(chosen.maxExecutorTokens||80000,resumeTokens),
    maxIterations:Math.min(64,Math.max((chosen.maxIterations||16)+4,(chosen.iterations||0)+4))
   })
   setNotice('외부 작업 확인 기록을 보존하고 새 복구 작업을 등록했습니다. 작업 ID: '+response.jobId)
   setAcknowledged(false);setReviewNote('');setDiff(null);await reload()
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const savePlan=async()=>{
  if(!chosen||busy||!planDraft.trim())return
  setBusy(true);setError('')
  try{await post('/api/goals/'+encodeURIComponent(chosen.id)+'/plan',{plan:planDraft.trim()});
   setNotice('사용자 수정 계획을 다음 작업에 적용했습니다.');setEditingPlan(false);await reload()}
  catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const showDiff=async()=>{
  if(!chosen||busy)return
  setBusy(true);setError('')
  try{const data=await get<{diff:string;note?:string}>('/api/goals/'+encodeURIComponent(chosen.id)+'/diff')
   setDiff({id:chosen.id,text:data.diff||data.note||'변경된 추적 파일의 diff가 없습니다.'})}
  catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 return <Panel className="mb-5 p-5">
  <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
   <div><h2 className="m-0 flex items-center gap-2 text-[16px] font-bold text-[#24211D]"><FolderTree size={19}/> 자율 에이전트 · 목표 완수</h2>
    <p className="mt-1 text-xs text-[#7C7469]">목표 → 프로젝트 파악 → 계획 → 도구 실행 → 독립 검증 → 재계획 → 결과물</p></div>
   <Button size="sm" variant="ghost" onClick={()=>void reload()}><RefreshCw size={14}/> 목표 새로고침</Button>
  </div>
  <div className="grid gap-4 lg:grid-cols-2">
   <div className="space-y-3">
    <label className="block text-xs font-semibold text-[#3A342D]">완수할 목표
     <TextArea rows={4} value={goalText} onChange={e=>setGoalText(e.target.value)}
      placeholder="예: 이 프로젝트의 결제 API 오류를 분석하고 수정한 뒤 테스트 결과와 변경 파일을 보고해줘."/>
    </label>
    <div className="flex flex-wrap gap-2">
     <SelectInput aria-label="실행 에이전트" value={executor} onChange={e=>setAgentId(e.target.value)}>
      {!available.length&&<option value="">등록된 실행 모델이 없습니다.</option>}
      {available.map(a=><option key={a.id} value={a.id}>{a.name} · {a.model}</option>)}
     </SelectInput>
     <TextInput aria-label="결과 검증할 파일(선택)" value={checkPath} onChange={e=>setCheckPath(e.target.value)}
      placeholder="산출물 파일 경로 · 선택"/>
    </div>
    <label className="block text-xs text-[#7C7469]">실행 모델 토큰 예산 (다음 턴 진입 제한)
     <TextInput type="number" min={5000} max={2000000} value={maxTokens}
      onChange={e=>setMaxTokens(Math.max(5000,Math.min(2000000,Number(e.target.value)||80000)))}/>
    </label>
    <Button variant="primary" disabled={!executor||!goalText.trim()||busy} onClick={()=>void start()}>
     <Play size={15}/> 자율 작업 시작
    </Button>
    <div className="max-h-36 space-y-1 overflow-y-auto">
     {goals.value.map(goal=><button key={goal.id} type="button" onClick={()=>{setSelected(goal.id);setDiff(null)}}
      className={'flex w-full items-center justify-between gap-2 rounded-lg border p-2 text-left text-xs '+(chosen?.id===goal.id?'border-[#C7B18F] bg-[#F2EADF]':'border-[#E7E0D5]')}>
      <span className="min-w-0 flex-1 truncate">{goal.goal}</span><span className="shrink-0 text-[#7C7469]">{goal.status}</span>
     </button>)}
     {!goals.value.length&&<p className="text-xs text-[#9C9388]">아직 등록된 자율 목표가 없습니다.</p>}
    </div>
   </div>
   <div className="min-w-0 space-y-3">
    {chosen?<><div className="flex flex-wrap items-center gap-2">
     <strong className="text-xs text-[#3A342D]">{chosen.status} · {chosen.phase||'queued'}</strong>
     <span className="text-[11px] text-[#7C7469]">반복 {chosen.iterations||0}/{chosen.maxIterations||16}</span>
     {chosen.planScore?.total!==undefined&&<span className="text-[11px] text-[#7C7469]">계획 품질 지표 {Math.round(chosen.planScore.total*100)}%</span>}
     {running(chosen.status)&&<Button variant="danger" size="sm" onClick={()=>void stop()} disabled={busy}><Square size={13}/> 중단</Button>}
     <Button size="sm" variant="secondary" onClick={()=>void showDiff()} disabled={busy}><FileCode2 size={13}/> Diff 확인</Button>
    </div>
     {chosen.plan&&<details open><summary className="cursor-pointer text-xs font-semibold">실행 계획</summary>
       <pre className="mt-2 max-h-36 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-[#FAF7F1] p-3 text-xs">{chosen.plan}</pre>
       {['queued','failed','needs_review','cancelled'].includes(chosen.status)&&<div className="mt-2 space-y-2">
        <Button size="sm" variant="secondary" onClick={()=>{setPlanDraft(chosen.plan||'');setEditingPlan(!editingPlan)}}>계획 직접 수정</Button>
        {editingPlan&&<><TextArea rows={5} value={planDraft} onChange={e=>setPlanDraft(e.target.value)}/>
          <Button size="sm" disabled={!planDraft.trim()||busy} onClick={()=>void savePlan()}>수정한 계획 저장</Button></>}
       </div>}
     </details>}
     <div className="text-xs"><strong className="flex items-center gap-1"><CheckCircle2 size={14}/> 검증된 산출물</strong>
      {chosen.deliverables?.files?.length?<div className="mt-2 space-y-1">{chosen.deliverables.files.map(item=><div key={item.path} className="break-all rounded bg-[#FAF7F1] p-2">
       {item.path} · {item.bytes.toLocaleString()}B {item.sha256&&<span className="text-[#8A8176]">SHA256 {item.sha256.slice(0,12)}…</span>}
      </div>)}</div>:<p className="mt-1 text-[#8A8176]">검증된 파일 수정 산출물 없음</p>}
     </div>
     {(chosen.gaps?.length||0)>0&&<div><strong className="flex items-center gap-1 text-xs"><AlertTriangle size={14}/> 검증 보완사항</strong>
       <ul className="mt-1 list-disc space-y-1 pl-5 text-xs text-amber-800">{chosen.gaps.slice(0,5).map((gap,i)=><li key={i}>{gap}</li>)}</ul></div>}
     {['failed','needs_review','cancelled'].includes(chosen.status)&&<div className="space-y-2 border-t border-[#E7E0D5] pt-3">
      <p className="text-xs font-semibold">수동 확인 후 안전하게 재개</p>
      <label className="flex items-start gap-2 text-xs"><input type="checkbox" checked={acknowledged} onChange={e=>setAcknowledged(e.target.checked)}/>
       <span>이전 실행의 외부 결과와 변경 사항을 확인했습니다.</span></label>
      <TextArea rows={2} value={reviewNote} onChange={e=>setReviewNote(e.target.value)}
       placeholder="확인한 실제 상태와 남은 문제를 기록하세요. 불확실한 작업은 승인과 10자 이상 기록이 필요합니다."/>
      <label className="block text-xs text-[#7C7469]">재개 후 실행 모델 토큰 한도 (기존 {chosen.maxExecutorTokens||80000} 이상)
       <TextInput type="number" min={chosen.maxExecutorTokens||80000} max={2000000} value={Math.max(chosen.maxExecutorTokens||80000,resumeTokens)}
        onChange={e=>setResumeTokens(Math.min(2000000,Math.max(chosen.maxExecutorTokens||80000,Number(e.target.value)||80000)))}/>
      </label>
      <Button variant="secondary" disabled={busy} onClick={()=>void resume()}><RotateCcw size={14}/> 새 작업으로 재개</Button>
     </div>}
     {diff?.id===chosen.id&&<details open><summary className="cursor-pointer text-xs font-semibold">실제 Git Diff</summary>
      <pre className="mt-2 max-h-60 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-[#FAF7F1] p-2 text-[10px]">{diff.text}</pre></details>}
     </>:<p className="text-sm text-[#8A8176]">목표를 선택하면 계획·검증·복구 정보를 볼 수 있습니다.</p>}
   </div>
  </div>
  {notice&&<p className="mt-3 text-xs text-emerald-700">{notice}</p>}
  {error&&<p role="alert" className="mt-3 text-xs text-rose-600">{error}</p>}
 </Panel>
}
