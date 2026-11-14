import {useEffect,useState} from 'react'
import {Bot,Check,ClipboardList,ExternalLink,LoaderCircle,PauseCircle,RefreshCw,RotateCw,Send,ShieldCheck} from 'lucide-react'
import {get,post} from '../api'
import {Button,TextArea} from '../components/Primitives'
import {Headline,Notice,useRemote} from '../components/UI'
import {StatusPill} from '../components/StatusPill'

type Artifact={path:string;sha256:string;bytes:number;sourceTool:string}
type Goal={id:string;goal:string;status:string;phase?:string;plan?:string;error?:string;
 planRevision?:number;appliedPlanRevision?:number;ownerPlan?:{revision:number;plan:string};
 planHistory?:{version:number;plan:string;reason:string}[];
 reviewHistory?:{iteration:number;complete:boolean;gaps:string[]}[];
 artifacts?:Artifact[];gaps?:string[];iterations?:number;maxIterations?:number;
 sessionId?:string;jobId?:string;resumeCount?:number;createdAt?:string;isolateWorktree?:boolean;gitBaseline?:{head:string;clean:boolean};
 gitWorktree?:{branch?:string;baseCommit?:string;path?:string;rolledBack?:boolean}}
type GoalDiff={paths?:string[];status:string;unstaged?:string;staged?:string;diff?:string;
 branch?:string;baseCommit?:string;untracked?:string;note?:string;
 patchSha256?:string;canApply?:boolean;patchBytes?:number}
type SharedChange={path:string;diff:string;currentSha256:string;canUndo:boolean;undoReason:string}
type SharedChanges={goalId:string;baselineClean:boolean;sameHead:boolean;files:SharedChange[];notice:string}
type Job={id:string;kind:string;status:string;error?:string;input?:{goalId?:string};
 stepIndex?:number;stepTotal?:number}

const active=(status:string)=>status==='queued'||status==='running'
export function AutonomousGoalsPage({openSession}:{openSession:(id:string)=>void}){
 const goals=useRemote<Goal[]>('/api/goals',[])
 const jobs=useRemote<Job[]>('/api/jobs',[])
 const [objective,setObjective]=useState('')
 const [isolate,setIsolate]=useState(true)
 const [drafts,setDrafts]=useState<Record<string,string>>({})
 const [diffs,setDiffs]=useState<Record<string,GoalDiff>>({})
 const [shared,setShared]=useState<Record<string,SharedChanges>>({})
 const [busy,setBusy]=useState(''),[error,setError]=useState(''),[notice,setNotice]=useState('')
 const reload=async()=>{await Promise.all([goals.reload(),jobs.reload()])}
 // SSE refreshes the records; polling fills in the gap after a tab reconnect.
 useEffect(()=>{
  const timer=window.setInterval(()=>{
   if(document.visibilityState==='visible'&&goals.value.some(goal=>active(goal.status)))
    void reload()
  },6000)
  return ()=>window.clearInterval(timer)
 },[goals.value,goals.reload,jobs.reload])
 const create=async()=>{
  if(!objective.trim()||busy)return
  setBusy('new');setError('');setNotice('')
  try{
   const created=await post<Goal>('/api/goals',{goal:objective.trim(),isolateWorktree:isolate})
   setObjective('');setNotice('자율 작업을 등록했습니다. 계획부터 실행·검증까지 작업 기록에 남습니다.')
   await reload()
   if(created.sessionId)openSession(created.sessionId)
  }catch(e){setError((e as Error).message)}finally{setBusy('')}
 }
 const steer=async(goal:Goal)=>{
  if(busy)return
  const plan=(drafts[goal.id]??goal.plan??'').trim()
  if(!plan)return
  setBusy(goal.id);setError('');setNotice('')
  try{
   await post('/api/goals/'+encodeURIComponent(goal.id)+'/steer',{
    plan,expectedRevision:goal.planRevision||0
   })
   setNotice('계획 변경이 접수됐습니다. 다음 안전한 실행 단계부터 적용됩니다.')
   setDrafts(prev=>{const next={...prev};delete next[goal.id];return next})
   await reload()
  }catch(e){setError((e as Error).message);await reload()}finally{setBusy('')}
 }
 const showDiff=async(goal:Goal)=>{
  if(diffs[goal.id]){setDiffs(previous=>{const next={...previous};delete next[goal.id];return next});return}
  setError('')
  try{
   const result=await get<GoalDiff>('/api/goals/'+encodeURIComponent(goal.id)+'/diff')
   setDiffs(previous=>({...previous,[goal.id]:result}))
  }catch(e){setError((e as Error).message)}
 }
 const showShared=async(goal:Goal)=>{
  if(shared[goal.id]){setShared(prev=>{const next={...prev};delete next[goal.id];return next});return}
  setError('')
  try{const snapshot=await get<SharedChanges>('/api/goals/'+encodeURIComponent(goal.id)+'/changes')
    setShared(prev=>({...prev,[goal.id]:snapshot}))
  }catch(e){setError((e as Error).message)}
 }
 const undoFile=async(goal:Goal,file:SharedChange)=>{
  if(busy||active(goal.status)||!file.canUndo)return
  if(!window.confirm('선택한 '+file.path+' 파일만 시작 당시 Git 내용으로 복원합니다. 다른 변경은 유지됩니다. 진행할까요?'))return
  setBusy(goal.id);setError('');setNotice('')
  try{
   await post('/api/goals/'+encodeURIComponent(goal.id)+'/undo',{
    path:file.path,expectedSha256:file.currentSha256
   })
   setShared(old=>{const next={...old};delete next[goal.id];return next})
   setNotice('선택한 파일의 원본 내용을 복원하고 검증했습니다.')
   await reload()
  }catch(e){setError((e as Error).message);setShared(old=>{const next={...old};delete next[goal.id];return next})}
  finally{setBusy('')}
 }
 const applyReviewed=async(goal:Goal)=>{
  const preview=diffs[goal.id]
  if(!preview?.canApply||!preview.patchSha256||busy)return
  if(!window.confirm('검토한 SHA-256 Git 패치를 원본 프로젝트에 적용합니다. 원본 HEAD와 청결 상태를 다시 검사하고 충돌하면 중단합니다. 적용하시겠습니까?'))return
  setBusy(goal.id);setError('');setNotice('')
  try{
   await post('/api/goals/'+encodeURIComponent(goal.id)+'/apply',{
    expectedPatchSha256:preview.patchSha256
   })
   setDiffs(old=>{const next={...old};delete next[goal.id];return next})
   setNotice('검토한 Git 패치를 원본 프로젝트에 적용하고 SHA-256 재검증을 마쳤습니다.')
   await reload()
  }catch(e){setError((e as Error).message);setDiffs(old=>{const next={...old};delete next[goal.id];return next})}
  finally{setBusy('')}
 }
 const discard=async(goal:Goal)=>{
  if(busy||!goal.gitWorktree||goal.gitWorktree.rolledBack||active(goal.status))return
  if(!window.confirm('이 목표의 독립 Git 작업공간과 커밋되지 않은 수정 내용을 폐기합니다. 원본 프로젝트에는 영향을 주지 않습니다. 진행할까요?'))return
  setBusy(goal.id);setError('');setNotice('')
  try{
   await post('/api/goals/'+encodeURIComponent(goal.id)+'/rollback',{})
   setDiffs(previous=>{const next={...previous};delete next[goal.id];return next})
   setNotice('이 목표 전용 Git 작업공간을 폐기했습니다. 원본 프로젝트는 유지됩니다.')
   await reload()
  }catch(e){setError((e as Error).message)}finally{setBusy('')}
 }
 const cancel=async(goal:Goal)=>{
  if(busy)return
  setBusy(goal.id);setError('');setNotice('')
  try{await post('/api/goals/'+encodeURIComponent(goal.id)+'/cancel',{})
   setNotice('목표 중지를 요청했습니다.');await reload()
  }catch(e){setError((e as Error).message)}finally{setBusy('')}
 }
 const resume=async(goal:Goal)=>{
  if(busy||!['needs_review','failed'].includes(goal.status))return
  if(!window.confirm('이전 실행의 파일 변경·외부 작업 결과를 확인하셨습니까? 결과가 불확실한 도구 호출은 반복하지 않고 완료된 단계 이후부터 이어갑니다.'))return
  setBusy(goal.id);setError('');setNotice('')
  try{
   await post('/api/goals/'+encodeURIComponent(goal.id)+'/resume',{acknowledgeUnknownEffects:true})
   setNotice('검토한 실행 증거를 승계하여 새 작업을 등록했습니다.')
   await reload()
  }catch(e){setError((e as Error).message);await reload()}finally{setBusy('')}
 }
 const ordered=[...goals.value].sort((a,b)=>(b.createdAt||'').localeCompare(a.createdAt||''))
 return <div className="mx-auto max-w-[1320px] space-y-5 px-5 py-5 sm:px-7 lg:px-6">
  <Headline eyebrow="AUTONOMOUS AGENT" title="🤖 자율 목표 에이전트"
   description="목표를 입력하면 프로젝트 탐색 → 계획 → 도구 실행 → 독립 검토 → 실패 수정으로 진행합니다. 실행 중 계획을 직접 바꿀 수 있습니다."/>
  <section className="rounded-2xl border border-[#E7E0D5] bg-white p-5">
   <label htmlFor="dominion-objective" className="mb-2 block text-sm font-semibold">완수할 작업</label>
   <TextArea id="dominion-objective" rows={3} value={objective}
    onChange={event=>setObjective(event.target.value)}
    placeholder="예: 현재 프로젝트의 로그인 오류를 조사하고, 관련 소스를 수정한 뒤 검증 증거와 변경 파일을 정리해줘"/>
   <div className="mt-3 flex flex-wrap items-center justify-between gap-3">
    <label className="flex cursor-pointer items-center gap-2 text-xs font-medium text-[#524941]">
     <input type="checkbox" checked={isolate} onChange={event=>setIsolate(event.target.checked)}
      className="size-4 accent-[#C96442]"/> Git 프로젝트를 독립 작업공간에서 수정
    </label>
    <p className="text-xs text-[#7C7469]">Git 격리는 원본을 보호합니다. OS 샌드박스는 아닙니다. 명령 실행은 승인을 따릅니다.</p>
    <Button variant="primary" disabled={busy!==''||!objective.trim()} onClick={()=>void create()}>
     {busy==='new'?<LoaderCircle size={15} className="animate-spin"/>:<Send size={15}/>} 작업 시작
    </Button>
   </div>
  </section>
  {error&&<Notice message={error}/>}
  {notice&&<p role="status" className="rounded-xl bg-[#F7F3EC] px-4 py-3 text-sm text-[#5C544A]">{notice}</p>}
  <div className="flex items-center justify-between"><h2 className="text-base font-bold">실행 기록</h2>
   <Button variant="ghost" size="sm" onClick={()=>void reload()}><RefreshCw size={14}/> 새로고침</Button></div>
  {ordered.length===0&&<p className="rounded-2xl border border-[#E7E0D5] p-6 text-center text-sm text-[#7C7469]">등록된 자율 목표가 없습니다.</p>}
  <div className="grid gap-4">{ordered.slice(0,30).map(goal=>{
   const job=jobs.value.find(row=>row.id===goal.jobId)??jobs.value.find(row=>row.kind==='goal'&&row.input?.goalId===goal.id)
   const revision=goal.planRevision||0,applied=goal.appliedPlanRevision||0
   const editor=drafts[goal.id]??goal.ownerPlan?.plan??goal.plan??''
   return <section key={goal.id} className="rounded-2xl border border-[#E7E0D5] bg-white p-5">
    <div className="flex flex-wrap items-start justify-between gap-3">
     <div className="min-w-0 flex-1"><h3 className="break-words text-[15px] font-bold">{goal.goal}</h3>
      <p className="mt-1 text-xs text-[#7C7469]">단계: {goal.phase||goal.status} · {goal.iterations||0}/{goal.maxIterations||16}회 · 계획 버전 {revision}</p>
     </div><StatusPill status={goal.status}>{goal.status}</StatusPill>
    </div>
    {active(goal.status)&&<div className="mt-3 h-1.5 overflow-hidden rounded-full bg-[#F1ECE4]">
     <div className="h-full bg-[#C96442]" style={{width:Math.min(100,100*(goal.iterations||0)/(goal.maxIterations||16))+'%'}}/>
    </div>}
    {job&&<p className="mt-2 text-[11px] text-[#7C7469]">작업 ID: {job.id} · 상태 {job.status}</p>}
    {(goal.error||job?.error)&&<p role="status" className="mt-2 whitespace-pre-wrap break-words text-xs text-[#A94E30]">실행 기록: {goal.error||job?.error}</p>}
    {goal.isolateWorktree&&<p className="mt-2 text-xs text-[#5C544A]">
      <ShieldCheck size={13} className="mr-1 inline"/> 독립 Git 작업공간
      {goal.gitWorktree?.branch&&' · '+goal.gitWorktree.branch}
      {goal.gitWorktree?.rolledBack?' · 폐기 완료':''}
     </p>}
    {['failed','needs_review'].includes(goal.status)&&<div className="mt-3 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3">
     <p className="mb-2 text-xs text-[#6F675D]">이전 실행의 변경·도구 결과를 검토한 뒤 중단 지점 이후부터 이어갈 수 있습니다. 불확실한 외부 작업은 재실행하지 않습니다.</p>
     <Button size="sm" variant="secondary" disabled={busy!==''} onClick={()=>void resume(goal)}>
      <RotateCw size={14}/> 검토 후 이어가기 {goal.resumeCount?('· 이전 재개 '+goal.resumeCount+'회'):''}
     </Button>
    </div>}
    {goal.gaps&&goal.gaps.length>0&&<details className="mt-3 rounded-xl bg-[#FFF7F1] p-3 text-xs">
     <summary className="cursor-pointer font-semibold">검토·수정이 필요한 항목 ({goal.gaps.length})</summary>
     <ul className="mt-2 list-disc space-y-1 pl-5">{goal.gaps.slice(0,12).map((gap,index)=><li key={index}>{gap}</li>)}</ul>
    </details>}
    <details className="mt-3 rounded-xl border border-[#EFE9DF] p-3" open={active(goal.status)}>
     <summary className="cursor-pointer text-xs font-semibold"><ClipboardList size={14} className="mr-1 inline"/> 실행 계획 {revision>applied&&'· 사용자 변경 적용 대기'}</summary>
     <div className="mt-3">
      <TextArea rows={5} value={editor} disabled={!active(goal.status)||busy===goal.id}
       onChange={event=>setDrafts(prev=>({...prev,[goal.id]:event.target.value}))}
       placeholder="계획 수립 중입니다. 이곳에서 직접 계획을 지정할 수 있습니다."/>
      {active(goal.status)&&<div className="mt-2 flex flex-wrap items-center justify-end gap-2">
       <Button size="sm" variant="ghost" disabled={busy!==''}
        onClick={()=>void cancel(goal)}><PauseCircle size={14}/> 중지</Button>
       <Button size="sm" variant="primary" disabled={busy!==''||!editor.trim()||editor.trim()===(goal.ownerPlan?.plan||goal.plan||'').trim()}
        onClick={()=>void steer(goal)}><Check size={14}/> 수정 계획 적용</Button>
      </div>}
     </div>
    </details>
    {(goal.artifacts?.length||0)>0&&<details className="mt-3 rounded-xl bg-[#FAF7F1] p-3">
     <summary className="cursor-pointer text-xs font-semibold"><ShieldCheck size={13} className="mr-1 inline"/> 실제 수정 파일 ({goal.artifacts!.length})</summary>
     <div className="mt-2 space-y-2">{goal.artifacts!.map(file=><div key={file.path} className="text-xs">
      <strong className="break-all">{file.path}</strong><p className="break-all font-mono text-[10px] text-[#7C7469]" title={file.sha256}>SHA-256 {file.sha256.slice(0,20)}… · {file.bytes.toLocaleString()} bytes</p>
     </div>)}</div><p className="mt-2 text-[11px] text-[#7C7469]">파일 해시는 무결성 기록입니다. 테스트 통과를 의미하지 않습니다.</p>
    </details>}
    {diffs[goal.id]&&<section aria-label="검증된 파일 변경 내용" className="mt-3 space-y-2 rounded-xl border border-[#EFE9DF] bg-[#FAF7F1] p-3">
     <p className="text-xs font-semibold">Git 변경 사항 · {diffs[goal.id].branch||'공유 작업공간'} · 파일 {diffs[goal.id].paths?.length||0}개</p>
     {diffs[goal.id].note&&<p className="text-xs text-[#7C7469]">{diffs[goal.id].note}</p>}
     {diffs[goal.id].patchSha256&&<p className="break-all text-[11px] text-[#7C7469]">패치 SHA-256: {diffs[goal.id].patchSha256}</p>}
     {diffs[goal.id].canApply&&!active(goal.status)&&
      <Button size="sm" variant="primary" disabled={busy!==''} onClick={()=>void applyReviewed(goal)}>
       검토된 변경 원본에 적용
      </Button>}
     {diffs[goal.id].status&&<pre className="overflow-auto whitespace-pre-wrap text-[11px]">{diffs[goal.id].status}</pre>}
     {diffs[goal.id].diff&&<pre className="max-h-72 overflow-auto whitespace-pre-wrap break-all rounded-md bg-white p-3 font-mono text-[11px]">{diffs[goal.id].diff}</pre>}
     {diffs[goal.id].untracked&&<p className="break-all text-[11px] text-[#7C7469]">미추적 파일: {diffs[goal.id].untracked}</p>}
     {diffs[goal.id].unstaged&&<pre className="max-h-72 overflow-auto whitespace-pre-wrap break-all rounded-md bg-white p-3 font-mono text-[11px]">{diffs[goal.id].unstaged}</pre>}
     {diffs[goal.id].staged&&<pre className="max-h-72 overflow-auto whitespace-pre-wrap break-all rounded-md bg-white p-3 font-mono text-[11px]">{diffs[goal.id].staged}</pre>}
     {!diffs[goal.id].note&&!diffs[goal.id].unstaged&&!diffs[goal.id].staged&&!diffs[goal.id].diff&&<p className="text-xs text-[#7C7469]">현재 작업 트리에 Git 변경이 없습니다. 이미 커밋된 변경 내용은 Git 기록에서 확인해야 합니다.</p>}
    </section>}
    {shared[goal.id]&&<section className="mt-3 rounded-xl border border-[#EFE9DF] p-3">
     <p className="text-xs font-semibold">공유 작업공간 변경 및 조건부 복원 · {shared[goal.id].files.length}개</p>
     <p className="mt-1 text-xs text-[#7C7469]">시작 시 청결: {String(shared[goal.id].baselineClean)} · 기준 HEAD 유지: {String(shared[goal.id].sameHead)}</p>
     <div className="mt-3 space-y-3">{shared[goal.id].files.map(file=><div key={file.path} className="space-y-1">
      <p className="break-all text-xs font-semibold">{file.path}</p>
      {file.diff&&<pre className="max-h-52 overflow-auto whitespace-pre-wrap break-all bg-[#FAF7F1] p-2 font-mono text-[11px]">{file.diff}</pre>}
      <Button size="sm" variant="ghost" disabled={!file.canUndo||busy!==''||active(goal.status)}
       onClick={()=>void undoFile(goal,file)}>이 파일만 원본으로 복원</Button>
      {!file.canUndo&&<p className="text-[11px] text-[#7C7469]">복원 불가: {file.undoReason}</p>}
     </div>)}</div>
    </section>}
    <div className="mt-3 flex flex-wrap items-center gap-2 text-xs">
     {goal.sessionId&&<Button size="sm" variant="ghost" onClick={()=>openSession(goal.sessionId!)}><Bot size={14}/> 작업 대화 열기</Button>}
     {!goal.isolateWorktree&&goal.gitBaseline&&<Button size="sm" variant="ghost"
       onClick={()=>void showShared(goal)}>{shared[goal.id]?'파일 복원 닫기':'변경·파일 복원'}</Button>}
     {(goal.gitWorktree&&!goal.gitWorktree.rolledBack||goal.artifacts?.length)&&<Button size="sm" variant="ghost" onClick={()=>void showDiff(goal)}><ExternalLink size={13}/>{diffs[goal.id]?'diff 닫기':'실제 Git diff 확인'}</Button>}
     {goal.gitWorktree&&!goal.gitWorktree.rolledBack&&!active(goal.status)&&
      <Button size="sm" variant="danger" disabled={busy!==''} onClick={()=>void discard(goal)}>작업공간 폐기</Button>}
     <a href="#/developer" className="inline-flex items-center gap-1 text-[#A94E30] hover:underline">개발자 도구</a>
     <span className="ml-auto text-[#7C7469]">재계획·검증 기록 {goal.reviewHistory?.length||0}회</span>
    </div>
   </section>
  })}</div>
 </div>
}
