import {formatDateTime} from '../../i18n'
import {useState} from 'react'
import {Button} from '../../components/Primitives'
import {Notice,useRemote} from '../../components/UI'

type Rec=Record<string,any>
type Row={key:string;title:string;subtitle?:string;tags:string[];facts:[string,string][];raw:Rec}
const sources=[
 {label:'경험',endpoint:'/api/experience'},
 {label:'개선안',endpoint:'/api/improvements'},
 {label:'알고리즘',endpoint:'/api/algorithms'},
 {label:'실험 요청',endpoint:'/api/experiment-requests'},
 {label:'개선 방식 평가',endpoint:'/api/optimizers'},
 {label:'알고리즘 평가',endpoint:'/api/algorithms/evaluate'},
] as const
const str=(value:any)=>typeof value==='string'?value:typeof value==='number'?String(value):''
const count=(value:any)=>typeof value==='number'?value:undefined
const list=(value:any)=>Array.isArray(value)?value.map(str).filter(Boolean):[]
const facts=(items:[string,string][])=>items.filter(([,value])=>!!value) as [string,string][]
const rate=(successes:any,uses:any)=>{
 const total=count(uses)??0
 if(!total)return ''
 const wins=count(successes)??0
 return `${Math.round(wins/total*100)}% (${wins}/${total})`
}
const when=(value:any)=>{
 const text=str(value)
 return text&&Number.isFinite(Date.parse(text))?formatDateTime(text):text
}
const fallback=(rec:Rec,index:number):Row=>({
 key:str(rec.id)||String(index),
 title:str(rec.name)||str(rec.title)||str(rec.text)||str(rec.lesson)||str(rec.id)||'기록',
 tags:[str(rec.status),str(rec.kind)].filter(Boolean),
 facts:Object.entries(rec).filter(([,value])=>['string','number','boolean'].includes(typeof value)).slice(0,8).map(([key,value])=>[key,String(value)] as [string,string]),
 raw:rec,
})
const experience=(rec:Rec,index:number):Row=>({
 key:str(rec.id)||`experience-${index}`,
 title:str(rec.lesson)||str(rec.request)||str(rec.id)||'경험',
 subtitle:str(rec.request)&&str(rec.request)!==str(rec.lesson)?str(rec.request):undefined,
 tags:[str(rec.outcome),str(rec.status),str(rec.scope)].filter(Boolean),
 facts:facts([
  ['피할 점',str(rec.avoid)],
  ['도구',list(rec.tools).join(', ')],
  ['발생',count(rec.occurrences)!==undefined?`${count(rec.occurrences)}회`:'' ],
  ['재사용',count(rec.uses)!==undefined?`${count(rec.uses)}회`:''],
  ['근거 메시지',String(list(rec.evidenceMessageIds).length||'')],
  ['최근 확인',when(rec.lastSeenAt)],
  ['기록 시각',when(rec.createdAt)],
 ]),
 raw:rec,
})
const improvement=(rec:Rec,index:number):Row=>({
 key:str(rec.id)||`improvement-${index}`,
 title:str(rec.text)||str(rec.rationale)||str(rec.id)||'개선안',
 tags:[str(rec.kind),str(rec.status),str(rec.evidenceGrade),str(rec.rollout)].filter(Boolean),
 facts:facts([
  ['근거',str(rec.rationale)],
  ['성공률',rate(rec.successes,rec.uses)],
  ['실패',count(rec.failures)!==undefined?`${count(rec.failures)}회`:''],
  ['기준 성공률',typeof rec.baselineSuccess==='number'?`${Math.round(rec.baselineSuccess*100)}%`:''],
  ['최근 관측',when(rec.lastObservedAt)],
  ['롤백 시각',when(rec.rolledBackAt)],
  ['사유',str(rec.reason)],
 ]),
 raw:rec,
})
const algorithm=(rec:Rec,index:number,kind:Rec):Row=>{
 const stats:Rec=rec.stats??{}
 const adopted=kind.active?.id&&kind.active.id===rec.id
 return {
  key:str(rec.id)||`algorithm-${index}`,
  title:str(rec.name)||str(rec.variant)||str(rec.id)||'알고리즘',
  subtitle:str(kind.label)||str(rec.kind),
  tags:[adopted?'채택됨':str(rec.status),str(rec.source),str(rec.implementation)].filter(Boolean),
  facts:facts([
   ['종류',str(rec.kind)],
   ['구현',str(rec.implementation)],
   ['성공률',rate(stats.successes,stats.uses)],
   ['실패',count(stats.failures)!==undefined?`${count(stats.failures)}회`:''],
   ['관측 에이전트',count(stats.agents)!==undefined?`${count(stats.agents)}명`:''],
   ['최근 사용',when(stats.lastUsedAt)],
   ['파라미터',rec.parameters?JSON.stringify(rec.parameters):''],
  ]),
  raw:{...rec,kindLabel:kind.label,adopted},
 }
}
const request=(rec:Rec,index:number):Row=>({
 key:str(rec.id)||`experiment-${index}`,
 title:str(rec.strategyId)||str(rec.id)||'실험 요청',
 subtitle:str(rec.revision),
 tags:[str(rec.status),str(rec.fixture)].filter(Boolean),
 facts:facts([
  ['최소 독립 단위',count(rec.minIndependentUnits)!==undefined?`${count(rec.minIndependentUnits)}개`:''],
  ['외부 실행기',rec.externalRunner===true?'사용':rec.externalRunner===false?'미사용':''],
  ['생성',when(rec.createdAt)],
  ['완료',when(rec.completedAt)],
  ['승격 상태',str(rec.promotionStatus)],
  ['오류',str(rec.promotionError)||str(rec.reason)],
 ]),
 raw:rec,
})
const optimizer=(rec:Rec,index:number):Row=>({
 key:str(rec.revision)||`optimizer-${index}`,
 title:str(rec.revision)||'최적화 방식',
 subtitle:rec.active===true?'현재 활성':undefined,
 tags:[rec.active===true?'활성':'후보'].filter(Boolean),
 facts:facts([
  ['검색',count(rec.searches)!==undefined?`${count(rec.searches)}회`:''],
  ['선택 성공',count(rec.selected)!==undefined?`${count(rec.selected)}회`:''],
  ['생성 후보',count(rec.generatedCandidates)!==undefined?`${count(rec.generatedCandidates)}개`:''],
  ['비용',typeof rec.cost==='number'?rec.cost.toFixed(3):''],
  ['점수',typeof rec.score==='number'?rec.score.toFixed(4):''],
 ]),
 raw:rec,
})
const percent=(value:any)=>typeof value==='number'?`${Math.round(value*100)}%`:''
const evaluation=(rec:Rec,index:number):Row=>{
 const kernels:Rec[]=Array.isArray(rec.kernels)?rec.kernels:[]
 const improvement=typeof rec.improvement==='number'?rec.improvement:0
 return {
  key:str(rec.kind)||`evaluation-${index}`,
  title:str(rec.label)||str(rec.kind)||'알고리즘 평가',
  subtitle:`${str(rec.baseline)} → ${str(rec.winner)}`,
  tags:[str(rec.kind),improvement>0?`+${Math.round(improvement*100)}%`:'개선 없음'].filter(Boolean),
  facts:facts([
   ['측정',str(rec.metric)],
   ['기준',`${str(rec.baseline)} ${percent(rec.baselineValue)}`],
   ['승자',`${str(rec.winner)} ${percent(rec.winnerValue)}`],
   ...kernels.map((kernel):[string,string]=>[str(kernel.kernel)||'커널',`${percent(kernel.value)}${typeof kernel.hits==='number'?` (${kernel.hits}/${kernel.total})`:''}`]),
  ]),
  raw:rec,
 }
}

const normalize=(source:number,data:any):Row[]=>{
 if(!data)return []
 if(source===2){
  const kinds:Rec[]=Array.isArray(data.kinds)?data.kinds:[]
  return kinds.flatMap((kind,kindIndex)=>(Array.isArray(kind.algorithms)?kind.algorithms:[]).map((rec:Rec,index:number)=>algorithm(rec,kindIndex*1000+index,kind)))
 }
 if(source===4){
  const rows:Rec[]=Array.isArray(data.optimizers)?data.optimizers:[]
  return rows.map(optimizer)
 }
 if(source===5){
  const rows:Rec[]=Array.isArray(data.dimensions)?data.dimensions:[]
  return rows.map(evaluation)
 }
 const records:Rec[]=Array.isArray(data)?data:Array.isArray(data.items)?data.items:null
 if(!records)return [fallback(data,0)]
 return records.map((rec,index)=>source===0?experience(rec,index):source===1?improvement(rec,index):source===3?request(rec,index):fallback(rec,index))
}

export function LearningRecords(){
 const [source,setSource]=useState(0)
 const data=useRemote<any>(sources[source].endpoint,null)
 const [limit,setLimit]=useState(20)
 const rows=normalize(source,data.value)
 const episodes=source===4&&typeof data.value?.episodes==='number'?data.value.episodes:undefined
 return <div className="mt-3 grid gap-2">
  <div className="flex flex-wrap gap-2">{sources.map((item,i)=><Button key={item.label} variant={source===i?'primary':'ghost'} onClick={()=>{setSource(i);setLimit(20)}}>{item.label}</Button>)}</div>
  <Notice message={data.error}/>
  {episodes!==undefined&&<p className="text-xs text-[#7C7469]">기록된 검색 에피소드 {episodes}건</p>}
  {data.loading?<p>기록을 불러오는 중…</p>:!data.error&&!rows.length?<p>저장된 기록이 없습니다.</p>:rows.slice(0,limit).map(row=><details key={row.key} className="rounded-lg border border-[#EFE9DF] p-2">
   <summary className="cursor-pointer text-sm"><strong>{row.title}</strong>{row.subtitle&&<span className="text-[#7C7469]"> · {row.subtitle}</span>}{row.tags.map(tag=><span key={tag} className="ml-2 rounded bg-[#F7E8E1] px-1.5 py-0.5 text-[10px] font-semibold text-[#A94E30]">{tag}</span>)}</summary>
   {row.facts.length>0&&<dl className="mt-2 grid grid-cols-1 gap-1 text-xs sm:grid-cols-[9rem_minmax(0,1fr)]">{row.facts.map(([label,value])=><div key={label} className="contents"><dt className="font-semibold text-[#7C7469]">{label}</dt><dd className="break-all text-[#4A443C]">{value}</dd></div>)}</dl>}
   <details className="mt-2 text-xs text-[#5C544A]"><summary className="cursor-pointer">원본 JSON</summary><pre className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-all">{JSON.stringify(row.raw,null,2)}</pre></details>
  </details>)}
  {rows.length>limit&&<Button variant="secondary" onClick={()=>setLimit(n=>n+20)}>더 보기</Button>}
 </div>
}
