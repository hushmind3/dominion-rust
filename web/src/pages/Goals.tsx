import {useCallback} from 'react'
import {type Agent} from '../api'
import {GoalWorkbench} from '../components/GoalWorkbench'
import {Notice} from '../components/UI'
import {useRemote} from '../components/UI'

// A dedicated entry point reuses the same goal console as Operations.
// All writes still go through the goal/job/Git verification endpoints.
type GoalMetrics={
 recordedGoals:number;eligibleTerminalGoals:number;harnessCompletedGoals:number;
 needsReviewGoals:number;completedWithModelCalls:number;
 completedWithNonEstimatedTokenUsage:number;completionRatePercent:number|null;
 modelUsage:{calls:number;totalTokens:number;estimatedCalls:number};
 independentBenchmark:boolean
}
const emptyMetrics:GoalMetrics={
 recordedGoals:0,eligibleTerminalGoals:0,harnessCompletedGoals:0,
 needsReviewGoals:0,completedWithModelCalls:0,completedWithNonEstimatedTokenUsage:0,
 completionRatePercent:null,modelUsage:{calls:0,totalTokens:0,estimatedCalls:0},
 independentBenchmark:false
}
export function GoalsPage(){
 const agents=useRemote<Agent[]>('/api/agents',[])
 const jobs=useRemote<{id:string;kind:string;status:string;input?:{goalId?:string}}[]>('/api/jobs',[])
 const metrics=useRemote<GoalMetrics>('/api/goals/metrics',emptyMetrics)
 const refreshJobs=useCallback(()=>{void jobs.reload();void metrics.reload()},[jobs.reload,metrics.reload])
 return <div className="mx-auto w-full max-w-[1320px] px-5 py-5 sm:px-7 lg:px-6 lg:pb-8">
  <div className="mb-5">
   <h1 className="text-xl font-bold tracking-tight">자율 목표</h1>
   <p className="mt-1 text-sm text-[#7C7469]">목표 생성·실행·중단·검토 후 재개·증거 검증·격리 Git 변경 검토</p>
  </div>
  {(agents.error||jobs.error||metrics.error)&&<Notice message={agents.error||jobs.error||metrics.error}/>}
  <div className="mb-4 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] px-4 py-3 text-xs">
   <div className="flex flex-wrap items-center justify-between gap-2">
    <strong className="text-sm">실제 목표 실행 기록</strong>
    <button type="button" className="font-semibold text-[#A64E32] hover:underline" onClick={()=>void metrics.reload()}>새로고침</button>
   </div>
   {metrics.value.eligibleTerminalGoals>0?
    <div className="mt-2 grid gap-2 sm:grid-cols-4">
     <span>종료된 목표 <strong>{metrics.value.eligibleTerminalGoals}</strong>개</span>
     <span>하네스 완료 <strong>{metrics.value.harnessCompletedGoals}</strong>개 ({metrics.value.completionRatePercent?.toFixed(1)}%)</span>
     <span>모델 호출 포함 완료 <strong>{metrics.value.completedWithModelCalls}</strong>개</span>
     <span>실측 토큰 동반 완료 <strong>{metrics.value.completedWithNonEstimatedTokenUsage}</strong>개</span>
    </div>:
    <p className="mt-2 text-[#7C7469]">아직 종료된 목표 기록이 없습니다. 실행 후 영수증이 누적되면 표시합니다.</p>}
   <p className="mt-2 text-[#7C7469]">로컬 목표 영수증 기준입니다. 외부 독립 벤치마크나 실모델 장기 완수율 증명이 아니며, 추정 사용량과 실제 공급자 청구액은 구분됩니다.</p>
  </div>
  <GoalWorkbench agents={agents.value} jobs={jobs.value} onJobsChanged={refreshJobs}/>
 </div>
}
