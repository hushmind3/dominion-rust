import {useCallback} from 'react'
import {type Agent} from '../api'
import {GoalWorkbench} from '../components/GoalWorkbench'
import {Notice} from '../components/UI'
import {useRemote} from '../components/UI'

// A dedicated entry point reuses the same goal console as Operations.
// All writes still go through the goal/job/Git verification endpoints.
export function GoalsPage(){
 const agents=useRemote<Agent[]>('/api/agents',[])
 const jobs=useRemote<{id:string;kind:string;status:string;input?:{goalId?:string}}[]>('/api/jobs',[])
 const refreshJobs=useCallback(()=>{void jobs.reload()},[jobs.reload])
 return <div className="mx-auto w-full max-w-[1320px] px-5 py-5 sm:px-7 lg:px-6 lg:pb-8">
  <div className="mb-5">
   <h1 className="text-xl font-bold tracking-tight">자율 목표</h1>
   <p className="mt-1 text-sm text-[#7C7469]">목표 생성·실행·중단·검토 후 재개·증거 검증·격리 Git 변경 검토</p>
  </div>
  {(agents.error||jobs.error)&&<Notice message={agents.error||jobs.error}/>}
  <GoalWorkbench agents={agents.value} jobs={jobs.value} onJobsChanged={refreshJobs}/>
 </div>
}
