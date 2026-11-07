import {t} from '../i18n'
import React,{useCallback,useEffect,useRef,useState} from 'react'
import {ArrowUpRight,LoaderCircle,Plus} from 'lucide-react'
import {get} from '../api'
import {Button} from './Primitives'
export function Panel({children,className='',...rest}:{children:React.ReactNode;className?:string;[key:string]:any}){return <section className={['min-w-0 rounded-2xl border border-[#E7E0D5] bg-[#FFFDF9] shadow-[0_1px_2px_rgba(36,33,29,.04),0_10px_30px_rgba(36,33,29,.03)]',className].join(' ')} {...rest}>{children}</section>}
export function Headline({eyebrow,title,description,actions}:{eyebrow?:string;title:string;description?:string;actions?:React.ReactNode}){return <header className="mb-6 flex flex-wrap items-end justify-between gap-4"><div className="min-w-0">{eyebrow&&<p className="mb-1.5 text-[11px] font-semibold uppercase tracking-[.09em] text-[#A79C8E]">{eyebrow}</p>}<h1 className="m-0 text-[24px] font-bold tracking-[-.03em] text-[#24211D]">{title}</h1>{description&&<p className="mt-2 max-w-2xl text-[13px] leading-6 text-[#7C7469]">{description}</p>}</div>{actions&&<div className="flex flex-wrap gap-2">{actions}</div>}</header>}
export function Action({children,onClick,variant='primary',disabled=false,type='button',className=''}:{children:React.ReactNode;onClick?:()=>void;variant?:'primary'|'secondary'|'ghost'|'danger';disabled?:boolean;type?:'button'|'submit';className?:string}){return <Button type={type} variant={variant} className={className} onClick={onClick} disabled={disabled}>{children}</Button>}
export function Subheading({title,extra}:{title:string;extra?:React.ReactNode}){return <div className="mb-3 flex flex-wrap items-center justify-between gap-3"><h3 className="m-0 text-[14px] font-bold tracking-[-.01em] text-[#3A342D]">{title}</h3>{extra}</div>}
export function FormField({label,children,hint}:{label:string;children:React.ReactNode;hint?:string}){return <label className="flex min-w-0 flex-col gap-2 text-[12px] font-semibold text-[#3A342D]"><span>{label}</span>{children}{hint&&<small className="text-[11px] font-normal leading-5 text-[#8A8176]">{hint}</small>}</label>}
export function Empty({icon:Icon=Plus,title,description}:{icon?:any;title:string;description?:string}){return <div className="flex min-h-36 flex-col items-center justify-center gap-2 p-6 text-center"><Icon size={21} className="text-[#9C9388]"/><strong className="text-[12px] font-semibold">{title}</strong>{description&&<p className="max-w-sm text-[11px] text-[#8A8176]">{description}</p>}</div>}
export function StateTag({online,children}:{online?:boolean;children:React.ReactNode}){return <span className={['inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-[10px] font-semibold',online?'bg-emerald-50 text-emerald-700':'bg-[#F3EEE5] text-[#5C544A]'].join(' ')}><i className={'size-1.5 rounded-full '+(online?'bg-emerald-500':'bg-slate-400')}/>{children}</span>}
export function useRemote<T>(endpoint: string | null, initial: T, refreshOnEvent = true) {
 const initialValue = useRef(initial)
 const [state, setState] = useState({endpoint, value: initial, loading: !!endpoint, error: ''})
 const active = useRef<AbortController | null>(null)
 const refreshPending = useRef(false)
 const reload = useCallback(async () => {
  if (!endpoint) return
  active.current?.abort()
  const controller = new AbortController()
  active.current = controller
  try {
   const value = await get<T>(endpoint, controller.signal)
   if (!controller.signal.aborted) setState({endpoint, value, loading: false, error: ''})
  } catch (e) {
   if (!controller.signal.aborted) setState(previous => ({
    endpoint, value: previous.endpoint === endpoint ? previous.value : initialValue.current,
    loading: false, error: (e as Error).message,
   }))
  } finally {
   if (active.current === controller) {
    active.current = null
    if (refreshPending.current) {
     refreshPending.current = false
     void reload()
    }
   }
  }
 }, [endpoint])
 useEffect(() => {
  void reload()
  const onRefresh = () => {
   if (active.current) refreshPending.current = true
   else void reload()
  }
  if (endpoint && refreshOnEvent) window.addEventListener('dominion-refresh', onRefresh)
  return () => {
   refreshPending.current = false
   active.current?.abort()
   active.current = null
   window.removeEventListener('dominion-refresh', onRefresh)
  }
 }, [endpoint, reload, refreshOnEvent])
 const setValue = useCallback((next: React.SetStateAction<T>) => {
  setState(previous => {
   const state = previous.endpoint === endpoint ? previous : {
    endpoint, value: initialValue.current, error: '', loading: !!endpoint,
   }
   return {...state, value: typeof next === 'function' ? (next as (old: T) => T)(state.value) : next}
  })
 }, [endpoint])
 const visible = state.endpoint === endpoint ? state : {
  value: initialValue.current, error: '', loading: !!endpoint,
 }
 return {value: visible.value, loading: visible.loading, error: visible.error, setValue, reload}
}
export const refresh=()=>window.dispatchEvent(new Event('dominion-refresh'))
export function Notice({message}:{message:string}){return message?<div role="alert" className="my-3 rounded-lg border border-rose-200 bg-rose-50 px-3 py-2 text-[12px] text-rose-700">{message}</div>:null}
export function InfoRow({label,value}:{label:string;value:React.ReactNode}){return <div className="flex flex-wrap items-center justify-between gap-3 border-b border-[#EFE9DF] py-2 text-[12px]"><span className="text-[#7C7469]">{label}</span><strong className="font-semibold">{value}</strong></div>}
export function Loading(){return <span className="inline-flex items-center gap-2 text-[12px] text-[#7C7469]"><LoaderCircle size={14} className="animate-spin"/> {t('common.processing')}</span>}
export function ArrowLink({children,onClick}:{children:React.ReactNode;onClick:()=>void}){return <Button variant="ghost" onClick={onClick}>{children}<ArrowUpRight size={14}/></Button>}
