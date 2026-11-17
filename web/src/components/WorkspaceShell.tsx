import {useEffect,useState} from 'react'
import {OwnerProfileEditor} from './OwnerProfile'
import {Activity,BookOpenText,Bot,ChevronDown,Cpu,House,MessageSquare,RadioTower,Settings2,UsersRound,Plus,X,Settings,Menu,Wallet} from 'lucide-react'
import {Button} from './Primitives'
import {t,type LocaleKey,useLocale} from '../i18n'

export type PageId='home'|'chat'|'models'|'agents'|'rooms'|'memory'|'money'|'capabilities'|'extensions'|'connections'|'activity'|'settings'
type NavItem={id:PageId;label:LocaleKey;icon:typeof House}
export const navigation:{title:LocaleKey;items:NavItem[];collapsible?:boolean}[]=[
 {title:'nav.group.workspace',items:[{id:'home',label:'nav.home',icon:House},{id:'chat',label:'nav.chat',icon:MessageSquare},{id:'rooms',label:'nav.rooms',icon:UsersRound},{id:'models',label:'nav.models',icon:Cpu},{id:'agents',label:'nav.agents',icon:Bot}]},
 {title:'nav.group.extensions',items:[{id:'connections',label:'nav.connections',icon:RadioTower},{id:'memory',label:'nav.memory',icon:BookOpenText},{id:'money',label:'nav.money',icon:Wallet}]},
 {title:'nav.group.operations',items:[{id:'activity',label:'nav.activity',icon:Activity},{id:'settings',label:'nav.settings',icon:Settings2}]}
]
const items=navigation.flatMap(g=>g.items)
export function WorkspaceSidebar({page,open,collapsed,online,running,pending,onNavigate,onNewChat,onClose,settingsOnly=false,navOpen=true,onToggleNav}:{settingsOnly?:boolean;page:PageId;open:boolean;collapsed:boolean;online:boolean;running:number;pending:number;onNavigate:(id:PageId)=>void;onNewChat:()=>void;onClose:()=>void;navOpen?:boolean;onToggleNav?:()=>void}){
 useLocale()
 const advancedItems=navigation.find(g=>g.collapsible)?.items||[]
 const [advancedOpen,setAdvancedOpen]=useState(()=>{try{return localStorage.getItem('dominion.nav.advanced')==='1'||advancedItems.some(x=>x.id===page)}catch{return advancedItems.some(x=>x.id===page)}})
 useEffect(()=>{if(advancedItems.some(x=>x.id===page))setAdvancedOpen(true)},[page])
 useEffect(()=>{try{localStorage.setItem('dominion.nav.advanced',advancedOpen?'1':'0')}catch{}},[advancedOpen])
 return <>
  {!settingsOnly&&open&&<Button variant="bare" type="button" className="fixed inset-0 z-40 bg-slate-900/40 min-[520px]:hidden" aria-label={t('shell.menuClose')} onClick={onClose}/>}
  <aside aria-label={t('shell.mainNav')} className={(settingsOnly?'absolute inset-y-0 left-0 z-10 flex':'fixed inset-y-0 left-0 z-50 flex')+' w-[216px] flex-col border-r border-[#E0D8CC] bg-[#EFEAE1] text-[#4A443C] transition-[width,transform] duration-200 '+(settingsOnly&&!navOpen?'settings-nav-collapsed ':'')+(!settingsOnly?'max-[519px]:-translate-x-full ':'')+(collapsed?'min-[520px]:-translate-x-full ':'')+(open?'max-[519px]:translate-x-0':'')}>
   <div className="flex h-16 shrink-0 items-center gap-3 border-b border-[#E0D8CC] px-5">
    <div className="grid size-8 shrink-0 place-items-center rounded-xl bg-[#C96442] text-[17px] font-black text-white shadow-[0_4px_12px_rgba(201,100,66,.28)]">D</div>
    <div className="min-w-0 flex-1 truncate text-[14px] font-extrabold tracking-[.14em] text-[#24211D]">DOMINION</div>
    {settingsOnly&&onToggleNav&&<Button variant="ghost" size="icon" aria-label={navOpen?t('shell.hideMenu'):t('shell.showMenu')} aria-expanded={navOpen} onClick={onToggleNav}><Menu size={18}/></Button>}
   </div>
   {!settingsOnly&&<div className="px-3 pt-5"><Button variant="bare" type="button" onClick={onNewChat} className="flex h-10 w-full items-center justify-center gap-2 rounded-xl bg-[#C96442] text-[12px] font-semibold text-white shadow-[0_6px_16px_rgba(201,100,66,.22)] transition-colors hover:bg-[#A94E30]"><Plus size={16}/> {t('shell.newChat')}</Button></div>}
   <nav className="mt-4 min-h-0 flex-1 overflow-y-auto px-2.5 pb-4">
    {navigation.map(group=>{const expanded=!group.collapsible||advancedOpen;return <section key={group.title} className="mt-5">
     {group.collapsible?<Button variant="bare" type="button" onClick={()=>setAdvancedOpen(x=>!x)} aria-expanded={advancedOpen} className="mb-2 flex w-full items-center justify-between px-3 text-[11px] font-semibold text-[#8A8176] hover:text-[#4A443C]"><span>{t(group.title)}</span><ChevronDown size={13} className={'transition-transform '+(advancedOpen?'':' -rotate-90')}/></Button>:<h2 className="mb-2 px-3 text-[11px] font-semibold text-[#8A8176]">{t(group.title)}</h2>}
     {expanded&&<div className="space-y-0.5">{group.items.filter(item=>!settingsOnly||!['chat','rooms'].includes(item.id)).map(item=>{const Icon=item.icon;const count=item.id==='activity'?running:item.id==='settings'?pending:0;return <Button variant="bare" key={item.id} type="button" aria-current={page===item.id?'page':undefined} onClick={()=>onNavigate(item.id)}
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
export function SettingsHeader({page,online,build,onClose}:{page:PageId;online:boolean;build?:{branch:string;commit:string}|null;onClose:()=>void}){
 useLocale()
 const selected=items.find(x=>x.id===page)
 return <header className="flex h-13 shrink-0 items-center gap-3 border-b border-[#E7E0D5] bg-[#FFFDF9] px-4">
  <Settings size={17} className="text-[#8A8176]"/><h1 className="min-w-0 flex-1 truncate text-[14px] font-semibold text-[#24211D]">{selected?t(selected.label):t('nav.settings')}</h1>
  <span className={'hidden items-center gap-1.5 text-[11px] sm:inline-flex '+(online?'text-emerald-700':'text-rose-600')}><i className={'size-1.5 rounded-full '+(online?'bg-emerald-500':'bg-rose-500')}/>{online?t('shell.connected'):t('shell.disconnected')}</span>
  {build?.branch&&build.branch!=='unknown'&&<span className="hidden max-w-[185px] truncate rounded-md border border-[#E7E0D5] bg-[#F7F3EC] px-2 py-1 text-[10px] font-mono text-[#8A8176] lg:block" title={build.branch+' · '+build.commit}>{build.branch.replace(/^dominion-/,'')} · {build.commit.slice(0,7)}</span>}
  <OwnerProfileEditor compact/>
  <Button variant="ghost" size="icon" aria-label={t('rooms.close')} onClick={onClose}><X size={18}/></Button>
 </header>
}
/** Persistent toolbar navigation; independent of either session list. */
export function ConversationSwitch({mode,onChange}:{mode:'chat'|'rooms';onChange:(mode:'chat'|'rooms')=>void}){
 useLocale()
 return <nav className="conversation-switch" aria-label={t('shell.conversationSwitch')}><Button variant="ghost" aria-current={mode==='chat'?'page':undefined} onClick={()=>onChange('chat')}><MessageSquare size={16}/><span>{t('nav.chat')}</span></Button><Button variant="ghost" aria-current={mode==='rooms'?'page':undefined} onClick={()=>onChange('rooms')}><UsersRound size={16}/><span>{t('rooms.listTitle')}</span></Button></nav>
}
