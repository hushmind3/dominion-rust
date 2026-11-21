import {createPortal} from 'react-dom'
import {ChevronDown,ShieldCheck} from 'lucide-react'
import {Button} from './Primitives'
import {useAnchoredPanel} from '../hooks/useAnchoredPanel'
import {useLocale} from '../i18n'

/** Rust native approval is per effect, not a browser-side global bypass.
 * The owner sees the real supported mode, and the exact tool request appears
 * in Chat/Room approval cards and native Run inspector. */
export function ExecutionPermissions({open,onOpenChange,active=true}:{
 open:boolean;onOpenChange:(open:boolean)=>void;onError?:(error:string)=>void;
 onChanged?:(label:string)=>void;active?:boolean
}){
 const ko=useLocale()==='ko'
 const panel=useAnchoredPanel(open,()=>onOpenChange(false),300,'top')
 return <div className="relative">
  <Button type="button" variant="ghost" ref={panel.trigger} disabled={!active}
   onClick={()=>onOpenChange(!open)} aria-expanded={open} aria-haspopup="dialog"
   title={ko?'민감한 도구는 매번 실제 실행 전 승인합니다.':'Mutating tools require a real approval before execution.'}
   className="chat-permission inline-flex !h-7 items-center gap-1.5 !rounded-md !border-0 !bg-[#FAF7F1] !px-2 !py-0 text-[11px] font-semibold text-[#5C544A] hover:!bg-[#F3EEE5]">
   <ShieldCheck size={13}/><span className="truncate">{ko?'실행별 승인':'Approve each action'}</span><ChevronDown size={11}/>
  </Button>
  {open&&panel.position&&createPortal(<div ref={panel.panel} style={panel.position}
   role="dialog" aria-label={ko?'도구 실행 권한':'Execution approval policy'}
   className="z-[80] max-w-full rounded-xl border border-[#E7E0D5] bg-white p-3 shadow-xl">
   <div className="flex items-center gap-2 text-xs font-semibold"><ShieldCheck size={16}/>{ko?'현재 Rust 실행 권한':'Native Rust approval mode'}</div>
   <p className="mt-2 text-[11px] leading-5 text-[#7C7469]">
    {ko?'작업을 읽고 조사하는 동안은 실행기가 허용 범위를 확인합니다. 파일 수정·쉘·MCP·외부 Worker 작업 등 승인이 필요한 도구는 실행 전에 정확한 인수를 보여드리고 1회만 승인받습니다.':'The native harness validates read-only work. Sensitive tools including file edits, shell, MCP and external agents pause to show exact arguments and request one-time approval.'}
   </p>
   <p className="mt-2 rounded-lg bg-[#FAF7F1] p-2 text-[11px] text-[#7C7469]">
    {ko?'전체 자동 승인·워크스페이스 무제한 실행은 이 Rust 엔진에서 제공하지 않습니다.':'Blanket auto-approval and unrestricted workspace mode are not available in this native Rust engine.'}
   </p>
   <Button variant="secondary" size="sm" className="mt-3" onClick={()=>onOpenChange(false)}>{ko?'닫기':'Close'}</Button>
  </div>,document.body)}
 </div>
}
