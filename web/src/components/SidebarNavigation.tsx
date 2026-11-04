import type {LucideIcon} from 'lucide-react'

export function SidebarNavItem({icon:Icon,label,active,count,onClick}:{
 icon:LucideIcon;label:string;active:boolean;count?:number;onClick:()=>void
}){
 return <button type="button" onClick={onClick} aria-current={active?'page':undefined}
  className={['group relative flex h-10 w-full items-center gap-3 rounded-xl border-0 px-3 text-left text-[13px] font-semibold',
   active?'bg-[#C96442]/15 text-white shadow-[inset_3px_0_0_#C96442]':'bg-transparent text-[#9C9388] hover:bg-white/10 hover:text-white'].join(' ')}>
  <Icon size={18} strokeWidth={1.8} className={active?'shrink-0 text-blue-300':'shrink-0 text-[#7C7469] group-hover:text-blue-300'}/>
  <span className="min-w-0 flex-1 truncate">{label}</span>
  {Boolean(count)&&<span className="rounded-full bg-[#C96442] px-2 py-0.5 text-[10px] font-bold text-white">{count}</span>}
 </button>
}
