import {useEffect,type HTMLAttributes,type ReactNode} from 'react'

export function DialogBackdrop({children,onDismiss,...props}:HTMLAttributes<HTMLDivElement>&{
 children:ReactNode;onDismiss:()=>void
}){
 useEffect(()=>{
  const onKeyDown=(event:KeyboardEvent)=>{if(event.key==='Escape')onDismiss()};
  document.addEventListener('keydown',onKeyDown);
  return()=>document.removeEventListener('keydown',onKeyDown);
 },[onDismiss]);
 return <div {...props} className="fixed inset-0 z-[60] flex items-center justify-center bg-slate-950/50 p-4 backdrop-blur-[2px]"
  onMouseDown={event=>{if(event.target===event.currentTarget)onDismiss()}}>{children}</div>
}
export function DialogPanel({children,size='default',className='',...props}:HTMLAttributes<HTMLDivElement>&{
 children:ReactNode;size?:'default'|'wide'
}){
 return <div role="dialog" aria-modal="true" aria-label={props['aria-label']||'설정 창'} {...props}
  className={['relative max-h-[90vh] w-full overflow-y-auto rounded-2xl border border-[#E7E0D5] bg-white shadow-2xl',
   size==='wide'?'max-w-3xl':'max-w-xl',className].join(' ')}>{children}</div>
}
export function DialogHeader({children}: {children:ReactNode}){
 return <header className="flex items-center justify-between gap-3 border-b border-[#EFE9DF] p-5">{children}</header>
}
export function DialogBody({children}: {children:ReactNode}){
 return <div className="grid gap-4 p-5">{children}</div>
}
export function DialogActions({children}: {children:ReactNode}){
 return <footer className="flex flex-wrap items-center justify-end gap-2 border-t border-[#EFE9DF] p-5">{children}</footer>
}
