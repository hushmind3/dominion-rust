import {useEffect,useMemo,useState} from 'react'
import {Activity,Blocks,BookOpenText,Bot,BrainCircuit,ChevronDown,ChevronRight,Clapperboard,Code2,Command,CommandIcon,Cpu,House,Layers3,Menu,MessageSquare,PanelLeftClose,RadioTower,Search,Settings2,UsersRound,Workflow,Plus} from 'lucide-react'
import {Button,TextInput} from './Primitives'

export type PageId='home'|'chat'|'video'|'models'|'agents'|'rooms'|'workflows'|'memory'|'capabilities'|'extensions'|'connections'|'automations'|'activity'|'settings'|'developer'|'algorithms'
type NavItem={id:PageId;label:string;icon:typeof House}
export const navigation:{title:string;items:NavItem[];collapsible?:boolean}[]=[
 {title:'작업 공간',items:[{id:'chat',label:'채팅',icon:MessageSquare},{id:'rooms',label:'그룹 대화 · 회의',icon:UsersRound},{id:'models',label:'모델 관리',icon:Cpu},{id:'agents',label:'에이전트',icon:Bot},{id:'video',label:'영상 제작',icon:Clapperboard}]},
 {title:'확장',items:[{id:'connections',label:'연결 · 채널',icon:RadioTower},{id:'extensions',label:'도구 · MCP · 스킬',icon:Blocks},{id:'algorithms',label:'알고리즘',icon:BrainCircuit},{id:'memory',label:'메모리',icon:BookOpenText}]},
 {title:'운영',items:[{id:'intelligence',label:'지능 대시보드',icon:BrainCircuit},{id:'automations',label:'자동화',icon:Workflow},{id:'activity',label:'실행 기록',icon:Activity},{id:'settings',label:'설정',icon:Settings2}]},
 {title:'고급',collapsible:true,items:[{id:'workflows',label:'반복 작업',icon:Workflow},{id:'developer',label:'소스 개발 스튜디오',icon:Code2}]}
]
const items=navigation.flatMap(g=>g.items)
export function WorkspaceSidebar({page,open,collapsed,online,running,pending,onNavigate,onNewChat,onToggle,onClose}:{page:PageId;open:boolean;collapsed:boolean;online:boolean;running:number;pending:number;onNavigate:(id:PageId)=>void;onNewChat:()=>void;onToggle:()=>void;onClose:()=>void}){
 const advancedItems=navigation.find(g=>g.collapsible)?.items||[]
 const [advancedOpen,setAdvancedOpen]=useState(()=>{try{return localStorage.getItem('dominion.nav.advanced')==='1'||advancedItems.some(x=>x.id===page)}catch{return advancedItems.some(x=>x.id===page)}})
 useEffect(()=>{if(advancedItems.some(x=>x.id===page))setAdvancedOpen(true)},[page])
 useEffect(()=>{try{localStorage.setItem('dominion.nav.advanced',advancedOpen?'1':'0')}catch{}},[advancedOpen])
 return <>
  {open&&<button type="button" className="fixed inset-0 z-40 bg-slate-900/40 min-[520px]:hidden" aria-label="메뉴 닫기" onClick={onClose}/>}
  <aside aria-label="주 탐색" className={'fixed inset-y-0 left-0 z-50 flex w-[216px] flex-col border-r border-[#E0D8CC] bg-[#EFEAE1] text-[#4A443C] transition-transform duration-200 max-[519px]:-translate-x-full '+(collapsed?'min-[520px]:-translate-x-full ':'')+(open?'max-[519px]:translate-x-0':'')}>
   <div className="workspace-sidebar-brand">
    <button type="button" aria-label="메뉴 접기" title="메뉴 접기" onClick={onToggle}><Menu size={19}/></button>
    <div className="min-w-0 flex-1 text-[14px] font-extrabold tracking-[.14em] text-[#24211D]">DOMINION</div>
   </div>
   <div className="px-3 pt-5"><button type="button" onClick={onNewChat} className="flex h-10 w-full items-center justify-center gap-2 rounded-xl bg-[#C96442] text-[12px] font-semibold text-white shadow-[0_6px_16px_rgba(201,100,66,.22)] transition-colors hover:bg-[#A94E30]"><Plus size={16}/> 새 대화</button></div>
   <nav className="mt-4 min-h-0 flex-1 overflow-y-auto px-2.5 pb-4">
    {navigation.map(group=>{const expanded=!group.collapsible||advancedOpen;return <section key={group.title} className="mt-5">
     {group.collapsible?<button type="button" onClick={()=>setAdvancedOpen(x=>!x)} aria-expanded={advancedOpen} className="mb-2 flex w-full items-center justify-between px-3 text-[11px] font-semibold text-[#8A8176] hover:text-[#4A443C]"><span>{group.title}</span><ChevronDown size={13} className={'transition-transform '+(advancedOpen?'':' -rotate-90')}/></button>:<h2 className="mb-2 px-3 text-[11px] font-semibold text-[#8A8176]">{group.title}</h2>}
     {expanded&&<div className="space-y-0.5">{group.items.map(item=>{const Icon=item.icon;const count=item.id==='activity'?running:item.id==='settings'?pending:0;return <button key={item.id} type="button" aria-current={page===item.id?'page':undefined} onClick={()=>onNavigate(item.id)}
      className={'group flex h-9 w-full items-center gap-3 rounded-xl px-3 text-left text-[13px] transition-colors '+(page===item.id?'bg-[#FFFDF9] font-semibold text-[#24211D] shadow-[0_2px_8px_rgba(70,58,42,.07)]':'text-[#6F675D] hover:bg-[#E8E0D3] hover:text-[#24211D]')}>
      <Icon size={15} strokeWidth={1.8}/><span className="min-w-0 flex-1">{item.label}</span>{count>0&&<span className="text-[10px] font-semibold text-[#C96442]">{count}</span>}
     </button>})}</div>}
    </section>})}
   </nav>
   <div className="flex h-14 shrink-0 items-center gap-2.5 border-t border-[#E0D8CC] px-5">
    <span className={'size-2 rounded-full '+(online?'bg-emerald-500':'bg-rose-400')}/>
    <span className="text-[11px] text-[#6F675D]">{online?'게이트웨이 연결됨':'게이트웨이 연결 안 됨'}</span>
   </div>
  </aside>
 </>
}
export function WorkspaceHeader({page,online,collapsed,onMenu,onSearch}:{page:PageId;online:boolean;collapsed:boolean;onMenu:()=>void;onSearch:()=>void}){
 const selected=items.find(x=>x.id===page)
 return <header className={'workspace-header '+(collapsed?'navigation-collapsed':'')}>
  <div className="workspace-header-brand"><button type="button" aria-label="메뉴 펼치기" title="메뉴 펼치기" onClick={onMenu}><Menu size={19}/></button><strong>DOMINION</strong></div>
  <div className="workspace-default-title">{selected?.label||'DOMINION'}</div>
  <div id="dominion-page-toolbar"/>
  <div className="workspace-header-actions"><button type="button" onClick={onSearch} aria-label="빠른 이동" title="빠른 이동 · ⌘K"><Search size={17}/></button><span className={'workspace-connection '+(online?'is-online':'')} title={online?'Rust 서버 연결됨':'서버 연결 끊김'} aria-label={online?'서버 연결됨':'서버 연결 끊김'}/></div>
 </header>
}

export function CommandPalette({onNavigate,onClose}:{onNavigate:(id:PageId)=>void;onClose:()=>void}){
 const [query,setQuery]=useState('')
 const filtered=useMemo(()=>items.filter(item=>item.label.toLowerCase().includes(query.trim().toLowerCase())),[query])
 useEffect(()=>{const h=(e:KeyboardEvent)=>{if(e.key==='Escape')onClose()};window.addEventListener('keydown',h);return()=>window.removeEventListener('keydown',h)},[onClose])
 return <div className="fixed inset-0 z-[90] flex justify-center bg-black/40 px-3 pt-[12vh]" onMouseDown={onClose} role="presentation">
  <div className="h-fit w-full max-w-[480px] overflow-hidden rounded-xl border border-[#E7E0D5] bg-white shadow-2xl" role="dialog" aria-modal="true" aria-label="빠른 이동" onMouseDown={e=>e.stopPropagation()}>
   <div className="flex items-center gap-2 border-b border-[#EFE9DF] px-4 py-3"><Search size={16} className="text-[#9C9388]"/><TextInput autoFocus aria-label="메뉴 검색" value={query} onChange={e=>setQuery(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'&&filtered[0])onNavigate(filtered[0].id)}} placeholder="메뉴 검색" className="!border-0 !shadow-none !ring-0"/></div>
   <div className="max-h-[55vh] overflow-auto p-2">{filtered.map(item=>{const Icon=item.icon;return <button type="button" key={item.id} onClick={()=>onNavigate(item.id)} className="flex h-10 w-full items-center gap-3 rounded-md px-3 text-left text-xs hover:bg-[#F3EEE5]"><Icon size={16}/><span className="flex-1">{item.label}</span><ChevronRight size={13} className="text-[#B8AFA2]"/></button>})}{!filtered.length&&<p className="p-5 text-center text-xs text-[#7C7469]">검색 결과 없음</p>}</div>
   <div className="border-t border-[#EFE9DF] px-4 py-2 text-[10px] text-[#9C9388]">Enter 이동 · Esc 닫기</div>
  </div>
 </div>
}
