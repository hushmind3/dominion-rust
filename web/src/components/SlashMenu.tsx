import {useLayoutEffect,useRef} from 'react'
import {Bot,BrainCircuit,ChevronRight,Command,FileDown,History,Layers3,ListTree,MessageSquare,Play,Search,Settings2,Shield,Square,Terminal,Workflow} from 'lucide-react'
import {Button} from './Primitives'
import commandDefinitions from '../../shared/commands.json'
import {t,useLocale} from '../i18n'

export type SlashChoice={
 key:string;command:string;name:string;description:string;group:string;
 requiresArg?:boolean;submenu?:boolean;hint?:string
}
type Selectable={id:string;name:string}
type Model={id:string;name?:string}
const defs:SlashChoice[]=commandDefinitions
type EnglishCommand={name:string;description:string;group:string;hint?:string}
const englishCommands:Record<string,EnglishCommand>={
 new:{name:'New chat',description:'Start a new session',group:'Session'},
 history:{name:'Conversation history',description:'Show saved conversations',group:'Session'},
 name:{name:'Rename session',description:'Rename the current session',group:'Session',hint:'New name'},
 clear:{name:'Reset conversation',description:'Clear messages and start over',group:'Session'},
 fork:{name:'Branch conversation',description:'Create a new session from this transcript',group:'Session'},
 export:{name:'Export transcript',description:'Save this conversation as JSON',group:'Session'},
 compact:{name:'Compact context',description:'Summarize earlier turns and keep recent messages',group:'Session'},
 goal:{name:'Run autonomous work',description:'Register a goal with the execution engine',group:'Run',hint:'Work to run'},
 stop:{name:'Stop execution',description:'Cancel the running execution in this session',group:'Run'},
 status:{name:'Execution status',description:'Check connection and execution state',group:'Run'},
 think:{name:'Show reasoning',description:'Toggle reasoning display (on / off)',group:'Model · agents'},
 model:{name:'Switch model',description:'Choose a model from the current provider',group:'Model · agents'},
 agent:{name:'Switch agent',description:'Choose the agent that runs work',group:'Model · agents'},
 tools:{name:'Tool list',description:'List built-in and MCP tools',group:'Tools · extensions'},
 mcp:{name:'MCP settings',description:'Open MCP connections and servers',group:'Tools · extensions'},
 skills:{name:'Manage skills',description:'Manage installed skills and extensions',group:'Tools · extensions'},
 memory:{name:'Manage memory',description:'Open saved long-term memories',group:'Tools · extensions'},
 workflows:{name:'Workflows',description:'Open the execution engine and workflows',group:'Screens'},
 agents:{name:'Manage agents',description:'Open agent settings',group:'Screens'},
 settings:{name:'Settings',description:'Open platform settings',group:'Screens'},
 help:{name:'Command help',description:'List available commands and usage',group:'Screens'},
}
export function slashCommandLabel(item:SlashChoice,locale:'ko'|'en'):SlashChoice{
 const english=locale==='en'&&!item.key.includes(':')?englishCommands[item.key]:undefined
 return english?{...item,...english}:item
}
export const slashCommands=defs
export const normalizeSlashName=(text:string)=>text.trim().split(/\s+/)[0]?.slice(1).toLowerCase()||''
export function slashCandidates(text:string,agents:Selectable[],models:Model[]):SlashChoice[]{
 if(!text.startsWith('/')||text.includes('\n'))return [];
 if(/^\/model\s/i.test(text)){
  const query=text.replace(/^\/model\s+/i,'').trim().toLocaleLowerCase()
  return models.filter(m=>m.id.toLocaleLowerCase().includes(query)||(m.name||'').toLocaleLowerCase().includes(query))
   .slice(0,30).map(m=>({key:'model:'+m.id,command:'/model '+m.id,name:m.name||m.id,description:m.id,group:t('slash.models')}))
 }
 if(/^\/agent\s/i.test(text)){
  const query=text.replace(/^\/agent\s+/i,'').trim().toLocaleLowerCase()
  return agents.filter(a=>a.id.toLocaleLowerCase().includes(query)||a.name.toLocaleLowerCase().includes(query))
   .slice(0,30).map(a=>({key:'agent:'+a.id,command:'/agent '+a.id,name:a.name,description:a.id,group:t('slash.agents')}))
 }
 if(/\s/.test(text))return [];
 const query=text.slice(1).toLocaleLowerCase()
 return defs.filter(item=>[item.key,item.name,item.description].some(s=>s.toLocaleLowerCase().includes(query)))
}
const icons:Record<string,typeof Command>={
 new:MessageSquare,history:History,name:MessageSquare,clear:Square,fork:Layers3,export:FileDown,
 compact:ListTree,goal:Play,stop:Square,status:Search,think:BrainCircuit,model:BrainCircuit,
 agent:Bot,tools:Terminal,mcp:Terminal,skills:Layers3,memory:History,
 workflows:Workflow,agents:Bot,settings:Settings2,help:Shield
}
export function SlashMenu({items,active,onChoose}:{items:SlashChoice[];active:number;onChoose:(item:SlashChoice)=>void}){
 const loc=useLocale()
 const localized=items.map(item=>slashCommandLabel(item,loc))
 const groups=[...new Set(localized.map(item=>item.group))]
 const list=useRef<HTMLDivElement>(null)
 const selected=useRef<HTMLButtonElement>(null)
 useLayoutEffect(()=>{
  const panel=list.current, option=selected.current
  if(!panel||!option)return
  const bounds=panel.getBoundingClientRect(), row=option.getBoundingClientRect()
  if(row.top<bounds.top)panel.scrollTop-=bounds.top-row.top
  else if(row.bottom>bounds.bottom)panel.scrollTop+=row.bottom-bounds.bottom
 },[active,items])
 return <div ref={list} role="listbox" aria-label={t('slash.title')} id="dominion-slash-menu"
  className="absolute bottom-full left-0 right-0 z-30 mb-3 max-h-[min(400px,60dvh)] overflow-y-auto overscroll-contain rounded-2xl border border-[#E7E0D5] bg-white p-2 shadow-[0_16px_60px_rgba(15,23,42,.2)]">
  <div className="flex items-center gap-2 border-b border-[#EFE9DF] px-3 pb-2 pt-1 text-xs text-[#7C7469]">
   <Command size={15}/> {t('slash.search')}
   <span className="ml-auto text-[10px] text-[#9C9388]">{t('slash.hint')}</span>
  </div>
  {items.length===0&&<p className="px-3 py-6 text-center text-xs text-[#7C7469]">{t('slash.empty')}</p>}
  {groups.map(group=><div key={group}>
   <p className="px-3 pb-1 pt-3 text-[10px] font-bold tracking-[.12em] text-[#9C9388]">{group}</p>
   {localized.map((item,index)=>{
    if(item.group!==group)return null
    const Icon=icons[item.key]|| (item.key.startsWith('model:')?BrainCircuit:Bot)
    return <Button key={item.key} type="button" variant="ghost" role="option" ref={active===index?selected:undefined} aria-selected={active===index}
      onMouseDown={e=>e.preventDefault()} onClick={()=>onChoose(item)}
      className={'!flex !w-full !items-center !justify-start !gap-3 !rounded-lg !border-0 !px-3 !py-2 !text-left '+(active===index?'!bg-[#F7E8E1] !text-[#A94E30]':'!text-[#4A443C]')}>
      <span className={'grid size-8 shrink-0 place-items-center rounded-lg '+(active===index?'bg-[#F1DCCF]':'bg-[#F3EEE5]')}><Icon size={16}/></span>
      <span className="min-w-0 flex-1"><strong className="block truncate text-sm font-semibold">
       {item.key.startsWith('model:')||item.key.startsWith('agent:')?item.name:item.command}
      </strong><small className="block truncate text-[11px] font-normal text-[#7C7469]">{item.key.includes(':')?item.description:item.name+' · '+item.description}</small></span>
      {item.submenu?<ChevronRight size={14} className="shrink-0 text-[#9C9388]"/>:
       item.requiresArg?<span className="shrink-0 text-[10px] text-[#9C9388]">{item.hint}…</span>:null}
    </Button>
   })}
  </div>)}
 </div>
}
