import {useEffect,useState} from 'react'
import {BookOpenText, Check, Pencil, Plus, Trash2} from 'lucide-react'
import {del, post, type Agent} from '../api'
import {Button, TextInput, TextArea, SelectInput} from '../components/Primitives'
import {Action, FormField, Headline, Notice, Panel, Subheading, useRemote} from '../components/UI'
import {LearningRecords} from './memory/LearningRecords'
import {t,useLocale,type LocaleKey} from '../i18n'
import {GrowthDashboard} from './memory/Growth'

type Memory={
 id:string;text:string;scope?:string;agentId?:string;sessionId?:string;partitionKey?:string;
 status?:string;retrievalEligible:boolean;source?:string;createdAt?:string;lastUsedAt?:string;
 importance?:number;verification?:string;sourceMessageIds?:string[];evidenceQuote?:string;
 forgetReason?:string;archiveReason?:string;expiresAt?:string;
}
const statusLabels:Record<string,LocaleKey>={active:'memory.status.active',archived:'memory.status.archived',superseded:'memory.status.superseded',deleted:'memory.status.deleted',draft:'memory.status.draft',excluded:'memory.status.excluded'}
const sourceLabels:Record<string,LocaleKey>={user:'memory.source.user',learn:'memory.source.learn',agent:'memory.source.agent',reflect:'memory.source.reflect',compression:'memory.source.compression'}
const displayTime=(time?:string)=>time&&Number.isFinite(Date.parse(time))?new Date(time).toLocaleString():t('memory.time.none')
const statusOf=(memory:Memory)=>memory.retrievalEligible?'active':memory.status&&memory.status!=='active'?memory.status:'excluded'

export function MemoryPage(){
 useLocale()
 const memories=useRemote<Memory[]>('/api/memories',[])
 const agents=useRemote<Agent[]>('/api/agents',[])
 const [query,setQuery]=useState(''),[text,setText]=useState(''),[scope,setScope]=useState('shared')
 const [status,setStatus]=useState('active'),[owner,setOwner]=useState('all')
 const [error,setError]=useState(''),[notice,setNotice]=useState(''),[busy,setBusy]=useState(false)
 const [showRecords,setShowRecords]=useState(false)
 const [editingId,setEditingId]=useState(''),[editingText,setEditingText]=useState('')
 const [visibleLimit,setVisibleLimit]=useState(40)
 useEffect(()=>setVisibleLimit(40),[query,status,owner])
 const agentNames=new Map(agents.value.map(agent=>[agent.id,agent.name]))
 const sessionScoped=(m:Memory)=>m.scope==='session'||(m.scope!=='global'&&!m.partitionKey&&['learn','agent'].includes(m.source??''))
 const agentOf=(m:Memory)=>{
  if(!sessionScoped(m)&&(!m.scope||['shared','global'].includes(m.scope)))return undefined
  return m.agentId??(!['shared','global','session','agent'].includes(m.scope??'shared')?m.scope:undefined)
 }
 const ownerName=(m:Memory)=>{
  const agent=agentOf(m)
  if(sessionScoped(m))return agent?t('memory.scopeLabel.session')+' · '+(agentNames.get(agent)??agent):t('memory.scopeLabel.session')
  if(m.scope==='global')return t('memory.scopeLabel.workspace')
  if(!m.scope||m.scope==='shared')return t('memory.scopeLabel.shared')
  return agent?agentNames.get(agent)??agent:t('memory.scopeLabel.unknown')
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
   setText('');setNotice(t('memory.saved'));setStatus('active');setOwner('all');await memories.reload()
  }catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 const beginEdit=(memory:Memory)=>{
  setEditingId(memory.id);setEditingText(memory.text);setError('');setNotice('')
 }
 const update=async()=>{
  const record=memories.value.find(memory=>memory.id===editingId)
  if(!record||!editingText.trim()||busy)return
  setBusy(true);setError('');setNotice('')
  try{
   await post('/api/memories/'+encodeURIComponent(record.id)+'/edit',{
    text:editingText.trim(),expectedText:record.text
   })
   setEditingId('');setEditingText('');setNotice(t('memory.updated'))
   await memories.reload()
  }catch(e){setError((e as Error).message);await memories.reload()}
  finally{setBusy(false)}
 }
 const drop=async(id:string)=>{
  if(busy||!confirm(t('memory.delete')+'?'))return
  setBusy(true);setError('');setNotice('')
  try{await del('/api/memories/'+encodeURIComponent(id));await memories.reload()}catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 return <div className="mx-auto w-full max-w-[1320px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
  <Headline eyebrow={t('ui.eyebrow.memory')} title={t('memory.title')} description={t('memory.description')}/>
  <Notice message={error||memories.error||agents.error}/>
  {notice&&<div role="status" className="my-3 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-700"><Check size={16}/>{notice}</div>}
  <div className="grid grid-cols-1 items-start gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)]">
   <Panel className="min-w-0 p-5">
    <Subheading title={t('memory.new')} extra={<Action disabled={busy||!text.trim()} onClick={save}><Plus size={16}/> {t('memory.save')}</Action>}/>
    <FormField label={t('memory.content')}><TextArea rows={6} value={text} onChange={e=>setText(e.target.value)}/></FormField>
    <FormField label={t('memory.scope')}><SelectInput value={scope} onChange={e=>setScope(e.target.value)}><option value="shared">{t('memory.scope.shared')}</option>{agents.value.map(a=><option key={a.id} value={a.id}>{a.name}</option>)}</SelectInput></FormField>
    <p className="mt-4 text-sm text-[#7C7469]">{t('memory.note')}</p>
   </Panel>
   <div>
    <Subheading title={t('memory.count',{count:filtered.length})}/>
    <div className="mb-4 grid gap-2 sm:grid-cols-2">
     <TextInput className="sm:col-span-2" aria-label={t('memory.searchAria')} value={query} onChange={e=>setQuery(e.target.value)} placeholder={t('memory.search')}/>
     <SelectInput aria-label={t('memory.statusAria')} value={status} onChange={e=>setStatus(e.target.value)}><option value="all">{t('memory.status.all')}</option>{Object.entries(statusLabels).map(([key,label])=><option key={key} value={key}>{t(label)}</option>)}</SelectInput>
     <SelectInput aria-label={t('memory.scopeAria')} value={owner} onChange={e=>setOwner(e.target.value)}><option value="all">{t('memory.owner.all')}</option><option value="shared">{t('memory.owner.shared')}</option><option value="session">{t('memory.owner.session')}</option>{owners.map(id=><option key={id} value={id}>{agentNames.get(id)??id}</option>)}</SelectInput>
    </div>
    <div className="flex flex-col gap-3">{filtered.slice(0,visibleLimit).map(m=><Panel key={m.id} className="min-w-0 p-4">
     <div className="mb-2 flex flex-wrap items-center gap-2"><BookOpenText size={18}/><strong>{ownerName(m)}</strong><span className="text-xs text-[#7C7469]">{t(statusLabels[statusOf(m)] ?? 'memory.status.excluded')}</span><div className="ml-auto flex items-center gap-1">
      {m.source==='user'&&statusOf(m)==='active'&&<Button size="icon" variant="ghost" disabled={busy} onClick={()=>beginEdit(m)} aria-label={t('memory.edit')}><Pencil size={16}/></Button>}
      <Button size="icon" variant="ghost" disabled={busy} onClick={()=>void drop(m.id)} aria-label={t('memory.delete')}><Trash2 size={16}/></Button>
     </div></div>
     {editingId===m.id?<div className="grid gap-2">
      <TextArea rows={5} aria-label={t('memory.content')} value={editingText} onChange={event=>setEditingText(event.target.value)} disabled={busy}/>
      <div className="flex flex-wrap gap-2">
       <Button size="sm" disabled={busy||!editingText.trim()||editingText.trim()===m.text} onClick={()=>void update()}>{t('memory.update')}</Button>
       <Button size="sm" variant="ghost" disabled={busy} onClick={()=>{setEditingId('');setEditingText('')}}>{t('memory.cancel')}</Button>
      </div>
     </div>:<p className="whitespace-pre-wrap">{m.text}</p>}
     <div className="mt-2 flex flex-wrap gap-x-3 text-xs text-[#7C7469]"><span>{t('memory.source')} {sourceLabels[m.source??'user']?t(sourceLabels[m.source??'user'] as LocaleKey):m.source}</span><span>{t('memory.importance')} {typeof m.importance==='number'?m.importance.toFixed(2):t('memory.time.none')}</span><span>{t('memory.lastUsed')} {displayTime(m.lastUsedAt)}</span></div>
     <details className="mt-2 text-xs text-[#5C544A]"><summary className="cursor-pointer">{t('memory.evidence')}</summary><div className="mt-2 grid gap-1 break-all"><span>{t('memory.created')} {displayTime(m.createdAt)}</span><span>{t('memory.verification')} {m.verification==='source_quote'?t('memory.verification.sourceQuote'):m.verification||t('memory.time.none')}</span>{m.partitionKey&&<span>{t('memory.workspace')} {m.partitionKey}</span>}{m.sessionId&&<span>{t('memory.sessionId')} {m.sessionId}</span>}{m.sourceMessageIds?.length?<span>{t('memory.sourceMessages')} {m.sourceMessageIds.join(', ')}</span>:null}{m.evidenceQuote&&<blockquote>{m.evidenceQuote}</blockquote>}{m.archiveReason||m.forgetReason?<span>{t('memory.archiveReason')} {m.archiveReason??m.forgetReason}</span>:null}{m.expiresAt&&<span>{t('memory.expires')} {displayTime(m.expiresAt)}</span>}</div></details>
    </Panel>)}</div>
    {filtered.length>visibleLimit&&<Button variant="secondary" className="w-full" onClick={()=>setVisibleLimit(value=>value+40)}>{t('memory.showMore',{count:filtered.length-visibleLimit})}</Button>}
    {!filtered.length&&<Panel className="p-6 text-center text-sm text-[#7C7469]">{memories.loading?t('memory.loading'):t('memory.empty')}</Panel>}
   </div>
  </div>
  <details className="mt-6 rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] p-4" onToggle={event=>setShowRecords(event.currentTarget.open)}>
   <summary className="cursor-pointer font-semibold">{t('memory.records')}</summary>
   {showRecords&&<LearningRecords/>}
  </details>
  <details className="mt-3 rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] p-4">
   <summary className="cursor-pointer font-semibold">{t('memory.growth')}</summary>
   <GrowthDashboard/>
  </details>
 </div>
}
