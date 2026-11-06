import {useState} from 'react'
import {BookOpenText, Check, Plus, Trash2} from 'lucide-react'
import {del, post, type Agent} from '../api'
import {Button, TextInput, TextArea, SelectInput} from '../components/Primitives'
import {Action, FormField, Headline, Notice, Panel, Subheading, useRemote} from '../components/UI'
import {LearningRecords} from './memory/LearningRecords'

type Memory={
 id:string;text:string;scope?:string;agentId?:string;sessionId?:string;partitionKey?:string;
 status?:string;retrievalEligible:boolean;source?:string;createdAt?:string;lastUsedAt?:string;
 importance?:number;verification?:string;sourceMessageIds?:string[];evidenceQuote?:string;
 forgetReason?:string;expiresAt?:string;
}
const statusLabels:Record<string,string>={active:'활성',archived:'보관',superseded:'대체됨',deleted:'삭제됨',draft:'초안',excluded:'검색 제외'}
const sourceLabels:Record<string,string>={user:'직접 입력',learn:'학습',agent:'에이전트',reflect:'실행 검토',compression:'기억 압축'}
const displayTime=(time?:string)=>time&&Number.isFinite(Date.parse(time))?new Date(time).toLocaleString('ko-KR'):'기록 없음'
const statusOf=(memory:Memory)=>memory.retrievalEligible?'active':memory.status&&memory.status!=='active'?memory.status:'excluded'

export function MemoryPage(){
 const memories=useRemote<Memory[]>('/api/memories',[])
 const agents=useRemote<Agent[]>('/api/agents',[])
 const [query,setQuery]=useState(''),[text,setText]=useState(''),[scope,setScope]=useState('shared')
 const [status,setStatus]=useState('active'),[owner,setOwner]=useState('all')
 const [error,setError]=useState(''),[notice,setNotice]=useState(''),[busy,setBusy]=useState(false)
 const [showRecords,setShowRecords]=useState(false)
 const agentNames=new Map(agents.value.map(agent=>[agent.id,agent.name]))
 const sessionScoped=(m:Memory)=>m.scope==='session'||(m.scope!=='global'&&!m.partitionKey&&['learn','agent'].includes(m.source??''))
 const agentOf=(m:Memory)=>{
  if(!sessionScoped(m)&&(!m.scope||['shared','global'].includes(m.scope)))return undefined
  return m.agentId??(!['shared','global','session','agent'].includes(m.scope??'shared')?m.scope:undefined)
 }
 const ownerName=(m:Memory)=>{
  const agent=agentOf(m)
  if(sessionScoped(m))return agent?'특정 대화 · '+(agentNames.get(agent)??agent):'특정 대화'
  if(m.scope==='global')return '모든 작업 공간에 공유'
  if(!m.scope||m.scope==='shared')return '현재 작업 공간에 공유'
  return agent?agentNames.get(agent)??agent:'담당 에이전트 미기록'
 }
 const owners=[...new Set(memories.value.map(agentOf).filter((id):id is string=>!!id))]
 const filtered=memories.value.filter(m=>{
  const agent=agentOf(m)
  const matchesScope=owner==='all'||(owner==='session'?sessionScoped(m):owner==='shared'?!sessionScoped(m)&&(!m.scope||['shared','global'].includes(m.scope)):owner===agent)
  return (status==='all'||statusOf(m)===status)&&matchesScope
   &&[m.text,ownerName(m),m.source].join(' ').toLowerCase().includes(query.trim().toLowerCase())
 })
 const save=async()=>{
  if(!text.trim()||busy)return
  setBusy(true);setError('');setNotice('')
  try{
   await post('/api/memories',{text:text.trim(),status:'active',source:'user',...(scope==='shared'?{scope}:{scope:'agent',agentId:scope})})
   setText('');setNotice('기억을 저장했습니다.');setStatus('active');setOwner('all');await memories.reload()
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const drop=async(id:string)=>{
  if(busy||!confirm('이 기억을 삭제할까요?'))return
  setBusy(true);setError('');setNotice('')
  try{await del('/api/memories/'+encodeURIComponent(id));await memories.reload()}catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 return <div className="mx-auto w-full max-w-[1320px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
  <Headline eyebrow="MEMORY LIBRARY" title="기억과 학습 기록" description="검색에 사용할 수 있는 기억과 보관된 기록을 확인하세요. 활성 기억도 실행의 작업 공간, 에이전트, 대화 범위에 맞을 때만 검색됩니다."/>
  <Notice message={error||memories.error||agents.error}/>
  {notice&&<div role="status" className="my-3 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-700"><Check size={16}/>{notice}</div>}
  <div className="grid grid-cols-1 items-start gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)]">
   <Panel className="min-w-0 p-5">
    <Subheading title="새 기억 등록" extra={<Action disabled={busy||!text.trim()} onClick={save}><Plus size={16}/> 기억 저장</Action>}/>
    <FormField label="저장할 내용"><TextArea rows={6} value={text} onChange={e=>setText(e.target.value)}/></FormField>
    <FormField label="적용 범위"><SelectInput value={scope} onChange={e=>setScope(e.target.value)}><option value="shared">현재 작업 공간에 공유</option>{agents.value.map(a=><option key={a.id} value={a.id}>{a.name}</option>)}</SelectInput></FormField>
    <p className="mt-4 text-sm text-[#7C7469]">수동으로 저장한 기억과 학습으로 추출한 기억은 출처에서 구별할 수 있습니다.</p>
   </Panel>
   <div>
    <Subheading title={'기억 '+filtered.length+'건'}/>
    <div className="mb-4 grid gap-2 sm:grid-cols-2">
     <TextInput className="sm:col-span-2" aria-label="기억 검색" value={query} onChange={e=>setQuery(e.target.value)} placeholder="내용, 에이전트, 출처 검색"/>
     <SelectInput aria-label="기억 상태" value={status} onChange={e=>setStatus(e.target.value)}><option value="all">모든 상태</option>{Object.entries(statusLabels).map(([key,label])=><option key={key} value={key}>{label}</option>)}</SelectInput>
     <SelectInput aria-label="기억 범위" value={owner} onChange={e=>setOwner(e.target.value)}><option value="all">모든 범위</option><option value="shared">공유</option><option value="session">대화</option>{owners.map(id=><option key={id} value={id}>{agentNames.get(id)??id}</option>)}</SelectInput>
    </div>
    <div className="flex flex-col gap-3">{filtered.map(m=><Panel key={m.id} className="min-w-0 p-4">
     <div className="mb-2 flex flex-wrap items-center gap-2"><BookOpenText size={18}/><strong>{ownerName(m)}</strong><span className="text-xs text-[#7C7469]">{statusLabels[statusOf(m)]??statusOf(m)}</span><Button size="icon" variant="ghost" className="ml-auto" disabled={busy} onClick={()=>void drop(m.id)} aria-label="기억 삭제"><Trash2 size={16}/></Button></div>
     <p className="whitespace-pre-wrap">{m.text}</p>
     <div className="mt-2 flex flex-wrap gap-x-3 text-xs text-[#7C7469]"><span>출처 {sourceLabels[m.source??'user']??m.source}</span><span>중요도 {typeof m.importance==='number'?m.importance.toFixed(2):'미기록'}</span><span>마지막 검색 사용 {displayTime(m.lastUsedAt)}</span></div>
     <details className="mt-2 text-xs text-[#5C544A]"><summary className="cursor-pointer">근거와 적용 범위</summary><div className="mt-2 grid gap-1 break-all"><span>등록 {displayTime(m.createdAt)}</span><span>검증 {m.verification==='source_quote'?'원문 인용 근거 있음':m.verification||'기록 없음'}</span>{m.partitionKey&&<span>작업 공간 {m.partitionKey}</span>}{m.sessionId&&<span>대화 ID {m.sessionId}</span>}{m.sourceMessageIds?.length?<span>원문 메시지 {m.sourceMessageIds.join(', ')}</span>:null}{m.evidenceQuote&&<blockquote>{m.evidenceQuote}</blockquote>}{m.forgetReason&&<span>보관 사유 {m.forgetReason}</span>}{m.expiresAt&&<span>만료 {displayTime(m.expiresAt)}</span>}</div></details>
    </Panel>)}</div>
    {!filtered.length&&<Panel className="p-6 text-center text-sm text-[#7C7469]">{memories.loading?'기억을 불러오는 중…':'이 조건에 해당하는 기억이 없습니다.'}</Panel>}
   </div>
  </div>
  <details className="mt-6 rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] p-4" onToggle={event=>setShowRecords(event.currentTarget.open)}>
   <summary className="cursor-pointer font-semibold">경험, 개선안, 알고리즘과 실험 기록</summary>
   {showRecords&&<LearningRecords/>}
  </details>
 </div>
}
