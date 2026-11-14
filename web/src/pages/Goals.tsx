import {GoalConsole} from '../components/GoalConsole'
import {Headline} from '../components/UI'

/** Full-width goal workspace; the dashboard remains an overview. */
export function GoalsPage(){
 return <div className="mx-auto w-full max-w-[1320px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
  <Headline eyebrow="DOMINION" title="자율 목표" description="목표 수립부터 실행, 계획 변경, 승인, 중단·재개, Git 변경 검토와 산출물 확인까지 한곳에서 관리합니다."/>
  <GoalConsole/>
 </div>
}
