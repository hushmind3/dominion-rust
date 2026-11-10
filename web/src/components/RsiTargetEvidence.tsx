/** Source navigation evidence, not a proof of improved intelligence. */
export type TargetCandidate={
 path:string
 score?:number
 scoring?:string
 evidence?:{path?:number;symbols?:number;summary?:number;domain?:number;
  priorUnproductive?:number;sourceCost?:number}
 policyError?:string
}
export type RsiTargetSelection={
 source?:string
 domain?:string
 policyRevision?:string|null
 candidates?:TargetCandidate[]
}
export type RsiTrialProof={
 status?:string;verdict?:string;
 comparison?:{matchedFrozenCases?:boolean;holdoutCases?:number;holdoutWins?:number;
 holdoutRegressions?:number;reason?:string}
}
export type RsiReplanProof={
 reason?:string;
 coordinatorPlan?:{domain?:string;generatorCommit?:string};
 descendantPlan?:{domain?:string;generatorCommit?:string};
}

export function RsiTrialEvidence({trial,replan}:{trial?:RsiTrialProof;replan?:RsiReplanProof}){
 const result=trial?.comparison
 if(!trial&&!replan)return null
 return <div className="mt-2 space-y-1 text-[11px] text-[#655D51]">
  {replan&&<p>자식 개선기의 독립 재계획: {replan.coordinatorPlan?.domain||'기존'} → {replan.descendantPlan?.domain||'재선택'} · 소스 근거 재구성</p>}
  {trial&&<p>탐색 정책 실험: {trial.verdict||trial.status||'미결'}
   {result?.matchedFrozenCases===true
    ?` · 동일 동결 평가 ${result.holdoutCases??0}개 · 개선 ${result.holdoutWins??0}개 · 악화 ${result.holdoutRegressions??0}개`
    :' · 동일 동결 평가 증거 부족'}
   {result?.reason?` · ${result.reason}`:''}
  </p>}
 </div>
}

export function RsiTargetEvidence({selection}:{selection?:RsiTargetSelection}){
 if(!selection)return null
 if(selection.source==='user_selected'){
  return <p className="mt-1 text-[11px] text-[#7C7469]">개선 대상: 사용자가 지정한 파일</p>
 }
 const candidates=selection.candidates?.slice(0,2)||[]
 return <div className="mt-2 rounded-lg border border-[#E7E0D5] bg-white px-3 py-2">
  <p className="text-[11px] font-semibold text-[#5C544A]">코드 탐색 근거 · {selection.domain||'자율 탐색'}</p>
  {candidates.length?candidates.map(item=><div key={item.path} className="mt-2 text-[11px]">
   <p className="break-all font-medium text-[#40392F]">{item.path}</p>
   <p className="mt-0.5 text-[#7C7469]">파일명·함수·설명 일치 기반
    {item.evidence?.priorUnproductive?' · 이전 무성과 '+item.evidence.priorUnproductive+'회 반영':''}
    {item.scoring==='expression'?' · 실험용 계산식 적용':''}
   </p>
   {item.policyError&&<p className="mt-0.5 text-[#A94E30]">계산식 오류로 기본 탐색 사용: {item.policyError}</p>}
  </div>):<p className="mt-1 text-[11px] text-[#7C7469]">일치하는 파일 없음 · 추가 코드 탐색 필요</p>}
  <p className="mt-2 text-[10px] text-[#8A8176]">선정 근거는 탐색용 점수입니다. 실제 개선 성과는 독립 비교 후에만 인정합니다.</p>
 </div>
}
