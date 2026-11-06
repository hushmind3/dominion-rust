import {useRemote,Notice,Panel,Subheading} from '../../components/UI'

type Growth={
 runs?:number;ratedRuns?:number;succeeded?:number;failed?:number;successRate?:number;
 failureClasses?:Record<string,number>;toolFailures?:Record<string,number>;
 guidance?:{text?:string;confidence?:number;evidenceCount?:number}[];
}
type AgentRow={id?:string;name?:string;growth?:Growth}
type Synergy={agents?:AgentRow[];sharedMethods?:any[];algorithmStats?:any[]}
type UsageGroup={providerId?:string;model?:string;purpose?:string;calls?:number;totalTokens?:number;estimatedCalls?:number}
type Usage={calls?:number;totalTokens?:number;completionTokens?:number;groups?:UsageGroup[]}
type Transfer={
 principles?:any[];rules?:any[];usages?:any[];learningCases?:any[];
}
type Policy={id?:string;slot?:string;name?:string;status?:string;evidenceGrade?:string;lastExperiment?:any}
type Experiment={id?:string;status?:string;fixture?:string;strategyId?:string;prompt?:string;error?:string}
const number=(value:any)=>typeof value==='number'?value:0
const percent=(value:any)=>`${Math.round(number(value)*100)}%`
const label=(value:any,fallback='기록 없음')=>typeof value==='string'&&value?value:fallback

export function GrowthDashboard(){
 const synergy=useRemote<Synergy>('/api/synergy',{agents:[],sharedMethods:[],algorithmStats:[]})
 const usage=useRemote<Usage>('/api/model-usage',{groups:[]})
 const transfer=useRemote<Transfer>('/api/transfer',{principles:[],rules:[],usages:[],learningCases:[]})
 const policies=useRemote<Policy[]>('/api/policies',[])
 const searches=useRemote<any[]>('/api/policy-searches',[])
 const experiments=useRemote<Experiment[]>('/api/experiment-requests',[])
 const agents=(synergy.value.agents||[]).filter(agent=>number(agent.growth?.runs)>0).sort((a,b)=>number(b.growth?.runs)-number(a.growth?.runs))
 const rated=agents.reduce((sum,agent)=>sum+number(agent.growth?.ratedRuns),0)
 const principles=transfer.value.principles||[]
 const rules=transfer.value.rules||[]
 const activePrinciples=principles.filter(item=>item.status==='active').length
 const calls=number(usage.value.calls)
 const tokens=number(usage.value.totalTokens)
 const groups=(usage.value.groups||[]).slice(0,10)
 return <div className="mt-3 grid gap-4">
  <Notice message={synergy.error||usage.error||transfer.error||policies.error||searches.error||experiments.error}/>
  <div className="grid grid-cols-2 gap-2 lg:grid-cols-4">
   <div className="rounded-xl border border-[#EFE9DF] bg-white p-3"><small className="text-[10px] font-semibold text-[#8A8176]">검증된 실행</small><strong className="block text-lg">{rated}</strong></div>
   <div className="rounded-xl border border-[#EFE9DF] bg-white p-3"><small className="text-[10px] font-semibold text-[#8A8176]">활성 원리</small><strong className="block text-lg">{activePrinciples}</strong></div>
   <div className="rounded-xl border border-[#EFE9DF] bg-white p-3"><small className="text-[10px] font-semibold text-[#8A8176]">공유 규칙</small><strong className="block text-lg">{rules.length}</strong></div>
   <div className="rounded-xl border border-[#EFE9DF] bg-white p-3"><small className="text-[10px] font-semibold text-[#8A8176]">모델 호출</small><strong className="block text-lg">{calls}</strong><small className="text-[10px] text-[#9C9388]">{tokens.toLocaleString()} 토큰</small></div>
  </div>

  <Panel className="min-w-0 p-4">
   <Subheading title="관측된 성장" extra={<span className="text-[10px] text-[#9C9388]">VERIFIED RUNS</span>}/>
   {!agents.length?<p className="text-sm text-[#7C7469]">아직 검증된 실행 기록이 없습니다.</p>:<div className="grid gap-2">{agents.slice(0,10).map(agent=>{
    const growth=agent.growth||{}
    const failures=Object.entries(growth.toolFailures||{}).sort((a,b)=>number(b[1])-number(a[1])).slice(0,3)
    const guidance=(growth.guidance||[])[0]
    return <div className="rounded-lg border border-[#EFE9DF] p-3" key={agent.id}>
     <div className="flex flex-wrap items-center gap-2"><strong className="text-sm">{label(agent.name,agent.id)}</strong><span className="text-[11px] text-[#7C7469]">{number(growth.runs)}회 실행 · 검증 {number(growth.ratedRuns)}회</span><span className="ml-auto rounded bg-[#F7E8E1] px-1.5 py-0.5 text-[10px] font-semibold text-[#A94E30]">{percent(growth.successRate)}</span></div>
     {failures.length>0&&<p className="mt-1 text-[11px] text-[#8A8176]">실패 도구: {failures.map(([tool,count])=>`${tool} ${number(count)}회`).join(' · ')}</p>}
     {guidance?.text&&<p className="mt-1 text-[11px] text-[#5C544A]">지침: {guidance.text}</p>}
    </div>
   })}</div>}
  </Panel>

  <Panel className="min-w-0 p-4">
   <Subheading title="모델 사용량" extra={<span className="text-[10px] text-[#9C9388]">API · LOCAL</span>}/>
   {!groups.length?<p className="text-sm text-[#7C7469]">아직 모델 호출 기록이 없습니다.</p>:<div className="grid gap-1 text-[11px]">{groups.map((group,index)=><div className="grid grid-cols-[1fr_auto_auto] gap-2 rounded-lg border border-[#EFE9DF] px-3 py-2" key={index}><span className="break-all font-semibold text-[#4A443C]">{label(group.model,'모델 미기록')}</span><span className="text-[#7C7469]">{label(group.purpose,'기타')} · {number(group.calls)}회</span><span className="text-[#7C7469]">{number(group.totalTokens).toLocaleString()} 토큰</span></div>)}</div>}
  </Panel>

  <Panel className="min-w-0 p-4">
   <Subheading title="모델 간 전이" extra={<span className="text-[10px] text-[#9C9388]">PROVENANCE</span>}/>
   {!principles.length&&!rules.length?<p className="text-sm text-[#7C7469]">아직 일반화된 원리나 공유 규칙이 없습니다. 검증된 실패 복구가 쌓이면 생성됩니다.</p>:<div className="grid gap-2">
    {principles.slice(0,6).map(principle=><div className="rounded-lg border border-[#EFE9DF] p-3" key={principle.id}>
     <div className="flex flex-wrap gap-2 text-[11px]"><strong>{label(principle.kind,'원리')}</strong><span className="text-[#7C7469]">{label(principle.status)} · 신뢰 {percent(principle.confidence)}</span><span className="ml-auto rounded bg-[#F7E8E1] px-1.5 py-0.5 text-[10px] text-[#A94E30]">사례 {number((principle.supportingCases||[]).length)}</span></div>
     <p className="mt-1 text-[12px] text-[#4A443C]">{label(principle.principle)}</p>
     <p className="mt-1 text-[10px] text-[#8A8176]">기여 모델 {(principle.supportingModels||[]).join(', ')||'기록 없음'} · 에이전트 {(principle.supportingAgents||[]).length}명 · 반례 {number((principle.counterexamples||[]).length)}</p>
    </div>)}
    {rules.slice(0,6).map(rule=><div className="rounded-lg border border-[#EFE9DF] p-3" key={rule.id}>
     <div className="flex flex-wrap gap-2 text-[11px]"><strong>조건부 방법</strong><span className="text-[#7C7469]">{label(rule.status)} · 신뢰 {percent(rule.confidence)}</span><span className="ml-auto text-[10px] text-[#A94E30]">사례 {number((rule.supportingCases||[]).length)}</span></div>
     <p className="mt-1 text-[12px] text-[#4A443C]">{label(rule.trigger)} → {label(rule.intervention?.tool,'도구')}</p>
     <p className="mt-1 text-[10px] text-[#8A8176]">기여 에이전트 {(rule.supportingAgents||[]).length}명 · 반례 {number((rule.counterexamples||[]).length)}</p>
    </div>)}
   </div>}
  </Panel>

  <Panel className="min-w-0 p-4">
   <Subheading title="실험과 승격" extra={<span className="text-[10px] text-[#9C9388]">EVIDENCE GATE</span>}/>
   <div className="grid gap-1 text-[11px]">
    {policies.value.slice(0,6).map(policy=><div className="flex flex-wrap items-center gap-2 rounded-lg border border-[#EFE9DF] px-3 py-2" key={policy.id}><strong>{label(policy.slot,'정책')}</strong><span className="text-[#7C7469]">{label(policy.name)}</span><span className="ml-auto">{label(policy.status)} · {label(policy.evidenceGrade,'E0')}</span></div>)}
    {experiments.value.slice(0,6).map(experiment=><div className="flex flex-wrap items-center gap-2 rounded-lg border border-[#EFE9DF] px-3 py-2" key={experiment.id}><strong>실험</strong><span className="text-[#7C7469]">{label(experiment.fixture,'fixture 없음')} · {label(experiment.strategyId)}</span><span className="ml-auto">{label(experiment.status)}{experiment.error?' · '+experiment.error:''}</span></div>)}
    <div className="text-[10px] text-[#9C9388]">정책 탐색 {searches.value.length}건 · 실험 요청 {experiments.value.length}건</div>
   </div>
  </Panel>
 </div>
}
