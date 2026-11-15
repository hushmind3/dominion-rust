import {useCallback,useEffect,useState} from 'react'
import {get,post} from '../../api'
import {Button,TextArea} from '../../components/Primitives'

type Goal = {
  id: string
  goal?: string
  status?: string
  planRevision?: number
  appliedPlanRevision?: number
  ownerInstruction?: string
  ownerPlan?: {kind?: string;plan?: string;revision?: number}
}

/** The owner, not the model, controls a goal's next executor turn. */
export function GoalOwnerControls({goalId}:{goalId:string}){
  const [goal,setGoal]=useState<Goal|null>(null)
  const [instruction,setInstruction]=useState('')
  const [busy,setBusy]=useState(false)
  const [notice,setNotice]=useState('')
  const [error,setError]=useState('')
  const reload=useCallback(async()=>{
    const saved=await get<Goal>('/api/goals/'+encodeURIComponent(goalId))
    setGoal(saved)
  },[goalId])
  useEffect(()=>{
    let mounted=true
    get<Goal>('/api/goals/'+encodeURIComponent(goalId))
      .then(value=>{if(mounted)setGoal(value)})
      .catch(error=>{if(mounted)setError((error as Error).message)})
    return()=>{mounted=false}
  },[goalId])

  const active=goal?.status==='running'||goal?.status==='queued'
  const submit=async()=>{
    if(!instruction.trim()||!goal||!active)return
    setBusy(true);setError('');setNotice('')
    try{
      // Load the latest revision immediately before the CAS submission.
      // If a concurrent owner correction wins, the server returns conflict
      // instead of overwriting the newer instruction.
      const current=await get<Goal>('/api/goals/'+encodeURIComponent(goalId))
      if(current.status!=='running'&&current.status!=='queued')throw Error('이미 종료된 목표입니다.')
      const result=await post<{planRevision:number}>('/api/goals/'+encodeURIComponent(goalId)+'/steer',{
        instruction:instruction.trim(),expectedRevision:current.planRevision||0,
      })
      setInstruction('')
      setNotice('사용자 수정 지시가 저장됐습니다. 다음 실행 턴에서 적용합니다. · revision '+result.planRevision)
      await reload()
    }catch(e){setError((e as Error).message)}
    finally{setBusy(false)}
  }
  const stop=async()=>{
    if(!goal||!active)return
    setBusy(true);setError('');setNotice('')
    try{
      await post('/api/goals/'+encodeURIComponent(goalId)+'/cancel',{})
      setNotice('목표 중단 명령을 전달했습니다.')
      await reload()
    }catch(e){setError((e as Error).message)}
    finally{setBusy(false)}
  }
  return <div className="mt-3 rounded-lg border border-[#EEE5D9] bg-[#FAF7F1] p-3 text-xs">
    <div className="flex flex-wrap items-center justify-between gap-2">
      <strong>작업 방향 변경하기</strong>
      <Button variant="ghost" disabled={busy} onClick={()=>void reload().catch(e=>setError((e as Error).message))}>상태 새로고침</Button>
    </div>
    <p className="mt-1 text-[#756A5B]">현재 상태: {goal?.status||'불러오는 중'} · 지시 반영 여부: {(goal?.planRevision??0)===(goal?.appliedPlanRevision??0)?'적용됨':'다음 단계에서 반영'}</p>
    {goal?.ownerInstruction&&<p className="mt-2 break-words text-[#5C544A]">현재 적용된 지시: {goal.ownerInstruction}</p>}
    {active&&<>
      <TextArea rows={3} maxLength={6000} value={instruction} onChange={e=>setInstruction(e.target.value)}
        className="mt-2 min-h-20 w-full" placeholder="예: 방금 방법은 그만 쓰고, 다른 방식으로 다시 확인해 주세요."/>
      <div className="mt-2 flex flex-wrap items-center gap-2">
        <Button disabled={busy||!instruction.trim()} onClick={()=>void submit()}>내 지시 전달</Button>
        <Button variant="secondary" disabled={busy} onClick={()=>void stop()}>작업 중단</Button>
      </div>
    </>}
    {goal?.ownerPlan?.revision && (goal.appliedPlanRevision||0)<goal.ownerPlan.revision&&
      <p className="mt-2 text-[#756A5B]">사용자 지시가 다음 실행 단계에 반영될 예정입니다.</p>}
    {notice&&<p className="mt-2 text-emerald-700">{notice}</p>}
    {error&&<p role="alert" className="mt-2 text-rose-600">{error}</p>}
  </div>
}
