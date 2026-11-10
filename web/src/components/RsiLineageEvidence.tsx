type CaseMetric = {improved?:boolean;regressed?:boolean}
type CaseResult = {id?:string;domain?:string;holdout?:boolean;metrics?:CaseMetric[]}
export type RsiGenerationProof = {
 attempt:number
 accepted?:boolean
 generatorCommit?:string
 acceptedCommit?:string
 evaluation?:{verdict?:string;metrics?:{
  eligible?:boolean;pairedCases?:number;holdoutCases?:number
  improvedHoldouts?:number;regressions?:number;metaHoldoutGains?:number
  measurements?:CaseResult[]
 }}
}
type Props = {
 history:RsiGenerationProof[]
 maxAcceptedStreak?:number
 capabilityDemonstrated?:boolean
}
function streaks(history:RsiGenerationProof[]){
 let current=0,max=0
 for(const generation of history){
  current=generation.accepted===true?current+1:0
  max=Math.max(max,current)
 }
 return {current,max}
}
function shortSha(value?:string){return value&&/^[a-f0-9]{40}$/i.test(value)?value.slice(0,8):'—'}
export function RsiLineageEvidence({history,maxAcceptedStreak,capabilityDemonstrated}:Props){
 const {current,max}=streaks(history)
 const longest=Math.max(max,maxAcceptedStreak||0)
 const recent=history.slice(-8)
 return <section aria-label="재귀적 자기개선 실험 증거" className="mt-4 rounded-xl border border-[#E7E0D5] bg-white p-4">
  <div className="flex flex-wrap items-start justify-between gap-2">
   <div>
    <h3 className="text-[13px] font-bold text-[#3A342D]">재귀적 개선 검증</h3>
    <p className="mt-1 text-[11px] leading-5 text-[#7C7469]">코드 채택과 능력 입증은 별개입니다. 아래 수치는 독립 비교 결과 및 세대 계보에서 읽습니다.</p>
   </div>
   <span className={'rounded-full px-2 py-1 text-[11px] font-semibold '+(capabilityDemonstrated?'bg-[#E5F0DF] text-[#3D6032]':'bg-[#F4EEE5] text-[#775F3A]')}>
    {capabilityDemonstrated?'재귀 능력 입증':'재귀 능력 미입증'}
   </span>
  </div>
  <div className="mt-3 grid grid-cols-2 gap-2 text-[12px]">
   <div className="rounded-lg bg-[#FAF7F1] p-3"><span className="text-[#7C7469]">최대 연속 채택</span><strong className="mt-1 block text-lg">{longest} / 3세대</strong></div>
   <div className="rounded-lg bg-[#FAF7F1] p-3"><span className="text-[#7C7469]">현재 연속 채택</span><strong className="mt-1 block text-lg">{current}세대</strong></div>
  </div>
  {recent.length>0&&<details className="mt-3">
   <summary className="cursor-pointer text-[12px] font-semibold text-[#5C544A]">세대별 독립 평가·계보 확인 ({history.length}회)</summary>
   <div className="mt-2 grid gap-2">
    {recent.map((generation,index)=>{
     const metrics=generation.evaluation?.metrics
     const source=index===0?history[history.length-recent.length-1]?.acceptedCommit:recent[index-1].acceptedCommit
     const linked=!!source&&!!generation.generatorCommit&&source===generation.generatorCommit
     const cases=metrics?.measurements?.filter(item=>item.holdout===true)||[]
     const improved=cases.filter(item=>item.metrics?.some(metric=>metric.improved===true)).length
     const regressed=cases.filter(item=>item.metrics?.some(metric=>metric.regressed===true)).length
     return <div key={generation.attempt} className="rounded-lg border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-[11px] leading-5">
      <p className="font-semibold text-[#3A342D]">{generation.attempt}세대 · {generation.accepted?'개선 버전 채택':'채택 없음'}</p>
      <p className="text-[#7C7469]">실행 {shortSha(generation.generatorCommit)} → 채택 {shortSha(generation.acceptedCommit)}
       {source&&<span> · 이전 채택본 {linked?'계승 확인':'계승 연결 없음'}</span>}</p>
      {metrics?<p className="text-[#5C544A]">
       비교 {metrics.pairedCases??'—'}건 · 홀드아웃 {metrics.holdoutCases??'—'}건 · 개선 {metrics.improvedHoldouts??'—'}건 · 회귀 {metrics.regressions??'—'}건
       {typeof metrics.metaHoldoutGains==='number'&&<span> · 개선기 효과 {metrics.metaHoldoutGains}건</span>}
      </p>:<p className="text-[#7C7469]">독립 평가 수치 없음</p>}
      {cases.length>0&&<p className="text-[#7C7469]">개별 홀드아웃 근거 {cases.length}건 (지표 개선 {improved}, 악화 {regressed})</p>}
      {metrics?.eligible===false&&<p className="text-[#A94E30]">비교 자격 미달</p>}
     </div>
    })}
   </div>
  </details>}
  <p className="mt-3 text-[11px] text-[#8A8176]">3세대 연속 채택만으로는 성공으로 표시하지 않습니다. 최종 입증은 서버의 독립 홀드아웃·메타 전이 검증 결과를 따릅니다.</p>
 </section>
}
