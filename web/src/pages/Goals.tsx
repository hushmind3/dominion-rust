import {GoalConsole} from './workflows/GoalConsole'
import {Headline} from '../components/UI'

// A discoverable autonomous-goal workspace, sharing the very same durable
// console and API operations already used from Workflows. No forked state.
export function GoalsPage({openSession}:{openSession:(id:string)=>void}){
 return <div className="mx-auto w-full max-w-[1320px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
  <Headline eyebrow="AUTONOMOUS GOALS" title="자율 목표" description="목표 등록 · 계획 조향 · 검증 기록 · 중단 · 검토 후 재개 · 격리 Git 변경 검토"/>
  <GoalConsole openSession={openSession}/>
 </div>
}
