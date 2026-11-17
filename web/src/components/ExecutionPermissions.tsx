import {useEffect,useState} from 'react'
import {createPortal} from 'react-dom'
import {Check,ChevronDown,ShieldAlert,ShieldCheck} from 'lucide-react'
import {get,post} from '../api'
import {Button} from './Primitives'
import {useAnchoredPanel} from '../hooks/useAnchoredPanel'
import {t,useLocale,type LocaleKey} from '../i18n'

type PermissionMode='request'|'workspace'|'full'
const permissionCopy:Record<PermissionMode,{label:LocaleKey;detail:LocaleKey}>={
 request:{label:'chat.perm.request.label',detail:'chat.perm.request.detail'},
 workspace:{label:'chat.perm.workspace.label',detail:'chat.perm.workspace.detail'},
 full:{label:'chat.perm.full.label',detail:'chat.perm.full.detail'}
}
/** Shared by direct and group chat; changing modes never changes policy by itself. */
export function ExecutionPermissions({open,onOpenChange,onError,onChanged,active=true}:{open:boolean;onOpenChange:(open:boolean)=>void;onError:(error:string)=>void;onChanged?:(label:string)=>void;active?:boolean}){
 useLocale()
 const [permissionMode,setPermissionMode]=useState<PermissionMode>('request'),[permissionBusy,setPermissionBusy]=useState(false)
 const permissionPanel=useAnchoredPanel(open,()=>onOpenChange(false),300,'top')
 useEffect(()=>{
  if(!active)return
  const controller=new AbortController()
  get<{autoApprove?:string[];autoApproveAll?:boolean}>('/api/policy',controller.signal).then(p=>{
   if(!controller.signal.aborted)setPermissionMode(p.autoApproveAll?'full':(p.autoApprove||[]).includes('workspace_write')?'workspace':'request')
  }).catch(()=>{})
  return()=>controller.abort()
 },[active,open])
 const changePermission=async(mode:PermissionMode)=>{
  if(permissionBusy)return
  if(mode==='full'&&!window.confirm(t('chat.perm.fullConfirm')))return
  setPermissionBusy(true);onError('')
  try{
   await post('/api/policy',mode==='full'?{id:'default',autoApprove:[],autoApproveAll:true,autoApproveMcp:true}
    :mode==='workspace'?{id:'default',autoApprove:['workspace_write'],autoApproveAll:false,autoApproveMcp:false}
    :{id:'default',autoApprove:[],autoApproveAll:false,autoApproveMcp:false})
   setPermissionMode(mode);onOpenChange(false)
   onChanged?.(t(permissionCopy[mode].label))
  }catch(e){onError((e as Error).message)}finally{setPermissionBusy(false)}
 }
 const activePermission=permissionCopy[permissionMode]
 return <div className="relative">
        <Button type="button" variant="ghost" ref={permissionPanel.trigger} onClick={()=>onOpenChange(!open)} className={'chat-permission inline-flex !h-7 items-center gap-1.5 !rounded-md !border-0 !px-2 !py-0 text-[11px] font-semibold '+(permissionMode==='full'?'!bg-amber-50 text-amber-700 hover:!bg-amber-100':'!bg-[#FAF7F1] text-[#5C544A] hover:!bg-[#F3EEE5]')} title={t(activePermission.detail)} aria-expanded={open}>{permissionMode==='full'?<ShieldAlert size={13}/>:<ShieldCheck size={13}/>} <span className="truncate">{t(activePermission.label)}</span> <ChevronDown size={11}/></Button>
        {open&&permissionPanel.position&&createPortal(<div ref={permissionPanel.panel} style={permissionPanel.position} className="z-[80] max-w-full overflow-y-auto overscroll-contain rounded-xl border border-[#E7E0D5] bg-white p-1.5 shadow-xl"><div className="px-2 py-1.5"><strong className="text-[11px] text-[#4A443C]">{t('chat.permissions')}</strong><p className="text-[10px] text-[#9C9388]">{t('chat.permissionsDesc')}</p></div>{(['request','workspace','full'] as PermissionMode[]).map(mode=><Button type="button" variant="ghost" key={mode} disabled={permissionBusy} onClick={()=>void changePermission(mode)} className={'flex w-full items-start gap-2 !justify-start !border-0 rounded-lg px-2 py-2 text-left hover:bg-[#FAF7F1] '+(permissionMode===mode?'bg-[#FAF7F1]':'')}><span className={'mt-0.5 grid size-4 shrink-0 place-items-center rounded-full border '+(permissionMode===mode?'border-[#C96442] bg-[#C96442] text-white':'border-[#D8CFC2]')}>{permissionMode===mode&&<Check size={11}/>}</span><span className="min-w-0 flex-1"><strong className={'block text-[11px] '+(mode==='full'?'text-amber-700':'text-[#4A443C]')}>{t(permissionCopy[mode].label)}</strong><small className="block text-[10px] leading-4 text-[#9C9388]">{t(permissionCopy[mode].detail)}</small></span></Button>)}</div>,document.body)}
 </div>
}
