import {t,useLocale} from '../../i18n'
import {useRemote,Notice,Panel,Subheading} from '../../components/UI'

type AgentGrowth={runs?:number;ratedRuns?:number;succeeded?:number;failed?:number;successRate?:number}
type Agent={id?:string;name?:string;growth?:AgentGrowth}
type TokenStats={calls?:number;totalTokens?:number;promptTokens?:number;completionTokens?:number;
 cachedPromptTokens?:number;uncachedPromptTokens?:number;cacheUsageReported?:boolean;cacheUnreportedCalls?:number}
type Usage=TokenStats&{groups?:Array<TokenStats&{model?:string;purpose?:string}>}
type Repair={id:string;principle?:string;failureMode?:string;repository?:string;evidenceGrade?:string;files?:string[]}
const number=(value?:number)=>typeof value==='number'?value:0
const label=(value?:string)=>value||t('growth.none')

export function GrowthDashboard(){
 useLocale()
 const synergy=useRemote<{agents:Agent[]}>('/api/synergy',{agents:[]})
 const usage=useRemote<Usage>('/api/model-usage',{groups:[]})
 const transfer=useRemote<{principles:Repair[]}>('/api/transfer',{principles:[]})
 const agents=synergy.value.agents.filter(agent=>number(agent.growth?.runs)>0)
  .sort((a,b)=>number(b.growth?.runs)-number(a.growth?.runs))
 const repairs=transfer.value.principles
 const rated=agents.reduce((count,agent)=>count+number(agent.growth?.ratedRuns),0)
 const calls=number(usage.value.calls)
 return <div className="mt-3 grid gap-4">
  <Notice message={synergy.error||usage.error||transfer.error}/>
  <div className="grid grid-cols-2 gap-2 lg:grid-cols-3">
   <div className="rounded-xl border border-[#EFE9DF] bg-white p-3"><small className="text-[10px] font-semibold text-[#8A8176]">{t('growth.verifiedRuns')}</small><strong className="block text-lg">{rated}</strong></div>
   <div className="rounded-xl border border-[#EFE9DF] bg-white p-3"><small className="text-[10px] font-semibold text-[#8A8176]">검증된 복구 기록</small><strong className="block text-lg">{repairs.length}</strong></div>
   <div className="rounded-xl border border-[#EFE9DF] bg-white p-3"><small className="text-[10px] font-semibold text-[#8A8176]">{t('growth.modelCalls')}</small><strong className="block text-lg">{calls}</strong><small className="block text-[10px] text-[#9C9388]">{number(usage.value.totalTokens).toLocaleString()} {t('growth.tokens')}
   <span className="block">{usage.value.cacheUsageReported===true?
    t('usage.cached',{count:number(usage.value.cachedPromptTokens).toLocaleString()}):
    t('usage.cacheUnknown')}</span></small></div>
  </div>
  <Panel className="min-w-0 p-4">
   <Subheading title={t('growth.observedGrowth')}/>
   {!agents.length?<p className="text-sm text-[#7C7469]">{t('growth.noRuns')}</p>:
    <div className="grid gap-2">{agents.slice(0,10).map(agent=>
     <div className="rounded-lg border border-[#EFE9DF] p-3" key={agent.id}>
      <div className="flex flex-wrap items-center gap-2"><strong className="text-sm">{label(agent.name||agent.id)}</strong><span className="text-xs text-[#7C7469]">{t('growth.runMeta',{runs:number(agent.growth?.runs),rated:number(agent.growth?.ratedRuns)})}</span><span className="ml-auto text-xs font-semibold text-[#A94E30]">{Math.round(number(agent.growth?.successRate)*100)}%</span></div>
     </div>)}</div>}
  </Panel>
  <Panel className="min-w-0 p-4">
   <Subheading title={t('growth.modelUsage')}/>
   {!usage.value.groups?.length?<p className="text-sm text-[#7C7469]">{t('growth.noCalls')}</p>:
    <div className="grid gap-1">{usage.value.groups.slice(0,10).map((group,index)=>
     <div className="grid grid-cols-[1fr_auto_auto] gap-2 rounded-lg border border-[#EFE9DF] px-3 py-2 text-xs" key={index}>
      <span className="break-all font-semibold">{label(group.model)}</span>
      <span>{t('growth.calls',{count:number(group.calls)})}</span>
      <span className="text-right">{number(group.totalTokens).toLocaleString()} {t('growth.tokens')}
       <small className="block text-[10px] text-[#8A8176]">{group.cacheUsageReported===true?
        t('usage.cached',{count:number(group.cachedPromptTokens).toLocaleString()}):
        t('usage.cacheUnknown')}</small>
      </span>
     </div>)}</div>}
  </Panel>
  <Panel className="min-w-0 p-4">
   <Subheading title="검증된 실행 복구 사례"/>
   {!repairs.length?<p className="text-sm text-[#7C7469]">저장된 검증 사례가 없습니다.</p>:
    <div className="grid gap-2">{repairs.slice(0,8).map(repair=>
     <div className="rounded-lg border border-[#EFE9DF] p-3" key={repair.id}>
      <p className="text-xs font-semibold text-[#4A443C]">{label(repair.principle)}</p>
      {repair.failureMode&&<p className="mt-1 text-xs text-[#7C7469]">{repair.failureMode}</p>}
      <p className="mt-1 text-[10px] text-[#8A8176]">{label(repair.repository)} · {label(repair.evidenceGrade)} · {repair.files?.join(', ')||''}</p>
     </div>)}</div>}
  </Panel>
 </div>
}
