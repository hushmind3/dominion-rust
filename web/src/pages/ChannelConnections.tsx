import {useMemo,useState} from 'react'
import {Activity,AlertTriangle,Check,ExternalLink,MessageCircle,Plus,RefreshCcw,Search,Send,Settings2,ShieldCheck,Trash2,X} from 'lucide-react'
import {del,post} from '../api'
import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {Button,SelectInput,TextInput} from '../components/Primitives'
import {FormField,Notice,Panel,useRemote} from '../components/UI'
import {t,useLocale,type LocaleKey} from '../i18n'

type Field={key:string;label:string;type:'secret'|'text'|'url';required?:boolean;placeholder?:string}
type Entry={id:string;name:string;category:string;mode:string;description:string;docs:string;available:boolean;features:string[];fields:Field[];requiresRoom?:boolean}
type Room={id:string;name:string;agentIds?:string[]}
type Delivery={pending?:number;pendingIndex?:number|null;needsReview?:number;cursor?:number}
type Binding={id:string;name:string;kind:string;roomId:string;enabled:boolean;syncRoomMessages:boolean;status:string;runtimeError?:string|null;configured:boolean;credentialsPresent?:Record<string,boolean>;chatId?:string;channelId?:string;delivery?:Delivery}
type Draft={id?:string;name:string;kind:string;roomId:string;chatId:string;channelId:string;token:string;enabled:boolean;syncRoomMessages:boolean}
type Editor={entry:Entry;record:Draft;original?:Binding}
type Review={binding:Binding;entry:Entry}

const filters:[string,LocaleKey][]=[['all','conn.group.all'],['messaging','conn.group.messaging'],['community','conn.group.community']]
const states:Record<string,LocaleKey>={
 outbound_ready:'conn.status.outbound_ready',sync_disabled:'conn.status.sync_disabled',
 sending:'conn.status.sending',
 disabled:'conn.status.disabled',error:'conn.status.error',
 not_verified:'conn.status.not_verified',needs_review:'conn.status.needs_review'
}
function stateLabel(status:string){return states[status]?t(states[status]):status}
function stateColor(status:string){
 if(status==='outbound_ready')return 'border-emerald-200 bg-emerald-50 text-emerald-800'
 if(status==='needs_review'||status==='not_verified'||status==='sending')return 'border-amber-200 bg-amber-50 text-amber-900'
 if(status==='error')return 'border-rose-200 bg-rose-50 text-rose-800'
 return 'border-stone-200 bg-stone-100 text-stone-600'
}
function asDraft(entry:Entry,old?:Binding):Draft{
 return {id:old?.id,name:old?.name||entry.name,kind:entry.id,
  roomId:old?.roomId||'',chatId:old?.chatId||'',channelId:old?.channelId||'',
  token:'',enabled:old?.enabled??true,syncRoomMessages:old?.syncRoomMessages??true}
}
function validation(editor:Editor,rooms:Room[]):LocaleKey|null{
 const {entry,record,original}=editor
 if(!record.name.trim()||record.name.length>120)return 'conn.form.nameRequired'
 if(entry.requiresRoom&&(!record.roomId||!rooms.some(room=>room.id===record.roomId)))return 'conn.form.roomRequired'
 const destination=entry.id==='telegram'?record.chatId:record.channelId
 if(!destination||destination.length>128||!(/^-?[0-9]+$/.test(destination)&&
  (entry.id==='telegram'||/^[0-9]+$/.test(destination))))return 'conn.form.targetRequired'
 if(!record.token&&!original?.credentialsPresent?.token)return 'conn.form.tokenRequired'
 if(record.token&&(record.token.length>512||/\s|[\u0000-\u001f\u007f]/u.test(record.token)))return 'conn.form.tokenRequired'
 return null
}

export function ChannelConnections(){
 useLocale()
 const registry=useRemote<Entry[]>('/api/channels/catalog',[])
 const channels=useRemote<Binding[]>('/api/channels',[])
 const rooms=useRemote<Room[]>('/api/rooms',[])
 const [query,setQuery]=useState('')
 const [category,setCategory]=useState('all')
 const [editor,setEditor]=useState<Editor|null>(null)
 const [review,setReview]=useState<Review|null>(null)
 const [busy,setBusy]=useState('')
 const [attempted,setAttempted]=useState(false)
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 const [checks,setChecks]=useState<Record<string,string>>({})
 const filtered=useMemo(()=>registry.value
  .filter(entry=>(category==='all'||entry.category===category)&&
   [entry.name,entry.id,entry.description].join(' ').toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()))
  .sort((a,b)=>Number(b.available)-Number(a.available)||a.name.localeCompare(b.name)),
  [registry.value,category,query])
 const ready=channels.value.filter(channel=>channel.status==='outbound_ready').length
 const pending=channels.value.filter(channel=>channel.status==='needs_review').length
 const open=(entry:Entry,old?:Binding)=>{
  if(!entry.available||busy)return
  setError('');setNotice('');setAttempted(false)
  setEditor({entry,record:asDraft(entry,old),original:old})
 }
 const editField=<K extends keyof Draft>(key:K,value:Draft[K])=>
  setEditor(previous=>previous?{...previous,record:{...previous.record,[key]:value}}:previous)
 const refresh=async()=>{
  if(busy)return
  setBusy('refresh');setError('')
  try{await Promise.all([registry.reload(),channels.reload(),rooms.reload()])}
  finally{setBusy('')}
 }
 const save=async()=>{
  if(!editor||busy)return
  setAttempted(true)
  const failure=validation(editor,rooms.value)
  if(failure)return
  setBusy('save');setError('');setNotice('')
  const name=editor.record.name.trim()
  try{
   // Send exactly the supported outbound contract. No invented webhook,
   // inbound agent, model, or Discord bot-permission fields are persisted.
   const saved=await post<Binding>('/api/channels',{
    ...editor.record,name,token:editor.record.token.trim()
   })
   setEditor(null)
   setNotice(t('conn.dlg.saved',{name}))
   if(saved.enabled){
    try{
     const result=await post<{verified:boolean;message:string}>(
      '/api/channels/'+encodeURIComponent(saved.id)+'/diagnose',{})
     setChecks(previous=>({...previous,[saved.id]:result.verified?
      t('conn.dlg.verified'):result.message}))
    }catch(cause){
     setChecks(previous=>({...previous,[saved.id]:(cause as Error).message}))
    }
   }
   await channels.reload()
  }catch(cause){setError((cause as Error).message)}
  finally{setBusy('')}
 }
 const diagnose=async(binding:Binding)=>{
  if(busy)return
  setBusy('check:'+binding.id);setError('')
  setChecks(previous=>({...previous,[binding.id]:t('conn.dlg.diagnosing')}))
  try{
   const result=await post<{verified:boolean;message:string}>(
    '/api/channels/'+encodeURIComponent(binding.id)+'/diagnose',{})
   setChecks(previous=>({...previous,[binding.id]:result.verified?
    t('conn.dlg.verified'):result.message}))
   await channels.reload()
  }catch(cause){
   const message=(cause as Error).message
   setChecks(previous=>({...previous,[binding.id]:message}))
   setError(message)
  }finally{setBusy('')}
 }
 const remove=async(binding:Binding)=>{
  if(busy||!window.confirm(t('conn.dlg.removeConfirm',{name:binding.name})))return
  setBusy('delete:'+binding.id);setError('');setNotice('')
  try{
   await del('/api/channels/'+encodeURIComponent(binding.id))
   await channels.reload()
   setNotice(t('conn.form.removed',{name:binding.name}))
  }catch(cause){setError((cause as Error).message)}
  finally{setBusy('')}
 }
 const resolve=async(resolution:'confirmed_delivered'|'confirmed_not_delivered')=>{
  if(!review||busy)return
  setBusy('review:'+review.binding.id);setError('')
  try{
   await post('/api/channels/'+encodeURIComponent(review.binding.id)+'/delivery/reconcile',{resolution})
   await channels.reload()
   setNotice(t(resolution==='confirmed_delivered'?'conn.review.deliveredDone':'conn.review.notDeliveredDone'))
   setReview(null)
  }catch(cause){setError((cause as Error).message)}
  finally{setBusy('')}
 }
 const editorError=editor?validation(editor,rooms.value):null
 const locked=Boolean(editor?.original?.delivery?.pendingIndex!==undefined&&
  editor.original.delivery.pendingIndex!==null)
 return <div className="space-y-5">
  <div className="flex flex-wrap items-center justify-between gap-3">
   <div><h2 className="m-0 text-lg font-bold text-[#24211D]">{t('conn.channels.heading')}</h2>
    <p className="mt-1 max-w-3xl text-[13px] leading-6 text-[#7C7469]">{t('conn.channels.description')}</p></div>
   <Button variant="secondary" disabled={Boolean(busy)} onClick={()=>void refresh()}>
    <RefreshCcw size={16}/>{t('conn.refresh')}</Button>
  </div>
  <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
   {[
    [t('conn.platforms'),registry.value.filter(e=>e.available).length],
    [t('conn.channels.configured'),channels.value.length],
    [t('conn.outbound'),ready],
    [t('conn.channels.review'),pending]
   ].map(([label,count])=><div key={label} className="rounded-xl border border-[#E7E0D5] bg-white px-4 py-3 shadow-sm">
    <p className="text-xs text-[#7C7469]">{label}</p><strong className="mt-1 block text-xl text-[#24211D]">{count}</strong>
   </div>)}
  </div>
  <div className="flex flex-wrap items-center gap-3">
   <label className="flex min-w-[180px] flex-1 items-center gap-2 rounded-xl border border-[#E7E0D5] bg-white px-3">
    <Search size={17} className="shrink-0 text-[#7C7469]"/>
    <TextInput className="!border-0 !bg-transparent !shadow-none" value={query}
     onChange={event=>setQuery(event.target.value)} placeholder={t('conn.search')} aria-label={t('conn.search')}/>
   </label>
   <div className="flex flex-wrap gap-1" role="group" aria-label={t('conn.channels.filter')}>
    {filters.map(([id,key])=><Button key={id} size="sm" variant={category===id?'primary':'ghost'}
     aria-pressed={category===id} onClick={()=>setCategory(id)}>{t(key)}</Button>)}
   </div>
  </div>
  {registry.error&&<Notice message={registry.error}/>}
  {channels.error&&<Notice message={channels.error}/>}
  {rooms.error&&<Notice message={rooms.error}/>}
  {error&&<Notice message={error}/>}
  {notice&&<p role="status" className="rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-800"><Check size={16} className="mr-2 inline"/>{notice}</p>}
  {registry.loading&&!registry.value.length&&<p role="status" className="p-5 text-sm text-[#7C7469]">{t('cap.loading')}</p>}
  <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
   {filtered.map(entry=>{
    const bound=channels.value.filter(channel=>channel.kind===entry.id)
    const Icon=entry.id==='telegram'?Send:MessageCircle
    return <Panel key={entry.id} className="flex flex-col gap-4 p-5">
     <div className="flex items-start gap-3">
      <div className="grid size-11 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]"><Icon size={22}/></div>
      <div className="min-w-0 flex-1"><div className="flex flex-wrap items-center justify-between gap-2">
       <h3 className="text-base font-bold text-[#24211D]">{entry.name}</h3>
       <span className="rounded-lg bg-[#F4EEE5] px-2 py-1 text-[11px] font-semibold text-[#796451]">{t('conn.outboundOnly')}</span></div>
       <p className="mt-1 text-[12px] leading-5 text-[#7C7469]">{entry.description}</p>
      </div>
     </div>
     <div className="space-y-2">
      {bound.map(binding=>{
       const room=rooms.value.find(item=>item.id===binding.roomId)
       const target=entry.id==='telegram'?binding.chatId:binding.channelId
       const uncertain=binding.delivery?.pendingIndex!==null&&binding.delivery?.pendingIndex!==undefined&&!(binding.delivery?.sending)
       return <div key={binding.id} className="rounded-xl border border-[#E7E0D5] bg-[#FAF8F4] p-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
         <strong className="break-words text-sm text-[#24211D]">{binding.name}</strong>
         <span className={'rounded-lg border px-2 py-1 text-[11px] font-semibold '+stateColor(binding.status)}>
          {stateLabel(binding.status)}</span>
        </div>
        <p className="mt-2 break-words text-xs leading-5 text-[#746B60]">
         {t('conn.room')} · {room?.name||t('conn.roomDeleted')}<span className="px-1.5">·</span>
         {t('conn.target')} · <code className="break-all">{target}</code>
        </p>
        {binding.runtimeError&&<p role="status" className="mt-2 break-words text-xs text-rose-800">{binding.runtimeError}</p>}
        {checks[binding.id]&&<p role="status" className="mt-2 break-words text-xs text-[#75634F]">{checks[binding.id]}</p>}
        {uncertain&&<div className="mt-3 rounded-lg border border-amber-200 bg-amber-50 p-3">
         <p className="flex items-center gap-2 text-xs font-semibold text-amber-900"><AlertTriangle size={16}/>{t('conn.review.warning')}</p>
         <p className="mt-1 text-xs leading-5 text-amber-800">{t('conn.review.hint')}</p>
         <Button size="sm" variant="secondary" className="mt-2" disabled={Boolean(busy)}
          onClick={()=>{setError('');setReview({binding,entry})}}>{t('conn.review.action')}</Button>
        </div>}
        <div className="mt-3 flex flex-wrap justify-end gap-1 border-t border-[#E7E0D5] pt-2">
         <Button size="sm" variant="ghost" disabled={Boolean(busy)} onClick={()=>open(entry,binding)}>
          <Settings2 size={15}/>{t('conn.dlg.settings')}</Button>
         <Button size="sm" variant="ghost" disabled={Boolean(busy)} onClick={()=>void diagnose(binding)}>
          <Activity size={15}/>{busy==='check:'+binding.id?t('conn.dlg.diagnosing'):t('conn.dlg.diagnose')}</Button>
         <Button size="sm" variant="ghost" className="text-rose-700" disabled={Boolean(busy)}
          onClick={()=>void remove(binding)}><Trash2 size={15}/>{t('conn.dlg.delete')}</Button>
        </div>
       </div>
      })}
      {!bound.length&&<p className="rounded-lg border border-dashed border-[#E7E0D5] p-4 text-center text-xs text-[#8A8176]">{t('conn.channels.noBinding')}</p>}
     </div>
     <div className="mt-auto flex items-center justify-between gap-3 border-t border-[#EFE9DF] pt-3">
      <a className="inline-flex items-center gap-1 text-xs font-semibold text-[#A94E30] underline" href={entry.docs}
       target="_blank" rel="noopener noreferrer">{t('conn.docs')} <ExternalLink size={13}/></a>
      {entry.available&&<Button size="sm" variant="secondary" disabled={Boolean(busy)} onClick={()=>open(entry)}>
       <Plus size={15}/>{bound.length?t('conn.addAccount'):t('conn.setup')}</Button>}
     </div>
    </Panel>
   })}
  </div>
  {!registry.loading&&!filtered.length&&<Panel className="p-6 text-center text-sm text-[#7C7469]">{t('conn.noResults')}</Panel>}
  <div className="flex gap-3 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-4">
   <ShieldCheck size={20} className="shrink-0 text-[#A94E30]"/>
   <div><strong className="text-sm text-[#24211D]">{t('conn.noteTitle')}</strong>
    <p className="mt-1 text-xs leading-5 text-[#7C7469]">{t('conn.note')}</p></div>
  </div>
  {editor&&<DialogBackdrop onDismiss={()=>{if(!busy)setEditor(null)}}><DialogPanel size="wide" aria-label={editor.record.id?t('conn.edit'):t('conn.new')}>
   <div className="flex items-center justify-between border-b border-[#EFE9DF] p-5">
    <div><p className="text-xs font-semibold uppercase tracking-wide text-[#7C7469]">{editor.entry.name} · {t('conn.outboundOnly')}</p>
     <h2 className="mt-1 text-lg font-bold text-[#24211D]">{editor.record.id?t('conn.edit'):t('conn.new')}</h2></div>
    <Button size="icon" variant="ghost" aria-label={t('conn.cancel')} disabled={Boolean(busy)} onClick={()=>setEditor(null)}><X size={18}/></Button>
   </div>
   <div className="grid gap-5 p-5">
    <p className="text-sm text-[#756B60]">{t('conn.channels.description')}</p>
    {locked&&<p role="alert" className="rounded-lg border border-amber-200 bg-amber-50 p-3 text-xs text-amber-900">{t('conn.form.pendingLocked')}</p>}
    <section className="grid gap-4" aria-label={t('conn.form.destinationTitle')}>
     <h3 className="text-sm font-bold text-[#24211D]">{t('conn.form.destinationTitle')}</h3>
     <FormField label={t('conn.name')}><TextInput autoFocus maxLength={120} value={editor.record.name}
      onChange={event=>editField('name',event.target.value)}/></FormField>
     <FormField label={t('conn.roomOrAgent')}>
      <SelectInput value={editor.record.roomId} disabled={locked}
       onChange={event=>editField('roomId',event.target.value)}>
       <option value="">{t('conn.pickRoom')}</option>
       {rooms.value.map(room=><option key={room.id} value={room.id}>{room.name} ({room.agentIds?.length||0})</option>)}
      </SelectInput>
     </FormField>
     {editor.entry.fields.map(field=><FormField key={field.key}
      label={field.label+(field.required?' *':'')}
      hint={field.type==='secret'&&editor.record.id?t('conn.dlg.secretHint'):undefined}>
      <TextInput type={field.type==='secret'?'password':'text'}
       autoComplete={field.type==='secret'?'new-password':'off'} spellCheck={false}
       disabled={locked&&(field.type==='secret'||field.key==='chatId'||field.key==='channelId')}
       value={String(editor.record[field.key as keyof Draft]??'')}
       onChange={event=>editField(field.key as keyof Draft,event.target.value as never)}
       placeholder={field.placeholder||''}/>
     </FormField>)}
    </section>
    <section className="grid gap-3 rounded-xl border border-[#E7E0D5] bg-[#FAF8F4] p-4"
     aria-label={t('conn.form.deliveryTitle')}>
     <h3 className="text-sm font-bold text-[#24211D]">{t('conn.form.deliveryTitle')}</h3>
     <label className="flex items-start gap-3 text-sm text-[#4A443C]"><TextInput type="checkbox"
      checked={editor.record.enabled} onChange={event=>editField('enabled',event.target.checked)}/>
      <span>{t('conn.enable')}<small className="mt-1 block text-xs text-[#7C7469]">{t('conn.form.enabledHint')}</small></span></label>
     <label className="flex items-start gap-3 text-sm text-[#4A443C]"><TextInput type="checkbox"
      checked={editor.record.syncRoomMessages}
      onChange={event=>editField('syncRoomMessages',event.target.checked)}/>
      <span>{t('conn.dlg.syncRoom')}</span></label>
    </section>
    {attempted&&editorError&&<p role="alert" className="rounded-lg border border-rose-200 bg-rose-50 p-3 text-xs text-rose-800">{t(editorError)}</p>}
    <p className="text-xs leading-5 text-[#7C7469]">{t('conn.form.verifyHint')}</p>
    {error&&<Notice message={error}/>}
   </div>
   <div className="flex flex-wrap justify-end gap-2 border-t border-[#EFE9DF] p-5">
    <Button variant="secondary" disabled={Boolean(busy)} onClick={()=>setEditor(null)}>{t('conn.cancel')}</Button>
    <Button variant="primary" disabled={Boolean(busy)} onClick={()=>void save()}>
     <Check size={16}/>{busy==='save'?t('common.processing'):t('conn.save')}</Button>
   </div>
  </DialogPanel></DialogBackdrop>}
  {review&&<DialogBackdrop onDismiss={()=>{if(!busy)setReview(null)}}><DialogPanel aria-label={t('conn.review.title')} className="p-5">
   <div className="flex items-start justify-between gap-3">
    <div><h2 className="text-lg font-bold text-[#24211D]">{t('conn.review.title')}</h2>
     <p className="mt-2 text-sm text-[#7C7469]">{review.entry.name} · {review.binding.name}</p></div>
    <Button size="icon" variant="ghost" aria-label={t('conn.cancel')} disabled={Boolean(busy)} onClick={()=>setReview(null)}><X size={18}/></Button>
   </div>
   <p className="mt-4 rounded-xl border border-amber-200 bg-amber-50 p-4 text-sm leading-6 text-amber-900">{t('conn.review.explanation')}</p>
   {error&&<Notice message={error}/>}
   <div className="mt-5 grid gap-2">
    <Button variant="secondary" disabled={Boolean(busy)} onClick={()=>void resolve('confirmed_delivered')}>
     <Check size={15}/>{t('conn.review.confirmDelivered')}</Button>
    <Button variant="secondary" disabled={Boolean(busy)} onClick={()=>void resolve('confirmed_not_delivered')}>
     <RefreshCcw size={15}/>{t('conn.review.confirmNotDelivered')}</Button>
    <Button variant="ghost" disabled={Boolean(busy)} onClick={()=>setReview(null)}>{t('conn.cancel')}</Button>
   </div>
  </DialogPanel></DialogBackdrop>}
 </div>
}
