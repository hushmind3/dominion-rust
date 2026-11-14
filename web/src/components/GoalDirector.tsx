import {useState} from 'react'
import {ArrowRight,RotateCcw} from 'lucide-react'
import {Button,SelectInput,TextArea} from './Primitives'
import {useRemote,refresh} from './UI'
import {post} from '../api'

type RunningGoal={
 id:string;goal:string;status:string;phase?:string;iterations?:number;
 ownerObjectiveRevision?:number;appliedOwnerObjectiveRevision?:number;
}

export function GoalDirector(){
 const goals=useRemote<RunningGoal[]>('/api/goals',[])
 const running=goals.value.filter(goal=>goal.status==='running')
 const [selection,setSelection]=useState('')
 const [instruction,setInstruction]=useState('')
 const [working,setWorking]=useState(false)
 const [error,setError]=useState('')
 const [status,setStatus]=useState('')
 const current=running.find(item=>item.id===selection)||running[0]

 const redirect=async()=>{
  if(!current||!instruction.trim()||working)return
  setWorking(true);setError('');setStatus('')
  try{
   const queued=await post<{revision:number;status:string}>(
    '/api/goals/'+encodeURIComponent(current.id)+'/objective',{
     expectedRevision:Number(current.ownerObjectiveRevision??0),
     goal:instruction,
    })
   setStatus('새 목표 '+queued.revision+'번을 저장했습니다. 다음 실행 단계에서 이전 계획·검증 기준을 변경합니다.')
   setInstruction('')
   await goals.reload()
   refresh()
  }catch(e){setError((e as Error).message)}
  finally{setWorking(false)}
 }
 if(!running.length)return null
 const pending=Number(current?.ownerObjectiveRevision??0)>Number(current?.appliedOwnerObjectiveRevision??0)
 return <section aria-label="실행 중인 자율 목표 지휘" className="rounded-xl border border-[#D7CFC1] bg-[#FFFDF9] p-4">
  <div className="flex flex-wrap items-center justify-between gap-3">
   <div>
    <h2 className="text-[14px] font-bold text-[#24211D]">자율 목표 지휘권</h2>
    <p className="mt-1 text-[11px] text-[#7C7469]">실행 중에도 목표를 바꿀 수 있습니다. 기존 실행 기록은 보존하고 새 목표의 성공 조건을 새로 검증합니다.</p>
   </div>
   <Button type="button" variant="ghost" onClick={()=>void goals.reload()} aria-label="실행 중 목표 새로고침" className="flex items-center gap-1 text-[11px]"><RotateCcw size={13}/> 새로고침</Button>
  </div>
  <form onSubmit={e=>{e.preventDefault();void redirect()}} className="mt-3 flex flex-col gap-2">
   <SelectInput aria-label="변경할 실행 목표" value={current?.id||''} onChange={e=>{setSelection(e.target.value);setStatus('');setError('')}} className="w-full">
    {running.map(goal=><option key={goal.id} value={goal.id}>
     {(goal.goal||goal.id).slice(0,95)} · {goal.phase||'실행'} · {goal.iterations??0}회차
    </option>)}
   </SelectInput>
   <TextArea aria-label="새 사용자 목표 원문" rows={3} value={instruction} onChange={e=>setInstruction(e.target.value)} placeholder="새로운 목표를 그대로 입력하세요. 현재 목표와 검증 기준을 새 지시로 교체합니다." className="w-full"/>
   <div className="flex flex-wrap items-center justify-between gap-2">
    <span className="text-[11px] text-[#8A8176]">{pending?'새 목표 반영 대기 중':'현재 적용된 목표 버전 '+String(current?.appliedOwnerObjectiveRevision??0)}</span>
    <Button type="submit" disabled={working||!instruction.trim()} variant="primary" className="flex items-center gap-1 px-3 py-2 text-xs">{working?'목표 저장 중':'새 목표 적용'} <ArrowRight size={13}/></Button>
   </div>
  </form>
  {status&&<p role="status" className="mt-2 text-[11px] text-[#376E54]">{status}</p>}
  {(error||goals.error)&&<p role="alert" className="mt-2 text-[11px] text-[#B84B43]">{error||goals.error}</p>}
 </section>
}
