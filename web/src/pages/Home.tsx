import {Activity,ArrowRight,Bot,CheckCircle2,Cpu,FolderOpen,MessageSquare,Play,RadioTower,Workflow} from 'lucide-react'
import {Button,PageContainer} from '../components/Primitives'
import {ExecutionMonitor} from '../components/ExecutionMonitor'
import {useRemote} from '../components/UI'
import {StatusPill} from '../components/StatusPill'
import type {Agent,ChatSession,Provider} from '../api'
type Destination='chat'|'models'|'agents'|'rooms'|'connections'|'workflows'|'memory'|'activity'
export function HomePage({navigate}:{navigate:(page:Destination)=>void}){
 const sessions=useRemote<ChatSession[]>('/api/sessions',[])
 const providers=useRemote<Provider[]>('/api/providers',[])
 const agents=useRemote<Agent[]>('/api/agents',[])
 const jobs=useRemote<any[]>('/api/jobs',[])
 const channels=useRemote<any[]>('/api/channels',[])
 const recent=[...jobs.value].sort((a,b)=>String(b.createdAt||'').localeCompare(String(a.createdAt||''))).slice(0,6)
 const busy=jobs.value.filter(j=>['queued','running','planning','reviewing','repairing'].includes(j.status)).length
 const metrics=[
  {name:'대화',value:sessions.value.length,icon:MessageSquare,page:'chat' as const},
  {name:'모델 제공자',value:providers.value.filter(p=>p.enabled).length,icon:Cpu,page:'models' as const},
  {name:'에이전트',value:agents.value.length,icon:Bot,page:'agents' as const},
  {name:'진행 중 작업',value:busy,icon:Activity,page:'activity' as const}
 ]
 const shortcuts=[
  {title:'채팅 시작',desc:'연결된 모델과 대화',icon:MessageSquare,page:'chat' as const},
  {title:'로컬 모델 가져오기',desc:'GGUF 등록과 로컬 모델 실행',icon:Cpu,page:'models' as const},
  {title:'반복 작업 열기',desc:'저장된 워크플로 실행',icon:Play,page:'workflows' as const},
  {title:'에이전트 관리',desc:'역할과 실행 모델 설정',icon:Bot,page:'agents' as const}
 ]
 return <PageContainer className="!max-w-[1200px] !py-5">
  <div className="mb-5 flex flex-wrap items-end justify-between gap-3"><div><h1 className="m-0 text-[19px] font-bold tracking-tight">대시보드</h1><p className="mt-1 text-[12px] text-[#8090a1]">모델, 에이전트, 작업을 한곳에서 관리합니다.</p></div><Button type="button" onClick={()=>navigate('chat')} className="flex h-8 items-center gap-2 rounded-lg bg-[#C96442] px-3 text-[11px] font-semibold text-white hover:bg-[#A94E30]"><MessageSquare size={14}/> 채팅 열기 <ArrowRight size={13}/></Button></div>
  <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">{metrics.map(m=>{const Icon=m.icon;return <Button type="button" key={m.name} onClick={()=>navigate(m.page)} className="flex min-h-24 flex-col items-start rounded-xl border border-[#e1e6ec] bg-white p-4 text-left transition-colors hover:border-[#a4b5e4]"><span className="flex w-full items-center justify-between text-[11px] text-[#7b8999]">{m.name}<Icon size={16} className="text-[#8595aa]"/></span><strong className="mt-4 text-[23px] font-bold leading-none tabular-nums text-[#263144]">{m.value}</strong></Button>})}</div>
  <div className="mt-5 grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(270px,.72fr)]">
   <section className="rounded-xl border border-[#e1e6ec] bg-white">
    <div className="border-b border-[#F0EAE1] px-4 py-3"><h2 className="text-[13px] font-bold">바로 실행</h2><p className="mt-1 text-[11px] text-[#8c98a7]">작업에 필요한 메뉴만 모았습니다.</p></div>
    <div className="grid grid-cols-1 gap-0 sm:grid-cols-2">{shortcuts.map(s=>{const Icon=s.icon;return <Button type="button" key={s.title} onClick={()=>navigate(s.page)} className="group flex items-center gap-3 border-b border-[#f0f2f5] px-4 py-4 text-left hover:bg-[#f8faff] sm:odd:border-r"><span className="grid size-9 place-items-center rounded-lg bg-[#eff2f9] text-[#6175a5]"><Icon size={17}/></span><span className="min-w-0 flex-1"><span className="block text-[12px] font-semibold">{s.title}</span><small className="mt-1 block text-[11px] text-[#8d9aa8]">{s.desc}</small></span><ArrowRight size={14} className="text-[#b0bac6] group-hover:text-[#C96442]"/></Button>})}</div>
   </section>
   <section className="rounded-xl border border-[#e1e6ec] bg-white">
    <div className="border-b border-[#F0EAE1] px-4 py-3"><h2 className="text-[13px] font-bold">연결 상태</h2></div>
    <div className="divide-y divide-[#f0f2f5] px-4">
     <Button onClick={()=>navigate('models')} className="flex w-full items-center gap-3 py-3 text-left text-[12px]"><Cpu size={16} className="text-[#7586a0]"/><span className="flex-1">모델 제공자</span><span className="text-[11px] text-[#7c8b9b]">{providers.value.filter(p=>p.enabled).length}개 활성</span><ArrowRight size={13}/></Button>
     <Button onClick={()=>navigate('connections')} className="flex w-full items-center gap-3 py-3 text-left text-[12px]"><RadioTower size={16} className="text-[#7586a0]"/><span className="flex-1">메신저 채널</span><span className="text-[11px] text-[#7c8b9b]">{channels.value.length}개 등록</span><ArrowRight size={13}/></Button>
     <Button onClick={()=>navigate('agents')} className="flex w-full items-center gap-3 py-3 text-left text-[12px]"><Bot size={16} className="text-[#7586a0]"/><span className="flex-1">에이전트</span><span className="text-[11px] text-[#7c8b9b]">{agents.value.length}개</span><ArrowRight size={13}/></Button>
    </div>
    <div className="m-4 mt-2 flex items-center gap-2 rounded-lg bg-[#f4f6f9] px-3 py-2 text-[11px] text-[#748196]"><CheckCircle2 size={14}/> 표시된 수치는 서버 저장 데이터를 반영합니다.</div>
   </section>
  </div>
  <section className="mt-4 rounded-xl border border-[#e1e6ec] bg-white">
   <div className="flex items-center justify-between border-b border-[#F0EAE1] px-4 py-3"><h2 className="text-[13px] font-bold">최근 작업</h2><Button type="button" onClick={()=>navigate('activity')} className="flex items-center gap-1 text-[11px] font-medium text-[#5672cd]">전체 보기 <ArrowRight size={13}/></Button></div>
   {recent.length?<div className="divide-y divide-[#F0EAE1]">{recent.map(job=><div key={job.id} className="flex items-center gap-3 px-4 py-3"><Workflow size={15} className="text-[#8794a5]"/><div className="min-w-0 flex-1"><div className="truncate text-[12px] font-semibold">{job.input?.goal||job.input?.prompt||job.kind||'작업'}</div><div className="mt-1 text-[10px] text-[#9aa6b3]">{job.kind} · {job.createdAt?new Date(job.createdAt).toLocaleString('ko-KR'):''}</div></div><StatusPill status={job.status}>{job.status}</StatusPill></div>)}</div>:<div className="flex h-32 items-center justify-center text-[12px] text-[#98a4b1]">아직 실행 기록이 없습니다.</div>}
  </section>
  <div className="mt-4"><ExecutionMonitor/></div>
 </PageContainer>
}
