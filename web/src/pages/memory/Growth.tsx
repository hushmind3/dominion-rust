import {t, useLocale, formatDateTime} from '../../i18n'
import {statusLabel} from '../../components/StatusPill'
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
const label=(value:any,fallback=t('growth.none'))=>typeof value==='string'&&value?value:fallback

export function GrowthDashboard(){
 useLocale()
 const synergy=useRemote<Synergy>('/api/synergy',{agents:[],sharedMethods:[],algorithmStats:[]})
 const usage=useRemote<Usage>('/api/model-usage',{groups:[]})
 const workflowJobs=useRemote<any[]>('/api/jobs',[])
 const transfer=useRemote<Transfer>('/api/transfer',{principles:[],rules:[],usages:[],learningCases:[]})
 const policies=useRemote<Policy[]>('/api/policies',[])
 const searches=useRemote<any[]>('/api/policy-searches',[])
 const experiments=useRemote<Experiment[]>('/api/experiment-requests',[])
 const evaluations=useRemote<{items?:any[]}>('/api/algorithms/evaluations',{items:[]})
 const latestEval=(evaluations.value.items||[])[0]
 const agents=(synergy.value.agents||[]).filter(agent=>number(agent.growth?.runs)>0).sort((a,b)=>number(b.growth?.runs)-number(a.growth?.runs))
 const rated=agents.reduce((sum,agent)=>sum+number(agent.growth?.ratedRuns),0)
 const principles=transfer.value.principles||[]
 const usages=transfer.value.usages||[]
 const rules=transfer.value.rules||[]
 const activePrinciples=principles.filter(item=>item.status==='active').length
 const calls=number(usage.value.calls)
 const tokens=number(usage.value.totalTokens)
 const groups=(usage.value.groups||[]).slice(0,10)
 // Evidence from persisted job results, not a hypothetical learning gain.
 const verifiedSteps=workflowJobs.value.filter(job=>job.kind==='workflow')
  .flatMap(job=>(job.result?.outputs||[]).map((step:any)=>({
    agentId:step.agentId,jobId:job.id,status:step.verification?.status,
    calls:step.verification?.modelCalls,tokens:step.verification?.totalTokens,
    operations:step.verification?.operationIds?.length||0
  }))).filter((step:any)=>step.agentId&&step.status==='verified')
 const reviewSteps=workflowJobs.value.filter(job=>job.kind==='workflow'&&job.status==='needs_review')
  .map(job=>{const ix=job.result?.failedStepIndex
    const steps=job.workflowState?.definition?.steps||[]
    return {agentId:Number.isInteger(ix)?steps[ix]?.agentId:null,jobId:job.id}
  }).filter(step=>step.agentId)
 const workflowAgents=[...new Set([
   ...verifiedSteps.map(row=>row.agentId as string),
   ...reviewSteps.map(row=>row.agentId as string)
 ])].map(id=>{
   const passed=verifiedSteps.filter(row=>row.agentId===id)
   const blocked=reviewSteps.filter(row=>row.agentId===id)
   return {id,name:synergy.value.agents?.find(a=>a.id===id)?.name||id,
     passed:passed.length,blocked:blocked.length,
     calls:passed.reduce((n,row)=>n+number(row.calls),0),
     tokens:passed.reduce((n,row)=>n+number(row.tokens),0),
     operations:passed.reduce((n,row)=>n+number(row.operations),0),
     jobs:new Set([...passed,...blocked].map(row=>row.jobId)).size
   }
 }).sort((a,b)=>b.passed+b.blocked-a.passed-a.blocked).slice(0,12)
 return <div className="mt-3 grid gap-4">
  <Notice message={synergy.error||usage.error||transfer.error||policies.error||searches.error||experiments.error}/>
  <div className="grid grid-cols-2 gap-2 lg:grid-cols-4">
   <div className="rounded-xl border border-[#EFE9DF] bg-white p-3"><small className="text-[10px] font-semibold text-[#8A8176]">{t('growth.verifiedRuns')}</small><strong className="block text-lg">{rated}</strong></div>
   <div className="rounded-xl border border-[#EFE9DF] bg-white p-3"><small className="text-[10px] font-semibold text-[#8A8176]">{t('growth.activePrinciples')}</small><strong className="block text-lg">{activePrinciples}</strong></div>
   <div className="rounded-xl border border-[#EFE9DF] bg-white p-3"><small className="text-[10px] font-semibold text-[#8A8176]">{t('growth.sharedRules')}</small><strong className="block text-lg">{rules.length}</strong></div>
   <div className="rounded-xl border border-[#EFE9DF] bg-white p-3"><small className="text-[10px] font-semibold text-[#8A8176]">{t('growth.modelCalls')}</small><strong className="block text-lg">{calls}</strong><small className="text-[10px] text-[#9C9388]">{tokens.toLocaleString()} {t('growth.tokens')}</small></div>
  </div>

  <Panel className="min-w-0 p-4">
   <Subheading title={t('growth.observedGrowth')} extra={<span className="text-[10px] text-[#9C9388]">{t('ui.label.verifiedRuns')}</span>}/>
   {!agents.length?<p className="text-sm text-[#7C7469]">{t('growth.noRuns')}</p>:<div className="grid gap-2">{agents.slice(0,10).map(agent=>{
    const growth=agent.growth||{}
    const failures=Object.entries(growth.toolFailures||{}).sort((a,b)=>number(b[1])-number(a[1])).slice(0,3)
    const guidance=(growth.guidance||[])[0]
    return <div className="rounded-lg border border-[#EFE9DF] p-3" key={agent.id}>
     <div className="flex flex-wrap items-center gap-2"><strong className="text-sm">{label(agent.name,agent.id)}</strong><span className="text-[11px] text-[#7C7469]">{t('growth.runMeta',{runs:number(growth.runs),rated:number(growth.ratedRuns)})}</span><span className="ml-auto rounded bg-[#F7E8E1] px-1.5 py-0.5 text-[10px] font-semibold text-[#A94E30]">{percent(growth.successRate)}</span></div>
     {failures.length>0&&<p className="mt-1 text-[11px] text-[#8A8176]">{t('growth.failedTools',{tools:failures.map(([tool,count])=>`${tool} ${number(count)}`).join(' · ')})}</p>}
     {guidance?.text&&<p className="mt-1 text-[11px] text-[#5C544A]">{t('growth.guidance',{text:guidance.text})}</p>}
    </div>
   })}</div>}
  </Panel>

  <Panel className="min-w-0 p-4">
   <Subheading title="에이전트별 실제 워크플로 검증 기록" extra={<a href="#/workflows" className="text-[11px] font-semibold text-[#A94E30] underline">워크플로 열기</a>}/>
   <p className="mb-3 text-[11px] text-[#8A8176]">저장된 작업에서 실제 검증 통과한 단계와 검토 보류된 단계를 구분합니다. 실모델 지능 향상이나 비용 절감 실적을 뜻하지 않습니다.</p>
   {workflowAgents.length?<div className="grid gap-2">{workflowAgents.map(agent=><div key={agent.id} className="rounded-lg border border-[#EFE9DF] p-3 text-xs">
     <div className="flex flex-wrap items-center gap-2"><a href="#/agents" className="font-bold text-[#4A443C] underline">{agent.name}</a><span className="text-[#63745B]">검증 통과 {agent.passed}단계</span>{agent.blocked>0&&<span className="text-amber-800">검토 보류 {agent.blocked}단계</span>}</div>
     <p className="mt-1 text-[11px] text-[#8A8176]">도구 영수증 {agent.operations}개 · 모델 호출 {agent.calls}회 · 기록 토큰 {agent.tokens.toLocaleString()} · 실행 작업 {agent.jobs}건</p>
   </div>)}</div>:<p className="text-[11px] text-[#8A8176]">검증 조건이 있는 워크플로 작업 기록이 아직 없습니다.</p>}
  </Panel>

  <Panel className="min-w-0 p-4">
   <Subheading title={t('growth.modelUsage')} extra={<span className="text-[10px] text-[#9C9388]">{t('ui.label.apiLocal')}</span>}/>
   {!groups.length?<p className="text-sm text-[#7C7469]">{t('growth.noCalls')}</p>:<div className="grid gap-1 text-[11px]">{groups.map((group,index)=><div className="grid grid-cols-[1fr_auto_auto] gap-2 rounded-lg border border-[#EFE9DF] px-3 py-2" key={index}><span className="break-all font-semibold text-[#4A443C]">{label(group.model,t('growth.modelUnknown'))}</span><span className="text-[#7C7469]">{label(group.purpose,t('growth.purposeOther'))} · {t('growth.calls',{count:number(group.calls)})}</span><span className="text-[#7C7469]">{number(group.totalTokens).toLocaleString()} {t('growth.tokens')}</span></div>)}</div>}
  </Panel>

  <Panel className="min-w-0 p-4">
   <Subheading title={t('growth.transfer')} extra={<span className="text-[10px] text-[#9C9388]">{t('ui.label.provenance')}</span>}/>
   {!principles.length&&!rules.length?<p className="text-sm text-[#7C7469]">{t('growth.noPrinciples')}</p>:<div className="grid gap-2">
    {principles.slice(0,6).map(principle=><div className="rounded-lg border border-[#EFE9DF] p-3" key={principle.id}>
     <div className="flex flex-wrap gap-2 text-[11px]"><strong>{label(principle.kind,t('growth.principle'))}</strong><span className="text-[#7C7469]">{label(principle.status)} · {t('growth.confidence',{value:percent(principle.confidence)})}</span><span className="ml-auto rounded bg-[#F7E8E1] px-1.5 py-0.5 text-[10px] text-[#A94E30]">{t('growth.cases',{count:number((principle.supportingCases||[]).length)})}</span></div>
     <p className="mt-1 text-[12px] text-[#4A443C]">{label(principle.principle)}</p>
     <p className="mt-1 text-[10px] text-[#8A8176]">{t('growth.contributors',{models:(principle.supportingModels||[]).join(', ')||t('growth.none')})} · {t('growth.agentsCount',{count:(principle.supportingAgents||[]).length})} · {t('growth.counterexamples',{count:number((principle.counterexamples||[]).length)})}</p>
    </div>)}
    {rules.slice(0,6).map(rule=><div className="rounded-lg border border-[#EFE9DF] p-3" key={rule.id}>
     <div className="flex flex-wrap gap-2 text-[11px]"><strong>{t('growth.conditional')}</strong><span className="text-[#7C7469]">{label(rule.status)} · {t('growth.confidence',{value:percent(rule.confidence)})}</span><span className="ml-auto text-[10px] text-[#A94E30]">{t('growth.cases',{count:number((rule.supportingCases||[]).length)})}</span></div>
     <p className="mt-1 text-[12px] text-[#4A443C]">{label(rule.trigger)} → {label(rule.intervention?.tool,t('growth.tool'))}</p>
     <p className="mt-1 text-[10px] text-[#8A8176]">{t('growth.agentsCount',{count:(rule.supportingAgents||[]).length})} · {t('growth.counterexamples',{count:number((rule.counterexamples||[]).length)})}</p>
    </div>)}
   </div>}
  </Panel>

  <Panel className="min-w-0 p-4">
   <Subheading title={t('growth.usageLog')} extra={<span className="text-[10px] text-[#9C9388]">{usages.length}</span>}/>
   {!usages.length?<p className="text-sm text-[#7C7469]">{t('growth.none')}</p>:<div className="grid gap-1">{usages.slice(0,8).map((usage:any)=><div className="rounded-lg border border-[#EFE9DF] px-3 py-2 text-[11px]" key={usage.id}>
    <div className="flex flex-wrap items-center gap-2"><strong className="min-w-0 flex-1 truncate">{label(usage.principle,t('growth.principle'))}</strong><span className="rounded bg-[#F7E8E1] px-1.5 py-0.5 text-[10px] text-[#A94E30]">{statusLabel(usage.outcome||usage.status)||t('growth.none')}</span></div>
    <p className="mt-1 text-[10px] text-[#8A8176]">{t('growth.usageReused',{model:label(usage.model,t('growth.modelUnknown'))})} · {t('growth.usageContributed',{models:(usage.contributingModels||[]).join(', ')||t('growth.none')})} · {t('growth.usageAgent',{agent:label(usage.agentId,t('growth.none'))})}</p>
    <p className="text-[10px] text-[#9C9388]">{formatDateTime(usage.execution?.startedAt||usage.createdAt)}</p>
   </div>)}</div>}
  </Panel>

  <Panel className="min-w-0 p-4">
   <Subheading title={t('growth.experiments')} extra={<span className="text-[10px] text-[#9C9388]">{t('ui.label.evidenceGate')}</span>}/>
   <div className="grid gap-1 text-[11px]">
    {policies.value.slice(0,6).map(policy=><div className="flex flex-wrap items-center gap-2 rounded-lg border border-[#EFE9DF] px-3 py-2" key={policy.id}><strong>{label(policy.slot,t('growth.policy'))}</strong><span className="text-[#7C7469]">{label(policy.name)}</span><span className="ml-auto">{label(policy.status)} · {label(policy.evidenceGrade,'E0')}</span></div>)}
    {experiments.value.slice(0,6).map(experiment=><div className="flex flex-wrap items-center gap-2 rounded-lg border border-[#EFE9DF] px-3 py-2" key={experiment.id}><strong>{t('growth.experiment')}</strong><span className="text-[#7C7469]">{label(experiment.fixture,t('growth.noFixture'))} · {label(experiment.strategyId)}</span><span className="ml-auto">{label(experiment.status)}{experiment.error?' · '+experiment.error:''}</span></div>)}
    <div className="text-[10px] text-[#9C9388]">{t('growth.policySearches',{searches:searches.value.length,experiments:experiments.value.length})}</div>
    <div className="mt-2 border-t border-[#EFE9DF] pt-2"><div className="flex items-center justify-between"><strong className="text-[11px]">{t('growth.evalTitle')}</strong><span className="text-[10px] text-[#9C9388]">{latestEval?formatDateTime(latestEval.generatedAt):''}</span></div>
     {!latestEval?<p className="mt-1 text-[10px] text-[#9C9388]">{t('growth.evalNone')}</p>:<div className="mt-1 grid gap-1">{((latestEval.dimensions||[]) as any[]).map((dimension:any)=><div className="flex flex-wrap items-center gap-2 text-[10px]" key={dimension.kind}><span className="font-semibold text-[#4A443C]">{dimension.kind}</span><span className="text-[#7C7469]">{t('growth.evalLine',{kind:dimension.kind,active:dimension.active||dimension.baseline,winner:dimension.winner,recommendation:t(('growth.recommend.'+(dimension.recommendation||'keep')) as any)})}</span></div>)}</div>}
    </div>
   </div>
  </Panel>
 </div>
}
