import {Activity,ArrowRight,Bot,BrainCircuit,CheckCircle2,Cpu,FolderOpen,MessageSquare,Play,RadioTower,Workflow} from 'lucide-react'
import {Button,PageContainer} from '../components/Primitives'
import {ExecutionMonitor} from '../components/ExecutionMonitor'
import {useRemote} from '../components/UI'
import {StatusPill} from '../components/StatusPill'
import {t,useLocale,formatDateTime,jobKindLabel} from '../i18n'
import type {Agent,ChatSession,Provider} from '../api'
type Destination='chat'|'models'|'agents'|'rooms'|'connections'|'workflows'|'memory'|'activity'|'rsi'
export function HomePage({navigate}:{navigate:(page:Destination)=>void}){
 useLocale()
 const sessions=useRemote<ChatSession[]>('/api/sessions',[])
 const providers=useRemote<Provider[]>('/api/providers',[])
 const agents=useRemote<Agent[]>('/api/agents',[])
 const jobs=useRemote<any[]>('/api/jobs',[])
 const channels=useRemote<any[]>('/api/channels',[])
 const runtime=useRemote<any>('/api/model-library/runtime',{status:'idle'})
 const enabledProviders=providers.value.filter(p=>p.enabled)
 const authenticatedProviders=enabledProviders.filter((p:any)=>p.hasKey||p.local).length
 const recent=[...jobs.value].sort((a,b)=>String(b.createdAt||'').localeCompare(String(a.createdAt||''))).slice(0,6)
 const busy=jobs.value.filter(j=>['queued','running','planning','reviewing','repairing'].includes(j.status)).length
 const metrics=[
  {name:'dashboard.metric.chat',value:sessions.value.length,icon:MessageSquare,page:'chat' as const},
  {name:'dashboard.metric.providers',value:providers.value.filter(p=>p.enabled).length,icon:Cpu,page:'models' as const},
  {name:'dashboard.metric.agents',value:agents.value.length,icon:Bot,page:'agents' as const},
  {name:'dashboard.metric.jobs',value:busy,icon:Activity,page:'activity' as const}
 ]
 const shortcuts=[
  {title:'dashboard.shortcut.chat',desc:'dashboard.shortcut.chat.desc',icon:MessageSquare,page:'chat' as const},
  {title:'dashboard.shortcut.models',desc:'dashboard.shortcut.models.desc',icon:Cpu,page:'models' as const},
  {title:'dashboard.shortcut.workflows',desc:'dashboard.shortcut.workflows.desc',icon:Play,page:'workflows' as const},
  {title:'dashboard.shortcut.agents',desc:'dashboard.shortcut.agents.desc',icon:Bot,page:'agents' as const},
  {title:'nav.rsi',desc:'dashboard.shortcut.rsi.desc',icon:BrainCircuit,page:'rsi' as const}
 ]
 return <PageContainer className="!max-w-[1200px] !py-5">
  <div className="mb-5 flex flex-wrap items-end justify-between gap-3"><div><h1 className="m-0 text-[19px] font-bold tracking-tight">{t('dashboard.title')}</h1><p className="mt-1 text-[12px] text-[#7C7469]">{t('dashboard.subtitle')}</p></div><Button type="button" variant="primary" onClick={()=>navigate('chat')} className="flex h-8 items-center gap-2 rounded-lg px-3 text-[11px] font-semibold"><MessageSquare size={14}/> {t('dashboard.openChat')} <ArrowRight size={13}/></Button></div>
  <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">{metrics.map(m=>{const Icon=m.icon;return <Button type="button" key={m.name} onClick={()=>navigate(m.page)} className="flex min-h-24 flex-col items-start rounded-xl border border-[#E7E0D5] bg-white p-4 text-left transition-colors hover:border-[#D8CFC2]"><span className="flex w-full items-center justify-between text-[11px] text-[#7C7469]">{t(m.name as any)}<Icon size={16} className="text-[#A79C8E]"/></span><strong className="mt-4 text-[23px] font-bold leading-none tabular-nums text-[#24211D]">{m.value}</strong></Button>})}</div>
  <div className="mt-5 grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(270px,.72fr)]">
   <section className="rounded-xl border border-[#E7E0D5] bg-white">
    <div className="border-b border-[#F0EAE1] px-4 py-3"><h2 className="text-[13px] font-bold">{t('dashboard.runNow')}</h2><p className="mt-1 text-[11px] text-[#8A8176]">{t('dashboard.runNow.desc')}</p></div>
    <div className="grid grid-cols-1 gap-0 sm:grid-cols-2">{shortcuts.map(s=>{const Icon=s.icon;return <Button type="button" key={s.title} onClick={()=>navigate(s.page)} className="group flex items-center gap-3 border-b border-[#f0f2f5] px-4 py-4 text-left hover:bg-[#FAF7F1] sm:odd:border-r"><span className="grid size-9 place-items-center rounded-lg bg-[#FAF7F1] text-[#A94E30]"><Icon size={17}/></span><span className="min-w-0 flex-1"><span className="block text-[12px] font-semibold">{t(s.title as any)}</span><small className="mt-1 block text-[11px] text-[#9C9388]">{t(s.desc as any)}</small></span><ArrowRight size={14} className="text-[#D8CFC2] group-hover:text-[#C96442]"/></Button>})}</div>
   </section>
   <section className="rounded-xl border border-[#E7E0D5] bg-white">
    <div className="border-b border-[#F0EAE1] px-4 py-3"><h2 className="text-[13px] font-bold">{t('dashboard.connections')}</h2></div>
    <div className="divide-y divide-[#f0f2f5] px-4">
     <Button onClick={()=>navigate('models')} className="flex w-full items-center gap-3 py-3 text-left text-[12px]"><Cpu size={16} className="text-[#8A8176]"/><span className="flex-1">{t('dashboard.providers')}</span><span className="text-[11px] text-[#8A8176]">{t('dashboard.providers.count',{count:enabledProviders.length,ready:authenticatedProviders})}</span><ArrowRight size={13}/></Button>
     <Button onClick={()=>navigate('models')} className="flex w-full items-center gap-3 py-3 text-left text-[12px]"><Cpu size={16} className="text-[#8A8176]"/><span className="flex-1">{t('dashboard.local')}</span><span className="text-[11px] text-[#8A8176]">{runtime.value.status==='ready'?t('dashboard.local.ready',{engine:runtime.value.engine||'ready'}):t('dashboard.local.idle')}</span><ArrowRight size={13}/></Button>
     <Button onClick={()=>navigate('connections')} className="flex w-full items-center gap-3 py-3 text-left text-[12px]"><RadioTower size={16} className="text-[#8A8176]"/><span className="flex-1">{t('dashboard.channels')}</span><span className="text-[11px] text-[#8A8176]">{t('dashboard.channels.count',{count:channels.value.length})}</span><ArrowRight size={13}/></Button>
     <Button onClick={()=>navigate('agents')} className="flex w-full items-center gap-3 py-3 text-left text-[12px]"><Bot size={16} className="text-[#8A8176]"/><span className="flex-1">{t('dashboard.agents')}</span><span className="text-[11px] text-[#8A8176]">{t('dashboard.agents.count',{count:agents.value.length})}</span><ArrowRight size={13}/></Button>
    </div>
    <div className="m-4 mt-2 flex items-center gap-2 rounded-lg bg-[#F7F2EA] px-3 py-2 text-[11px] text-[#7C7469]"><CheckCircle2 size={14}/> {t('dashboard.note')}</div>
   </section>
  </div>
  <section className="mt-4 rounded-xl border border-[#E7E0D5] bg-white">
   <div className="flex items-center justify-between border-b border-[#F0EAE1] px-4 py-3"><h2 className="text-[13px] font-bold">{t('dashboard.recent')}</h2><Button type="button" onClick={()=>navigate('activity')} className="flex items-center gap-1 text-[11px] font-medium text-[#C96442]">{t('dashboard.recent.all')} <ArrowRight size={13}/></Button></div>
   {recent.length?<div className="divide-y divide-[#F0EAE1]">{recent.map(job=><div key={job.id} className="flex items-center gap-3 px-4 py-3"><Workflow size={15} className="text-[#A79C8E]"/><div className="min-w-0 flex-1"><div className="truncate text-[12px] font-semibold">{job.input?.goal||job.input?.prompt||job.kind||t('dashboard.jobFallback')}</div><div className="mt-1 text-[10px] text-[#9C9388]">{jobKindLabel(job.kind)} · {job.createdAt?formatDateTime(job.createdAt):''}</div></div><StatusPill status={job.status} /></div>)}</div>:<div className="flex h-32 items-center justify-center text-[12px] text-[#9C9388]">{t('dashboard.recent.empty')}</div>}
  </section>
  <div className="mt-4"><ExecutionMonitor/></div>
 </PageContainer>
}
