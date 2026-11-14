import {useEffect,useMemo,useState} from 'react'
import {Bot,CheckCircle2,FileCode2,ListChecks,Play,RefreshCw,Square,Workflow} from 'lucide-react'
import {type Agent,get,post} from '../api'
import {Button,SelectInput,TextArea,TextInput} from './Primitives'
import {FormField,Notice,Panel,Subheading,useRemote} from './UI'
import {StatusPill} from './StatusPill'

type Goal={
 id:string;goal:string;status:string;phase?:string;sessionId?:string;
 plan?:string;planHistory?:{version?:number;plan?:string;reason?:unknown}[];
 iterations?:number;replans?:number;maxIterations?:number;error?:string;
 gaps?:string[];evidence?:{tool?:string;arguments?:{path?:string};result?:unknown}[];
 acceptanceChecks?:{type?:string;path?:string;tool?:string}[];
 receipt?:{verified?:boolean;successfulToolEvidence?:number;tokenUsage?:{totalTokens?:number;calls?:number}};
 workspaceSnapshot?:{stack?:string[];manifests?:string[]};
 gitWorktree?:{path?:string;branch?:string;baseCommit?:string;rolledBack?:boolean;appliedAt?:string};
 publishPr?:boolean;
}
type GoalJob={id:string;kind:string;status:string;input?:{goalId?:string}}
type Props={agents:Agent[];jobs:GoalJob[];onJobsChanged:()=>void}

const active=(status:string)=>status==='queued'||status==='running'
const preview=(text:unknown,n=240)=>String(text??'').slice(0,n)

export function GoalWorkbench({agents,jobs,onJobsChanged}:Props){
 const goals=useRemote<Goal[]>('/api/goals',[],false)
 const [request,setRequest]=useState('')
 const [agentId,setAgentId]=useState('')
 const [tokenBudget,setTokenBudget]=useState(0)
 const [isolated,setIsolated]=useState(false)
 const [publishPr,setPublishPr]=useState(false)
 const [verifyPreset,setVerifyPreset]=useState('none')
 const [diff,setDiff]=useState<{diff:string;status:string;untracked:string;truncated?:boolean;
  review?:{sha256:string;bytes:number;canApply:boolean;notice?:string};reviewError?:string}|null>(null)
 const [selectedId,setSelectedId]=useState('')
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const ordered=useMemo(()=>[...goals.value].reverse(),[goals.value])
 const selected=ordered.find(goal=>goal.id===selectedId)??ordered[0]
 const pending=ordered.some(goal=>active(goal.status))
 useEffect(()=>{
  if(!pending)return
  const timer=setInterval(()=>{void goals.reload();onJobsChanged()},5000)
  return()=>clearInterval(timer)
 },[pending,goals.reload,onJobsChanged])
 const create=async()=>{
  if(!request.trim()||busy)return
  setBusy(true);setError('')
  try{
   const goal=await post<Goal>('/api/goals',{
    goal:request.trim(),tokenBudget,isolateWorktree:isolated,publishPr:isolated&&publishPr,
    verifyCommands:verifyPreset==='rust'?['cargo test']:verifyPreset==='web'?['npm run typecheck --prefix web']:
     verifyPreset==='both'?['cargo test','npm run typecheck --prefix web']:[],
    ...(agentId?{executor:{agentId}}:{})
   })
   setRequest('')
   setSelectedId(goal.id);setDiff(null)
   await goals.reload();onJobsChanged()
  }catch(reason){setError((reason as Error).message)}finally{setBusy(false)}
 }
 const stop=async()=>{
  if(!selected||busy)return
  const job=jobs.find(item=>item.kind==='goal'&&item.input?.goalId===selected.id&&active(item.status))
  if(!job){setError('실행 중인 목표 작업을 찾지 못했습니다. 새로고침 후 확인하세요.');return}
  setBusy(true);setError('')
  try{await post('/api/jobs/'+job.id+'/cancel',{});await goals.reload();onJobsChanged()}
  catch(reason){setError((reason as Error).message)}finally{setBusy(false)}
 }
 const loadDiff=async()=>{
  if(!selected)return
  setBusy(true);setError('')
  try{setDiff(await get('/api/goals/'+selected.id+'/changes'))}
  catch(reason){setError((reason as Error).message)}finally{setBusy(false)}
 }
 const applyPatch=async()=>{
  if(!selected||!diff?.review?.canApply||busy)return
  const sha=diff.review.sha256
  if(!window.confirm('Git 패치 '+sha.slice(0,16)+'…의 변경 내역을 확인하셨습니까? 깨끗한 원본 Git 작업공간에 적용합니다.'))return
  setBusy(true);setError('')
  try{
   await post('/api/goals/'+selected.id+'/apply',{expectedPatchSha256:sha})
   setDiff(null);await goals.reload();onJobsChanged()
  }catch(reason){setError((reason as Error).message)}finally{setBusy(false)}
 }
 const discard=async()=>{
  if(!selected||busy)return
  if(!window.confirm('이 목표의 독립 Git 작업공간을 폐기하시겠습니까? 커밋하지 않은 수정도 삭제됩니다. 원본·원격 브랜치는 변경하지 않습니다.'))return
  setBusy(true);setError('')
  try{await post('/api/goals/'+selected.id+'/rollback',{});setDiff(null);await goals.reload();onJobsChanged()}
  catch(reason){setError((reason as Error).message)}finally{setBusy(false)}
 }
  return <Panel className="min-w-0 p-5">
  <Subheading title="자율 에이전트 · 목표부터 결과까지" extra={<div className="flex items-center gap-2">
   <span className="text-[11px] text-[#7C7469]">{goals.value.length} goals</span>
   <Button variant="ghost" size="sm" onClick={()=>{void goals.reload();onJobsChanged()}}><RefreshCw size={14}/> 새로고침</Button>
  </div>}/>
  <p className="mb-3 text-[12px] leading-5 text-[#7C7469]">목표 → 프로젝트 구조 파악 → 계획 → 도구 작업 → 독립 검토 → 실패 복구. 검증된 결과만 완료로 기록합니다.</p>
  <div className="grid gap-2">
   <FormField label="완수할 목표"><TextArea rows={3} value={request} onChange={e=>setRequest(e.target.value)}
    placeholder="예: 이 프로젝트의 로그인 오류를 조사하고, 코드를 수정한 다음 관련 파일과 검증 결과까지 남겨줘"/></FormField>
   <div className="flex flex-wrap items-center gap-2">
    <SelectInput aria-label="목표 실행 에이전트" value={agentId} onChange={e=>setAgentId(e.target.value)}>
     <option value="">자동 모델 라우팅</option>
     {agents.map(agent=><option key={agent.id} value={agent.id}>{agent.name}</option>)}
    </SelectInput>
    <label className="flex items-center gap-2 text-[11px] text-[#7C7469]">실행 토큰 상한
     <TextInput type="number" min={0} max={2000000} step={1000}
      value={tokenBudget} onChange={event=>setTokenBudget(Math.max(0,Math.min(2000000,Number(event.target.value)||0)))}
      className="!w-28" aria-label="목표 실행 토큰 상한"/>
     <span>0 = 제한 없음</span>
    </label>
    <label className="flex items-center gap-1 text-[11px]"><TextInput type="checkbox" checked={isolated}
     onChange={event=>{setIsolated(event.target.checked);if(!event.target.checked)setPublishPr(false)}}/> 독립 Git 브랜치</label>
    <label className="flex items-center gap-1 text-[11px]"><TextInput type="checkbox" checked={publishPr}
     disabled={!isolated} onChange={event=>setPublishPr(event.target.checked)}/> GitHub PR 제출</label>
    <SelectInput aria-label="필수 검증 명령" value={verifyPreset} onChange={event=>setVerifyPreset(event.target.value)}>
     <option value="none">검증: 자동</option>
     <option value="rust">Rust 테스트 필수</option>
     <option value="web">웹 타입검사 필수</option>
     <option value="both">Rust + 웹 검사 필수</option>
    </SelectInput>
    <Button disabled={busy||!request.trim()} onClick={()=>void create()}><Play size={14}/> {busy?'처리 중':'자율 작업 시작'}</Button>
   </div>
  </div>
  {(error||goals.error)&&<Notice message={error||goals.error}/>}
  <div className="mt-4 grid gap-4 xl:grid-cols-[minmax(190px,280px)_minmax(0,1fr)]">
   <div className="grid max-h-[440px] content-start gap-2 overflow-auto">
    {ordered.map(goal=><button key={goal.id} type="button" onClick={()=>{setSelectedId(goal.id);setDiff(null)}}
      className={'min-w-0 rounded-xl border px-3 py-3 text-left '+(selected?.id===goal.id?'border-[#B89A75] bg-[#FAF5EB]':'border-[#ECE4D8] bg-white hover:bg-[#FCFAF6]')}>
      <div className="mb-1 flex items-center gap-2"><Bot size={14}/><StatusPill status={goal.status}/></div>
      <p className="line-clamp-2 text-[12px] font-semibold text-[#3F3931]">{goal.goal}</p>
      <span className="text-[10px] text-[#8A8176]">{goal.phase||goal.status} · {goal.iterations||0}/{goal.maxIterations||16}회</span>
    </button>)}
    {!ordered.length&&<p className="rounded-lg border border-dashed border-[#E7E0D5] p-4 text-[12px] text-[#8A8176]">첫 자율 작업을 등록하세요.</p>}
   </div>
   {selected&&<div className="min-w-0 rounded-xl border border-[#ECE4D8] bg-[#FFFEFC] p-4">
    <div className="flex flex-wrap items-center justify-between gap-2">
     <div className="flex items-center gap-2"><Workflow size={16}/><strong className="text-[13px]">작업 실행 기록</strong><StatusPill status={selected.status}/></div>
     <div className="flex flex-wrap items-center gap-1">
      {selected.gitWorktree?.path&&!selected.gitWorktree?.rolledBack&&
       <Button size="sm" variant="secondary" disabled={busy} onClick={()=>void loadDiff()}>변경 diff</Button>}
      {selected.gitWorktree?.path&&!selected.gitWorktree?.rolledBack&&!active(selected.status)&&
       <Button size="sm" variant="secondary" disabled={busy} onClick={()=>void discard()}>로컬 되돌리기</Button>}
      {active(selected.status)&&<Button size="sm" variant="secondary" disabled={busy} onClick={()=>void stop()}><Square size={12}/> 중단</Button>}
     </div>
    </div>
    <p className="mt-2 break-words text-[12px] font-medium">{selected.goal}</p>
    <div className="mt-3 flex flex-wrap gap-3 text-[11px] text-[#7C7469]">
     <span>단계: {selected.phase||'대기'}</span>
     <span>실행: {selected.iterations||0}/{selected.maxIterations||16}</span>
     <span>재계획: {selected.replans||0}</span>
     <span>도구 증거: {selected.evidence?.length||0}</span>
     {selected.receipt?.tokenUsage?.totalTokens!==undefined&&
      <span>실행 사용량: {selected.receipt.tokenUsage.totalTokens.toLocaleString()} 토큰</span>}
    </div>
    {selected.gitWorktree?.branch&&<p className="mt-2 break-all text-[11px] text-[#7C7469]">Git: {selected.gitWorktree.branch}{selected.gitWorktree.rolledBack?' · 작업공간 폐기됨':''}</p>}
    {diff&&<details open className="mt-3 rounded-xl border border-[#E7E0D5] p-3">
     <summary className="cursor-pointer text-[12px] font-semibold">Git 변경 내역 {diff.truncated?'· 일부 생략':''}</summary>
     <p className="mt-2 text-[11px] text-[#7C7469]">상태: {diff.status||'clean'}</p>
     {diff.untracked&&<p className="mt-1 break-all text-[11px]">추가 파일: {diff.untracked}</p>}
     <pre className="mt-2 max-h-80 overflow-auto whitespace-pre-wrap break-all bg-[#FAF7F1] p-2 text-[10px]">{diff.diff||'추적 파일의 변경이 없습니다.'}</pre>
     {diff.review?.sha256&&<p className="mt-2 break-all text-[11px] text-[#7C7469]">패치 SHA-256: <code>{diff.review.sha256}</code> · {diff.review.bytes.toLocaleString()} bytes</p>}
     {diff.reviewError&&<p className="mt-1 text-[11px] text-rose-700">원본 적용 불가: {diff.reviewError}</p>}
     {diff.review?.notice&&<p className="mt-1 text-[10px] text-[#8A8176]">{diff.review.notice}</p>}
     {diff.review?.canApply&&!active(selected.status)&&<Button size="sm" variant="secondary" disabled={busy} onClick={()=>void applyPatch()}>검토한 패치 원본에 적용</Button>}
     {selected.gitWorktree?.appliedAt&&<p className="mt-1 text-[11px]">✅ 적용 완료: {selected.gitWorktree.appliedAt}</p>}
    </details>}
    {selected.workspaceSnapshot?.stack?.length? <p className="mt-2 text-[11px] text-[#7C7469]">프로젝트: {selected.workspaceSnapshot.stack.join(', ')}</p>:null}
    <details open className="mt-3 border-t border-[#ECE4D8] pt-3">
     <summary className="cursor-pointer text-[12px] font-semibold"><ListChecks size={14} className="mr-1 inline"/> 계획 및 수정 이력</summary>
     <pre className="mt-2 max-h-44 overflow-auto whitespace-pre-wrap break-words text-[11px] leading-5">{selected.plan||'계획 대기 중'}</pre>
     {(selected.planHistory||[]).length>1&&<div className="mt-2 grid gap-1">{selected.planHistory!.map((entry,index)=><details key={index} className="rounded border border-[#EDE7DC] p-2 text-[11px]"><summary>계획 버전 {entry.version??index} · 사유: {preview(entry.reason,60)}</summary><pre className="mt-1 whitespace-pre-wrap break-words">{entry.plan}</pre></details>)}</div>}
    </details>
    <details className="mt-3 border-t border-[#ECE4D8] pt-3">
     <summary className="cursor-pointer text-[12px] font-semibold"><CheckCircle2 size={14} className="mr-1 inline"/> 검증 조건 · {selected.acceptanceChecks?.length||0}개</summary>
     <div className="mt-2 grid gap-1 text-[11px]">{selected.acceptanceChecks?.map((check,index)=><p key={index}>{index+1}. {check.type} · {preview(check.path||check.tool,110)}</p>)}</div>
    </details>
    {(selected.gaps||[]).length>0&&<div className="mt-3 rounded-lg border border-[#EED8C3] p-3">
     <strong className="text-[12px]">미충족 조건 · {selected.gaps!.length}개</strong>
     {selected.gaps!.slice(0,10).map((gap,index)=><p className="mt-1 break-words text-[11px]" key={index}>{gap}</p>)}
    </div>}
    <details className="mt-3 border-t border-[#ECE4D8] pt-3">
     <summary className="cursor-pointer text-[12px] font-semibold"><FileCode2 size={14} className="mr-1 inline"/> 실제 도구 증거 · {selected.evidence?.length||0}개</summary>
     <div className="mt-2 grid max-h-48 gap-1 overflow-auto">{selected.evidence?.map((item,index)=><div key={index} className="rounded-lg bg-[#FAF7F1] px-2 py-1.5 text-[11px]"><strong>{item.tool||'tool'}</strong>{item.arguments?.path&&<span className="ml-2 break-all">{item.arguments.path}</span>}</div>)}</div>
    </details>
    {selected.error&&<p className="mt-2 break-words text-[11px] text-rose-700">{selected.error}</p>}
   </div>}
  </div>
 </Panel>
}
