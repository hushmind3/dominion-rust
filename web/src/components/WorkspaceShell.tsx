import {useEffect,useMemo,useState} from 'react'
import {OwnerProfileEditor} from './OwnerProfile'
import {Activity,Blocks,BookOpenText,Bot,ChevronDown,ChevronRight,Clapperboard,Code2,Command,CommandIcon,Cpu,House,Layers3,Menu,MessageSquare,PanelLeftClose,RadioTower,Search,Settings2,UsersRound,Workflow,Plus,BrainCircuit} from 'lucide-react'
import {Button,TextInput} from './Primitives'
import {t,type LocaleKey,useLocale} from '../i18n'

export type PageId='home'|'chat'|'video'|'models'|'agents'|'rooms'|'workflows'|'goals'|'memory'|'capabilities'|'extensions'|'connections'|'automations'|'activity'|'settings'|'developer'|'rsi'
type NavItem={id:PageId;label:LocaleKey;icon:typeof House}
export const navigation:{title:LocaleKey;items:NavItem[];collapsible?:boolean}[]=[
 {title:'nav.group.workspace',items:[{id:'home',label:'nav.home',icon:House},{id:'chat',label:'nav.chat',icon:MessageSquare},{id:'rooms',label:'nav.rooms',icon:UsersRound},{id:'models',label:'nav.models',icon:Cpu},{id:'agents',label:'nav.agents',icon:Bot},{id:'video',label:'nav.video',icon:Clapperboard}]},
 {title:'nav.group.extensions',items:[{id:'connections',label:'nav.connections',icon:RadioTower},{id:'extensions',label:'nav.extensions',icon:Blocks},{id:'capabilities',label:'nav.capabilities',icon:Blocks},{id:'memory',label:'nav.memory',icon:BookOpenText}]},
 {title:'nav.group.operations',items:[{id:'goals',label:'nav.goals',icon:Bot},{id:'automations',label:'nav.automations',icon:Workflow},{id:'activity',label:'nav.activity',icon:Activity},{id:'rsi',label:'nav.rsi',icon:BrainCircuit},{id:'settings',label:'nav.settings',icon:Settings2}]},
 {title:'nav.group.advanced',collapsible:true,items:[{id:'workflows',label:'nav.workflows',icon:Workflow},{id:'developer',label:'nav.developer',icon:Code2}]}
]
const items=navigation.flatMap(g=>g.items)
export function WorkspaceSidebar({page,open,collapsed,online,running,pending,onNavigate,onNewChat,onClose}:{page:PageId;open:boolean;collapsed:boolean;online:boolean;running:number;pending:number;onNavigate:(id:PageId)=>void;onNewChat:()=>void;onClose:()=>void}){
 useLocale()
 const advancedItems=navigation.find(g=>g.collapsible)?.items||[]
 const [advancedOpen,setAdvancedOpen]=useState(()=>{try{return localStorage.getItem('dominion.nav.advanced')==='1'||advancedItems.some(x=>x.id===page)}catch{return advancedItems.some(x=>x.id===page)}})
 useEffect(()=>{if(advancedItems.some(x=>x.id===page))setAdvancedOpen(true)},[page])
 useEffect(()=>{try{localStorage.setItem('dominion.nav.advanced',advancedOpen?'1':'0')}catch{}},[advancedOpen])
 return <>
  {open&&<Button variant="bare" type="button" className="fixed inset-0 z-40 bg-slate-900/40 min-[520px]:hidden" aria-label={t('shell.menuClose')} onClick={onClose}/>}
  <aside aria-label={t('shell.mainNav')} className={'fixed inset-y-0 left-0 z-50 flex w-[216px] flex-col border-r border-[#E0D8CC] bg-[#EFEAE1] text-[#4A443C] transition-transform duration-200 max-[519px]:-translate-x-full '+(collapsed?'min-[520px]:-translate-x-full ':'')+(open?'max-[519px]:translate-x-0':'')}>
   <div className="flex h-16 shrink-0 items-center gap-3 border-b border-[#E0D8CC] px-5">
    <div className="grid size-8 place-items-center rounded-xl bg-[#C96442] text-[17px] font-black text-white shadow-[0_4px_12px_rgba(201,100,66,.28)]">D</div>
    <div className="min-w-0 flex-1 text-[14px] font-extrabold tracking-[.14em] text-[#24211D]">DOMINION</div>
   </div>
   <div className="px-3 pt-5"><Button variant="bare" type="button" onClick={onNewChat} className="flex h-10 w-full items-center justify-center gap-2 rounded-xl bg-[#C96442] text-[12px] font-semibold text-white shadow-[0_6px_16px_rgba(201,100,66,.22)] transition-colors hover:bg-[#A94E30]"><Plus size={16}/> {t('shell.newChat')}</Button></div>
   <nav className="mt-4 min-h-0 flex-1 overflow-y-auto px-2.5 pb-4">
    {navigation.map(group=>{const expanded=!group.collapsible||advancedOpen;return <section key={group.title} className="mt-5">
     {group.collapsible?<Button variant="bare" type="button" onClick={()=>setAdvancedOpen(x=>!x)} aria-expanded={advancedOpen} className="mb-2 flex w-full items-center justify-between px-3 text-[11px] font-semibold text-[#8A8176] hover:text-[#4A443C]"><span>{t(group.title)}</span><ChevronDown size={13} className={'transition-transform '+(advancedOpen?'':' -rotate-90')}/></Button>:<h2 className="mb-2 px-3 text-[11px] font-semibold text-[#8A8176]">{t(group.title)}</h2>}
     {expanded&&<div className="space-y-0.5">{group.items.map(item=>{const Icon=item.icon;const count=item.id==='activity'?running:item.id==='settings'?pending:0;return <Button variant="bare" key={item.id} type="button" aria-current={page===item.id?'page':undefined} onClick={()=>onNavigate(item.id)}
      className={'group flex h-9 w-full items-center gap-3 rounded-xl px-3 text-left text-[13px] transition-colors '+(page===item.id?'bg-[#FFFDF9] font-semibold text-[#24211D] shadow-[0_2px_8px_rgba(70,58,42,.07)]':'text-[#6F675D] hover:bg-[#E8E0D3] hover:text-[#24211D]')}>
      <Icon size={15} strokeWidth={1.8}/><span className="min-w-0 flex-1">{t(item.label)}</span>{count>0&&<span className="text-[10px] font-semibold text-[#C96442]">{count}</span>}
     </Button>})}</div>}
    </section>})}
   </nav>
   <OwnerProfileEditor/>
   <div className="flex h-14 shrink-0 items-center gap-2.5 border-t border-[#E0D8CC] px-5">
    <span className={'size-2 rounded-full '+(online?'bg-emerald-500':'bg-rose-400')}/>
    <span className="text-[11px] text-[#6F675D]">{online?t('shell.engineConnected'):t('shell.engineDisconnected')}</span>
   </div>
  </aside>
 </>
}
export function WorkspaceHeader({page,online,collapsed,build,onMenu,onSearch}:{page:PageId;online:boolean;collapsed:boolean;build?:{branch:string;commit:string}|null;onMenu:()=>void;onSearch:()=>void}){
 useLocale()
 const selected=items.find(x=>x.id===page)
 return <header className="sticky top-0 z-30 flex h-16 items-center justify-between gap-3 border-b border-[#E7E0D5] bg-[#FFFDF9]/90 px-6 backdrop-blur-md">
  <div className="flex min-w-0 items-center gap-2">
   <Button variant="bare" type="button" className="mr-2 rounded-md p-1.5 text-[#5C544A] transition-colors hover:bg-[#F3EEE5]" aria-label={collapsed?t('shell.menuExpand'):t('shell.menuCollapse')} title={collapsed?t('shell.menuExpand'):t('shell.menuCollapse')} onClick={onMenu}>{collapsed?<Menu size={18}/>:<PanelLeftClose size={18}/>}</Button>
   <span className="hidden text-[12px] text-[#9C9388] sm:block">{t('shell.workspace')}</span><ChevronRight size={13} className="hidden text-[#B8AFA2] sm:block"/>
   <h1 className="truncate text-[15px] font-bold tracking-[-.01em] text-[#24211D]">{selected?t(selected.label):t('nav.home')}</h1>
  </div>
  <div className="flex items-center gap-3">
   <Button variant="bare" type="button" onClick={onSearch} className="flex h-9 items-center gap-2 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] px-3.5 text-[12px] text-[#7C7469] transition-colors hover:border-[#D8CFC2] hover:bg-[#F3EEE5]"><Search size={14}/><span className="hidden sm:inline">{t('shell.quickNav')}</span><kbd className="hidden text-[10px] md:inline">⌘K</kbd></Button>
   <span className={'hidden items-center gap-1.5 text-[11px] sm:inline-flex '+(online?'text-emerald-700':'text-rose-600')}><i className={'size-1.5 rounded-full '+(online?'bg-emerald-500':'bg-rose-500')}/>{online?t('shell.connected'):t('shell.disconnected')}</span>
    {build?.branch&&build.branch!=='unknown'&&<span className="hidden max-w-[185px] truncate rounded-md border border-[#E7E0D5] bg-[#F7F3EC] px-2 py-1 text-[10px] font-mono text-[#8A8176] lg:block" title={build.branch+' · '+build.commit}>{build.branch.replace(/^dominion-/,'')} · {build.commit.slice(0,7)}</span>}
    <OwnerProfileEditor compact/>
  </div>
 </header>
}
export function CommandPalette({onNavigate,onClose}:{onNavigate:(id:PageId)=>void;onClose:()=>void}){
 useLocale()
 const [query,setQuery]=useState('')
 const [active,setActive]=useState(0)
 const filtered=useMemo(()=>items.filter(item=>t(item.label).toLowerCase().includes(query.trim().toLowerCase())),[query])
 useEffect(()=>{setActive(0)},[query])
 useEffect(()=>{const h=(e:KeyboardEvent)=>{if(e.key==='Escape')onClose()};window.addEventListener('keydown',h);return()=>window.removeEventListener('keydown',h)},[onClose])
 return <div className="fixed inset-0 z-[90] flex justify-center bg-black/40 px-3 pt-[12vh]" onMouseDown={onClose} role="presentation">
  <div className="h-fit w-full max-w-[480px] overflow-hidden rounded-xl border border-[#E7E0D5] bg-white shadow-2xl" role="dialog" aria-modal="true" aria-label={t('shell.paletteAria')} onMouseDown={e=>e.stopPropagation()}>
   <div className="flex items-center gap-2 border-b border-[#EFE9DF] px-4 py-3"><Search size={16} className="text-[#9C9388]"/><TextInput autoFocus aria-label={t('shell.searchAria')} value={query} onChange={e=>setQuery(e.target.value)} onKeyDown={e=>{if(e.key==='ArrowDown'){e.preventDefault();setActive(value=>Math.min(filtered.length-1,value+1))}else if(e.key==='ArrowUp'){e.preventDefault();setActive(value=>Math.max(0,value-1))}else if(e.key==='Enter'&&filtered[active])onNavigate(filtered[active].id)}} placeholder={t('palette.placeholder')} className="!border-0 !shadow-none !ring-0"/></div>
   <div className="max-h-[55vh] overflow-auto p-2">{filtered.map((item,index)=>{const Icon=item.icon;return <Button variant="bare" type="button" key={item.id} onMouseEnter={()=>setActive(index)} onClick={()=>onNavigate(item.id)} aria-selected={active===index} className={'flex h-10 w-full items-center gap-3 rounded-md px-3 text-left text-xs '+(active===index?'bg-[#F3EEE5]':'hover:bg-[#F3EEE5]')}><Icon size={16}/><span className="flex-1">{t(item.label)}</span><ChevronRight size={13} className="text-[#B8AFA2]"/></Button>})}{!filtered.length&&<p className="p-5 text-center text-xs text-[#7C7469]">{t('palette.empty')}</p>}</div>
   <div className="border-t border-[#EFE9DF] px-4 py-2 text-[10px] text-[#9C9388]">{t('palette.hint')}</div>
  </div>
 </div>
}
