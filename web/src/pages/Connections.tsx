import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {StatusPill} from '../components/StatusPill'
import {Button,TextInput,SelectInput} from '../components/Primitives'
import {useEffect,useMemo,useState} from 'react'
import {Activity,ArrowRight,Check,ChevronRight,Cloud,ExternalLink,Filter,Globe2,Github,KeyRound,Link2,LockKeyhole,MessageCircle,Plus,RadioTower,Search,Send,Settings2,ShieldCheck,Trash2,UsersRound,X} from 'lucide-react'
import {del,openOAuth,post} from '../api'
import {Action,FormField,Headline,Notice,Panel,StateTag,Subheading,useRemote} from '../components/UI'
import {t,useLocale,type LocaleKey} from '../i18n'
import {ExtensionsPage} from './Extensions'
import {CapabilitiesPage} from './Capabilities'

type ChannelField={key:string;label:string;type:'secret'|'text'|'url';required?:boolean;placeholder?:string}
type ChannelEntry={id:string;name:string;category:string;mode:string;description:string;docs:string;available:boolean;status:string;features:string[];fields:ChannelField[];instructions?:string[];requiresPublicUrl?:boolean;requiresRoom?:boolean;maxRoomParticipants?:number}
type Channel={id:string;name:string;kind:string;agentId:string;providerId:string;model:string;enabled:boolean;mentionOnly:boolean;roomId?:string;roomPolicy?:string;status:string;runtimeError?:string;configured:boolean;missing?:string[];credentialsPresent:Record<string,boolean>;channelId?:string;chatId?:string;delivery?:{pending?:number;sending?:number;sent?:number;needsReview?:number;lastError?:string};toolsEnabled?:boolean;replyMentionBots?:boolean;webhookPath?:string}
type OAuthProvider={id:string;configured:boolean;scope:string}
type Connection={id:string;provider:string;displayName:string;scopes:string;hasToken:boolean}
const groups=[{id:'all',name:'conn.group.all'},{id:'messaging',name:'conn.group.messaging'},{id:'workplace',name:'conn.group.workplace'},{id:'community',name:'conn.group.community'},{id:'decentralized',name:'conn.group.decentralized'}] as const
const badge=(s:string)=>{const keys:Record<string,LocaleKey>={connected:'conn.status.connected',outbound_ready:'conn.status.outbound_ready',sync_disabled:'conn.status.sync_disabled',webhook_ready:'conn.status.webhook_ready',starting:'conn.status.starting',reconnecting:'conn.status.reconnecting',error:'conn.status.error',disabled:'conn.status.disabled',not_started:'conn.status.not_started',stopped:'conn.status.stopped'};return keys[s]?t(keys[s]):s};
const active=(s:string)=>s==='connected';

export function ConnectionsPage(){
 useLocale()
 const registry=useRemote<ChannelEntry[]>('/api/channels/catalog',[]);
 const channels=useRemote<Channel[]>('/api/channels',[]);
 const oauth=useRemote<OAuthProvider[]>('/api/oauth/providers',[]);
 const accounts=useRemote<Connection[]>('/api/oauth/connections',[]);
 const agents=useRemote<any[]>('/api/agents',[]);
 const rooms=useRemote<any[]>('/api/rooms',[]);
 const providers=useRemote<any[]>('/api/providers',[]);
 const [query,setQuery]=useState(''),[category,setCategory]=useState('all'),[view,setView]=useState<'channels'|'accounts'>('channels');
 const [section,setSection]=useState<'channels'|'extensions'|'capabilities'>('channels');
 const [editor,setEditor]=useState<{entry:ChannelEntry;record:any}|null>(null);
 const [error,setError]=useState(''),[info,setInfo]=useState(''),[probe,setProbe]=useState<Record<string,string>>({}),[saving,setSaving]=useState(false);
 const installed=useMemo(()=>new Set(channels.value.map(x=>x.kind)),[channels.value]);
 const filtered=useMemo(()=>registry.value.filter(x=>(category==='all'||x.category===category)&&[x.name,x.id,x.description].join(' ').toLowerCase().includes(query.toLowerCase())),[registry.value,category,query]);
 const online=channels.value.filter(c=>active(c.status)).length;
 const outbound=channels.value.filter(c=>c.status==='outbound_ready').length;
 const ready=registry.value.filter(x=>x.available).length;
 const openEditor=(entry:ChannelEntry,existing?:Channel)=>{
  if(!entry.available)return;
  const record:any=existing?{...existing}:{name:entry.name,kind:entry.id,enabled:false,syncRoomMessages:entry.mode==='outbound',mentionOnly:true,roomId:'',roomPolicy:'all_or_named',agentId:'',providerId:'',model:''};
  for(const field of entry.fields)if(field.type==='secret')record[field.key]='';
  setEditor({entry,record});setError('');
 };
 const save=async()=>{
  if(!editor||saving)return;
  setSaving(true);setError('');
  try{
   await post('/api/channels',editor.record);
   setEditor(null);setInfo(t('conn.dlg.saved',{name:editor.entry.name}));
   await channels.reload();
  }catch(e){setError((e as Error).message)}finally{setSaving(false)}
 };
 const diagnose=async(id:string)=>{
  setProbe(x=>({...x,[id]:t('conn.dlg.diagnosing')}));
  try{const r=await post<any>('/api/channels/'+id+'/diagnose',{});
   setProbe(x=>({...x,[id]:r.verified?t('conn.dlg.verified'):r.message||t('conn.dlg.checkNeeded')}));
  }catch(e){setProbe(x=>({...x,[id]:(e as Error).message}))}
 };
 const connect=async(id:string)=>{
  setError('');
  try{await openOAuth(id)}catch(e){setError((e as Error).message)}
 };
 const sectionTabs=<div className="mb-4 flex flex-wrap items-center gap-2">
  <Button variant={section==='channels'?'primary':'ghost'} onClick={()=>setSection('channels')}>연결 · 채널</Button>
  <Button variant={section==='extensions'?'primary':'ghost'} onClick={()=>setSection('extensions')}>MCP · 스킬</Button>
  <Button variant={section==='capabilities'?'primary':'ghost'} onClick={()=>setSection('capabilities')}>역량 마켓</Button>
 </div>;
 if(section!=='channels'){
  return <div className="mx-auto w-full max-w-[1320px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
   {sectionTabs}
   {section==='extensions'?<ExtensionsPage/>:<CapabilitiesPage navigate={page=>setSection(page==='extensions'?'extensions':'channels')}/>}
  </div>
 }
 return <div className="mx-auto w-full max-w-[1320px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
  {sectionTabs}
  <Headline eyebrow={t('ui.eyebrow.connections')} title={t('conn.title')} description={t('conn.description')} actions={<Action variant="secondary" onClick={()=>{void registry.reload();void channels.reload()}}><Activity size={16}/> {t('conn.refresh')}</Action>}/>
  <div className="mb-5 grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-4">
   <div className="flex items-center gap-4 rounded-xl border border-[#E7E0D5] bg-white p-5 shadow-sm"><span className="grid size-12 place-items-center rounded-xl bg-[#F3EEE5] text-[#7C7469]"><Globe2 size={20}/></span><div><small className="block text-[11px] text-[#8A8176]">{t('conn.platforms')}</small><strong className="mt-0.5 block text-[19px] font-bold leading-none text-[#24211D]">{registry.value.length}</strong></div></div>
   <div className="flex items-center gap-4 rounded-xl border border-[#E7E0D5] bg-white p-5 shadow-sm"><span className="grid size-12 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]"><RadioTower size={20}/></span><div><small className="block text-[11px] text-[#8A8176]">{t('conn.code')}</small><strong className="mt-0.5 block text-[19px] font-bold leading-none text-[#24211D]">{ready}</strong></div></div>
   <div className="flex items-center gap-4 rounded-xl border border-[#E7E0D5] bg-white p-5 shadow-sm"><span className="grid size-12 place-items-center rounded-xl bg-teal-50 text-teal-600"><Check size={20}/></span><div><small className="block text-[11px] text-[#8A8176]">{t('conn.connected')}</small><strong className="mt-0.5 block text-[19px] font-bold leading-none text-[#24211D]">{online}</strong></div></div>
   <div className="flex items-center gap-4 rounded-xl border border-[#E7E0D5] bg-white p-5 shadow-sm"><span className="grid size-12 place-items-center rounded-xl bg-[#F3EEE5] text-[#7C7469]"><Send size={20}/></span><div><small className="block text-[11px] text-[#8A8176]">{t('conn.outbound')}</small><strong className="mt-0.5 block text-[19px] font-bold leading-none text-[#24211D]">{outbound}</strong></div></div>
  </div>
  <div className="mb-6 flex flex-wrap items-center gap-2 border-b border-[#E7E0D5] pb-3"><Button variant={view==='channels'?'primary':'ghost'} onClick={()=>setView('channels')}><RadioTower size={17}/> {t('conn.tab.channels')} <b>{registry.value.length}</b></Button><Button variant={view==='accounts'?'primary':'ghost'} onClick={()=>setView('accounts')}><Link2 size={17}/> {t('conn.tab.accounts')} <b>{accounts.value.length}</b></Button></div>
  {error&&<Notice message={error}/>}
  {info&&<div className="my-3 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-700"><Check size={16}/>{info}</div>}
  {view==='channels'&&<>
   <div className="mb-5 flex flex-wrap items-center gap-3"><div className="flex min-w-0 flex-1 items-center gap-2 rounded-xl border border-[#E7E0D5] bg-white px-3 py-2"><Search size={17}/><TextInput value={query} onChange={e=>setQuery(e.target.value)} placeholder={t('conn.search')}/></div><div className="flex flex-wrap items-center gap-2"><Filter size={15}/>{groups.map(g=><Button variant={category===g.id?'primary':'ghost'} onClick={()=>setCategory(g.id)} key={g.id}>{t(g.name)}</Button>)}</div></div>
   {registry.error&&<Notice message={registry.error}/>}
   <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
    {filtered.map(entry=>{const bindings=channels.value.filter(c=>c.kind===entry.id);return <Panel key={entry.id} className="flex min-w-0 flex-col gap-3 rounded-xl border border-[#E7E0D5] bg-white p-5 shadow-sm">
     <div className="flex items-start justify-between gap-3"><div className="grid size-11 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]"><MessageCircle size={23}/></div><StatusPill status={entry.available?'available':'planned'}>{entry.available?t('conn.available'):t('conn.unavailable')}</StatusPill></div>
     <h3>{entry.name}</h3><p>{entry.description}</p>
     <div className="flex flex-wrap gap-1.5">{entry.features.map(f=><span key={f}>{f}</span>)}<span>{entry.mode==='outbound'?t('conn.outboundOnly'):entry.mode}</span></div>
     {bindings.map(c=><div className="flex items-center gap-2 text-xs text-[#7C7469]" key={c.id}><div ><strong>{c.name}</strong>{c.roomId&&<small>{t('conn.room')} · {rooms.value.find(r=>r.id===c.roomId)?.name||t('conn.roomDeleted')}</small>}{entry.mode==='outbound'&&(c.chatId||c.channelId)&&<small>{t('conn.target')} · {c.chatId||c.channelId}</small>}<small>{badge(c.status)}{c.runtimeError?' · '+c.runtimeError:''}</small>{entry.mode==='outbound'&&c.delivery&&<small>{t('conn.delivery',{pending:c.delivery.pending??0,sending:c.delivery.sending??0,sent:c.delivery.sent??0,review:c.delivery.needsReview??0})}</small>}{entry.mode==='outbound'&&c.delivery?.lastError&&<small className="text-[#A94E30]">{t('conn.lastError')} · {c.delivery.lastError}</small>}{probe[c.id]&&<small>{probe[c.id]}</small>}</div><div className={active(c.status)?'size-2 rounded-full bg-emerald-500':'size-2 rounded-full bg-slate-300'}/><Button size="icon" variant="ghost" title={t('conn.dlg.settings')} onClick={()=>openEditor(entry,c)}><Settings2 size={16}/></Button><Button size="icon" variant="ghost" title={t('conn.dlg.diagnose')} onClick={()=>diagnose(c.id)}><Activity size={16}/></Button><Button size="icon" variant="ghost" title={t('conn.dlg.delete')} onClick={async()=>{if(confirm(t('conn.dlg.removeConfirm',{name:c.name}))){try{await del('/api/channels/'+c.id);await channels.reload()}catch(e){setError((e as Error).message)}}}}><Trash2 size={15}/></Button></div>)}
     <div className="mt-auto flex flex-wrap items-center justify-between gap-3 border-t border-[#EFE9DF] pt-3"><a href={entry.docs} target="_blank" rel="noreferrer">{t('conn.docs')} <ExternalLink size={13}/></a>{entry.available?<Button onClick={()=>openEditor(entry)}><Plus size={15}/> {bindings.length?t('conn.addAccount'):t('conn.setup')}</Button>:<span className="rounded-xl border border-amber-200 bg-amber-50 p-3 text-xs text-amber-800">{t('conn.noCode')}</span>}</div>
    </Panel>})}
   </div>
   {!filtered.length&&<Panel><div className="flex min-h-40 flex-col items-center justify-center gap-2 p-5 text-center text-[#7C7469]">{t('conn.noResults')}</div></Panel>}
   <Panel className="flex min-w-0 items-start gap-3 rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-4 text-sm leading-6"><div className="grid size-10 shrink-0 place-items-center rounded-xl bg-[#F1DCCF] text-[#A94E30]"><ShieldCheck size={21}/></div><div><strong>{t('conn.noteTitle')}</strong><p>{t('conn.note')}</p></div></Panel>
  </>}
  {view==='accounts'&&<>
   <Subheading title={t('conn.accounts')} extra={<span className="text-[10px] font-bold uppercase tracking-widest text-[#9C9388]">{t('ui.label.oauth')}</span>}/>
   <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">{oauth.value.map(item=>{const account=accounts.value.find(a=>a.provider===item.id);const Icon=item.id==='github'?Github:Globe2;return <Panel className="flex min-w-0 flex-col gap-3 p-5" key={item.id}><div className="grid size-11 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]"><Icon size={28}/></div><h3>{item.id==='github'?'GitHub':'Google · YouTube'}</h3><p>{item.id==='github'?t('conn.dlg.githubTitle'):t('conn.dlg.googleTitle')}</p><div className="integration-status"><StateTag online={Boolean(account)}>{account?t('conn.accountReady',{name:account.displayName}):item.configured?t('conn.oauthReady'):t('conn.oauthNeeded')}</StateTag></div><div className="mt-auto flex flex-wrap items-center gap-2"><Action variant="secondary" disabled={!item.configured} onClick={()=>connect(item.id)}><Link2 size={16}/>{account?t('conn.reconnect'):t('conn.connect')}</Action></div></Panel>})}</div>
  </>}
  {editor&&<DialogBackdrop onDismiss={()=>{if(!saving)setEditor(null)}}><DialogPanel size="wide">
   <div className="flex items-center justify-between border-b border-[#EFE9DF] p-5"><div><small>CHANNEL CONFIGURATION · {editor.entry.id.toUpperCase()}</small><h2>{editor.record.id?t('conn.edit'):t('conn.new')}</h2></div><Button size="icon" variant="ghost" onClick={()=>setEditor(null)}><X size={18}/></Button></div>
   <div className="grid gap-4 p-5">
    <div className="mb-5 flex items-center gap-4 rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-5"><div className="grid size-12 shrink-0 place-items-center rounded-xl bg-[#F1DCCF] text-[#C96442]"><MessageCircle size={21}/></div><div><strong>{editor.entry.name}</strong><p>{editor.entry.description}</p></div></div>
    <FormField label={t('conn.name')}><TextInput  value={editor.record.name||''} onChange={e=>setEditor({...editor,record:{...editor.record,name:e.target.value}})}/></FormField>
    {editor.entry.fields.map(field=><FormField key={field.key} label={field.label+(field.required?' *':'')} hint={field.type==='secret'&&editor.record.id?t('conn.dlg.secretHint'):undefined}><TextInput  type={field.type==='secret'?'password':field.type==='url'?'url':'text'} value={editor.record[field.key]||''} placeholder={field.placeholder||''} onChange={e=>setEditor({...editor,record:{...editor.record,[field.key]:e.target.value}})}/></FormField>)}
    <FormField label={editor.entry.requiresRoom?t('conn.roomOrAgent'):t('conn.replyMode')}><SelectInput  value={editor.record.roomId||''} onChange={e=>setEditor({...editor,record:{...editor.record,roomId:e.target.value}})}><option value="">{editor.entry.requiresRoom?t('conn.pickRoom'):t('conn.singleAgent')}</option>{rooms.value.map(r=><option value={r.id} key={r.id} disabled={!!editor.entry.maxRoomParticipants&&(r.agentIds?.length||0)>editor.entry.maxRoomParticipants}>{t('conn.dlg.roomOption',{name:r.name,count:r.agentIds?.length||0})}</option>)}</SelectInput></FormField>
    {editor.record.roomId&&<div className="rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-3 text-xs leading-6 text-blue-800"><UsersRound size={19}/><div><strong>{t('conn.roomNote')}</strong><p>{editor.entry.maxRoomParticipants?t('conn.dlg.maxParticipants',{count:editor.entry.maxRoomParticipants}):editor.entry.mode==='outbound'?t('conn.dlg.outboundNote'):t('conn.dlg.roomPolicyNote')}</p></div></div>}
    {editor.record.roomId&&editor.entry.mode!=='outbound'&&<FormField label={t('conn.policy')}><SelectInput  value={editor.record.roomPolicy||'all_or_named'} onChange={e=>setEditor({...editor,record:{...editor.record,roomPolicy:e.target.value}})}><option value="all_or_named">{t('conn.dlg.policyAll')}</option><option value="named_only">{t('conn.dlg.policyNamed')}</option><option value="round_robin">{t('conn.dlg.policyRound')}</option></SelectInput></FormField>}
    {editor.entry.mode!=='outbound'&&<>
    <div className="grid grid-cols-1 gap-4 sm:grid-cols-2"><FormField label={t('conn.agent')}><SelectInput  value={editor.record.agentId||''} onChange={e=>setEditor({...editor,record:{...editor.record,agentId:e.target.value}})}><option value="">{t('conn.dlg.pickAgent')}</option>{agents.value.map(a=><option key={a.id} value={a.id}>{a.name}</option>)}</SelectInput></FormField><FormField label={t('conn.provider')}><SelectInput  value={editor.record.providerId||''} onChange={e=>setEditor({...editor,record:{...editor.record,providerId:e.target.value}})}><option value="">{t('conn.dlg.agentDefault')}</option>{providers.value.map(p=><option key={p.id} value={p.id}>{p.name}</option>)}</SelectInput></FormField></div>
    <FormField label={t('conn.model')}><TextInput  value={editor.record.model||''} onChange={e=>setEditor({...editor,record:{...editor.record,model:e.target.value}})} placeholder={t('conn.dlg.agentDefaultModel')}/></FormField>
    <label className="inline-flex items-center gap-2 text-sm text-[#4A443C]"><TextInput type="checkbox" checked={editor.record.mentionOnly!==false} onChange={e=>setEditor({...editor,record:{...editor.record,mentionOnly:e.target.checked}})}/> {t('conn.mentionOnly')}</label>
    </>}
    {editor.entry.id==='discord'&&<label className="inline-flex items-center gap-2 text-sm text-[#4A443C]"><TextInput type="checkbox" checked={Boolean(editor.record.replyMentionBots)} onChange={e=>setEditor({...editor,record:{...editor.record,replyMentionBots:e.target.checked}})}/> {t('conn.dlg.discordMention')}</label>}
    {editor.entry.id==='discord'&&<label className="inline-flex items-center gap-2 text-sm text-[#4A443C]"><TextInput type="checkbox" checked={Boolean(editor.record.toolsEnabled)} onChange={e=>setEditor({...editor,record:{...editor.record,toolsEnabled:e.target.checked}})}/> {t('conn.dlg.discordTools')}</label>}
    {editor.record.roomId&&<label className="inline-flex items-center gap-2 text-sm text-[#4A443C]"><TextInput type="checkbox" checked={Boolean(editor.record.syncRoomMessages)} onChange={e=>setEditor({...editor,record:{...editor.record,syncRoomMessages:e.target.checked}})}/> {t('conn.dlg.syncRoom')}</label>}
    <label className="inline-flex items-center gap-2 text-sm text-[#4A443C]"><TextInput type="checkbox" checked={Boolean(editor.record.enabled)} onChange={e=>setEditor({...editor,record:{...editor.record,enabled:e.target.checked}})}/> {t('conn.enable')}</label>
    {editor.record.webhookPath&&<div className="rounded-xl bg-[#FAF7F1] p-3 font-mono text-xs text-[#4A443C]"><strong>{t('conn.webhook')}</strong><code>{editor.record.webhookPath}</code><small>{t('conn.dlg.webhookNote')}</small></div>}
    {(editor.entry.instructions?.length||0)>0&&<div className="rounded-xl border border-[#E7E0D5] p-4 text-xs leading-6"><strong>{t('conn.instructions')}</strong><ol>{(editor.entry.instructions||[]).map((t,i)=><li key={i}>{t}</li>)}</ol><a href={editor.entry.docs} target="_blank" rel="noreferrer">{t('conn.openDocs')} <ExternalLink size={13}/></a></div>}
   </div>
   {error&&<Notice message={error}/>}
   <div className="flex flex-wrap justify-end gap-2 border-t border-[#EFE9DF] p-5"><Action variant="secondary" onClick={()=>setEditor(null)}>{t('conn.cancel')}</Action><Action onClick={save} disabled={saving||Boolean(editor.entry.requiresRoom&&!editor.record.roomId)}><Check size={16}/>{saving?t('conn.save'):t('conn.save')}</Action></div>
  </DialogPanel></DialogBackdrop>}
 </div>
}
