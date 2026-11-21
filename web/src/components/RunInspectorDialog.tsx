import {useEffect,useState} from 'react'
import {Activity,X} from 'lucide-react'
import {DialogBackdrop,DialogPanel} from './Dialog'
import {NativeRunWorkspace} from './NativeRunWorkspace'
import {Button} from './Primitives'
import {useLocale} from '../i18n'

/** Shared native execution evidence viewer; chat and rooms only pass Run IDs.
 * Actions remain routed to the one Rust Core approval/journal path. */
export function RunInspectorDialog({id,onClose}:{id:string;onClose:()=>void}){
 const ko=useLocale()==='ko'
 const [selected,setSelected]=useState(id)
 useEffect(()=>setSelected(id),[id])
 return <DialogBackdrop onDismiss={onClose}>
  <DialogPanel size="wide" aria-label={ko?'에이전트 실행 증거':'Agent execution evidence'}>
   <header className="sticky top-0 z-10 flex items-center justify-between gap-2 border-b border-[#E7E0D5] bg-white px-5 py-3">
    <div className="flex items-center gap-2 text-sm font-semibold"><Activity size={17}/>{ko?'실행 기록 · 승인 · 결과':'Execution · approvals · evidence'}</div>
    <Button variant="ghost" size="icon" onClick={onClose} aria-label={ko?'실행 상세 닫기':'Close execution detail'}><X size={18}/></Button>
   </header>
   <div className="px-4 pb-5"><NativeRunWorkspace key={selected} id={selected} onSelect={setSelected}/></div>
  </DialogPanel>
 </DialogBackdrop>
}
