import {useEffect,useState} from 'react'
import {Activity,BrainCircuit,RefreshCw,Share2,TrendingUp,UsersRound} from 'lucide-react'
import {get} from '../api'
import {Button} from '../components/Primitives'
import {Notice,refresh} from '../components/UI'
import {PageToolbar} from '../components/PageToolbar'

type Growth = {
  agentId?:string
  runs?:number
  succeeded?:number
  failed?:number
  answeredUnverified?:number
  successRate?:number
  guidance?:{id:string;text:string;evidenceCount?:number;confidence?:number}[]
  toolFailures?:Record<string,number>
  failureClasses?:Record<string,number>
}
type AgentRow = {id:string;name?:string;growth:Growth}
type Stats = {
  id?:string
  agentId?:string
  kind?:string
  revision?:string
  uses?:number
  successes?:number
  failures?:number
  successRate?:number
}
type SharedMethod = {
  id?:string
  trigger?:string
  intervention?:{tool?:string;inputShape?:string}
  supportingAgents?:string[]
  supportingCases?:string[]
  counterexamples?:string[]
  confidence?:number
}
type Synergy = {agents?:AgentRow[];sharedMethods?:SharedMethod[];algorithmStats?:Stats[]}
type Algorithm = {id:string;kind:string;name:string;status:string;implementation?:string;revision?:string}
type AlgorithmPayload = {kinds:{kind:string;label:string;active?:Algorithm;algorithms:Algorithm[]}[]}

const pct = (value?:number) => `${Math.round((value||0)*100)}%`

export function IntelligenceDashboard(){
  const [synergy,setSynergy]=useState<Synergy>({})
  const [algorithms,setAlgorithms]=useState<AlgorithmPayload>({kinds:[]})
  const [selected,setSelected]=useState('')
  const [error,setError]=useState('')
  const [busy,setBusy]=useState(false)

  const load=async()=>{
    setBusy(true);setError('')
    try{
      const [s,a]=await Promise.all([get<Synergy>('/api/synergy'),get<AlgorithmPayload>('/api/algorithms')])
      setSynergy(s);setAlgorithms(a)
      if(!selected&&s.agents?.length)setSelected(s.agents[0].id)
    }catch(e){setError((e as Error).message)}finally{setBusy(false)}
  }
  useEffect(()=>{void load()},[])

  const agents=synergy.agents||[]
  const agent=agents.find(item=>item.id===selected)||agents[0]
  const shared=synergy.sharedMethods||[]
  const stats=(synergy.algorithmStats||[]).filter(item=>item.agentId===agent?.id||item.agentId==='global')
  const totalRuns=agents.reduce((sum,item)=>sum+(item.growth?.runs||0),0)
  const totalSuccess=agents.reduce((sum,item)=>sum+(item.growth?.succeeded||0),0)
  const activeCount=algorithms.kinds.filter(group=>group.active).length

  return <div className="mx-auto w-full max-w-[1200px] px-4 py-5 sm:px-6">
    <PageToolbar>
      <strong className="page-toolbar-title">지능 대시보드</strong>
      <Button size="sm" variant="ghost" onClick={()=>void load()} disabled={busy}><RefreshCw size={15} className={busy?'animate-spin':''}/>새로고침</Button>
    </PageToolbar>

    <div className="mb-5 grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
      <Metric icon={<UsersRound size={18}/>} label="봇" value={String(agents.length)} note="개별 성장 기록"/>
      <Metric icon={<Activity size={18}/>} label="총 실행" value={String(totalRuns)} note={`성공 ${totalSuccess}`}/>
      <Metric icon={<TrendingUp size={18}/>} label="봇 성공률" value={totalRuns?pct(totalSuccess/totalRuns):'—'} note="전체 봇 누적"/>
      <Metric icon={<Share2 size={18}/>} label="공유 지식" value={String(shared.length)} note="2개 이상 봇 검증"/>
    </div>

    <Notice message={error}/>

    <div className="grid gap-4 lg:grid-cols-[minmax(0,1.1fr)_minmax(0,1fr)]">
      <section className="rounded-2xl border border-[#E7E0D5] bg-white p-4">
        <h2 className="mb-3 flex items-center gap-2 text-sm font-semibold text-[#24211D]"><UsersRound size={16}/>봇별 성장</h2>
        <div className="mb-3 flex flex-wrap gap-2">
          {agents.map(item=><button key={item.id} onClick={()=>setSelected(item.id)}
            className={`rounded-full border px-3 py-1 text-[11px] ${agent?.id===item.id?'border-[#C96442] bg-[#FFF3ED] text-[#A94E30]':'border-[#E7E0D5] text-[#6F675D]'}`}>
            {item.name||item.id}
          </button>)}
          {!agents.length&&<span className="text-xs text-[#9C9388]">아직 에이전트가 없습니다.</span>}
        </div>
        {agent&&<div>
          <div className="grid grid-cols-2 gap-2 text-xs sm:grid-cols-4">
            <Stat label="실행" value={String(agent.growth.runs||0)}/>
            <Stat label="성공" value={String(agent.growth.succeeded||0)}/>
            <Stat label="실패" value={String(agent.growth.failed||0)}/>
            <Stat label="성공률" value={pct(agent.growth.successRate)}/>
          </div>
          <div className="mt-3">
            <div className="mb-1 text-[11px] font-semibold text-[#6F675D]">축적된 지침</div>
            <div className="space-y-1.5">
              {(agent.growth.guidance||[]).map(item=><div key={item.id} className="rounded-lg bg-[#FFF8F2] px-3 py-2 text-[11px] text-[#4A443C]">
                {item.text}
                <span className="ml-2 text-[10px] text-[#9C9388]">근거 {item.evidenceCount||0} · 신뢰 {pct(item.confidence)}</span>
              </div>)}
              {!(agent.growth.guidance||[]).length&&<p className="text-[11px] text-[#9C9388]">아직 반복 패턴이 없습니다.</p>}
            </div>
          </div>
        </div>}
      </section>

      <section className="rounded-2xl border border-[#E7E0D5] bg-white p-4">
        <h2 className="mb-3 flex items-center gap-2 text-sm font-semibold text-[#24211D]"><Share2 size={16}/>에이전트 간 공유 지식</h2>
        <div className="space-y-2">
          {shared.slice(0,8).map(item=><article key={item.id} className="rounded-lg border border-[#EFE9DF] p-3 text-[11px]">
            <div className="font-medium text-[#24211D]">{item.trigger||'검증된 방법'}</div>
            <div className="mt-1 text-[#6F675D]">
              {item.intervention?.tool||'-'} · {item.intervention?.inputShape||'-'}
            </div>
            <div className="mt-1 text-[10px] text-[#9C9388]">
              지지 봇 {(item.supportingAgents||[]).length} · 사례 {(item.supportingCases||[]).length} · 반례 {(item.counterexamples||[]).length}
            </div>
          </article>)}
          {!shared.length&&<p className="text-[11px] text-[#9C9388]">아직 여러 봇이 독립적으로 검증한 공유 방법이 없습니다.</p>}
        </div>
      </section>
    </div>

    <div className="mt-4 grid gap-4 lg:grid-cols-2">
      <section className="rounded-2xl border border-[#E7E0D5] bg-white p-4">
        <h2 className="mb-3 flex items-center gap-2 text-sm font-semibold text-[#24211D]"><BrainCircuit size={16}/>알고리즘 영역</h2>
        <div className="grid gap-2 sm:grid-cols-2">
          {algorithms.kinds.map(group=><div key={group.kind} className="rounded-lg border border-[#EFE9DF] px-3 py-2 text-[11px]">
            <div className="text-[#9C9388]">{group.label}</div>
            <div className="font-medium text-[#24211D]">{group.active?.name||'기본값'}</div>
            <div className="text-[10px] text-[#9C9388]">{group.active?.implementation||'-'}</div>
          </div>)}
        </div>
        <p className="mt-2 text-[10px] text-[#9C9388]">활성 {activeCount}/{algorithms.kinds.length} 영역</p>
      </section>

      <section className="rounded-2xl border border-[#E7E0D5] bg-white p-4">
        <h2 className="mb-3 flex items-center gap-2 text-sm font-semibold text-[#24211D]"><TrendingUp size={16}/>알고리즘 성과</h2>
        <div className="overflow-x-auto">
          <table className="w-full min-w-[420px] border-collapse text-left text-[11px]">
            <thead><tr className="border-b border-[#EFE9DF] text-[#9C9388]"><th className="py-2 pr-2">영역</th><th className="py-2 pr-2">사용</th><th className="py-2 pr-2">성공률</th><th className="py-2">범위</th></tr></thead>
            <tbody>{stats.slice(0,12).map((item,index)=><tr key={(item.id||index)+''} className="border-b border-[#F3EEE5]">
              <td className="py-2 pr-2 text-[#24211D]">{item.kind}</td>
              <td className="py-2 pr-2">{item.uses||0}</td>
              <td className="py-2 pr-2">{pct(item.successRate)}</td>
              <td className="py-2">{item.agentId==='global'?'공통':'이 봇'}</td>
            </tr>)}</tbody>
          </table>
          {!stats.length&&<p className="py-3 text-[11px] text-[#9C9388]">아직 알고리즘 실행 성과가 없습니다.</p>}
        </div>
      </section>
    </div>
  </div>
}

function Metric({icon,label,value,note}:{icon:React.ReactNode;label:string;value:string;note:string}){
  return <div className="rounded-2xl border border-[#E7E0D5] bg-white p-4">
    <div className="flex items-center gap-2 text-[11px] text-[#9C9388]">{icon}{label}</div>
    <div className="mt-1 text-2xl font-semibold text-[#24211D]">{value}</div>
    <div className="text-[10px] text-[#9C9388]">{note}</div>
  </div>
}
function Stat({label,value}:{label:string;value:string}){
  return <div className="rounded-lg bg-[#F8F4EC] px-3 py-2">
    <div className="text-[10px] text-[#9C9388]">{label}</div>
    <div className="font-semibold text-[#24211D]">{value}</div>
  </div>
}
