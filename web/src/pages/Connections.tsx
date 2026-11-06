import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {StatusPill} from '../components/StatusPill'
import {Button,TextInput,SelectInput} from '../components/Primitives'
import {useEffect,useMemo,useState} from 'react'
import {Activity,ArrowRight,Check,ChevronRight,Cloud,ExternalLink,Filter,Globe2,Github,KeyRound,Link2,LockKeyhole,MessageCircle,Plus,RadioTower,Search,Settings2,ShieldCheck,Trash2,UsersRound,X} from 'lucide-react'
import {del,openOAuth,post} from '../api'
import {Action,FormField,Headline,Notice,Panel,StateTag,Subheading,useRemote} from '../components/UI'

type ChannelField={key:string;label:string;type:'secret'|'text'|'url';required?:boolean;placeholder?:string}
type ChannelEntry={id:string;name:string;category:string;mode:string;description:string;docs:string;available:boolean;status:string;features:string[];fields:ChannelField[];instructions?:string[];requiresPublicUrl?:boolean;requiresRoom?:boolean;maxRoomParticipants?:number}
type Channel={id:string;name:string;kind:string;agentId:string;providerId:string;model:string;enabled:boolean;mentionOnly:boolean;roomId?:string;roomPolicy?:string;status:string;runtimeError?:string;configured:boolean;missing?:string[];credentialsPresent:Record<string,boolean>;channelId?:string;chatId?:string;delivery?:{pending?:number;sending?:number;sent?:number;needsReview?:number;lastError?:string};toolsEnabled?:boolean;replyMentionBots?:boolean;webhookPath?:string}
type OAuthProvider={id:string;configured:boolean;scope:string}
type Connection={id:string;provider:string;displayName:string;scopes:string;hasToken:boolean}
const groups=[{id:'all',name:'전체'},{id:'messaging',name:'메신저'},{id:'workplace',name:'업무'},{id:'community',name:'커뮤니티'},{id:'decentralized',name:'분산형'}]
const badge=(s:string)=>({connected:'연결됨',outbound_ready:'송신 설정됨',sync_disabled:'동기화 꺼짐',webhook_ready:'웹훅 대기',starting:'시작 중',reconnecting:'재연결 중',error:'오류',disabled:'비활성',not_started:'미시작',stopped:'중지'} as Record<string,string>)[s]||s;
const active=(s:string)=>s==='connected';

export function ConnectionsPage(){
 const registry=useRemote<ChannelEntry[]>('/api/channels/catalog',[]);
 const channels=useRemote<Channel[]>('/api/channels',[]);
 const oauth=useRemote<OAuthProvider[]>('/api/oauth/providers',[]);
 const accounts=useRemote<Connection[]>('/api/oauth/connections',[]);
 const agents=useRemote<any[]>('/api/agents',[]);
 const rooms=useRemote<any[]>('/api/rooms',[]);
 const providers=useRemote<any[]>('/api/providers',[]);
 const [query,setQuery]=useState(''),[category,setCategory]=useState('all'),[view,setView]=useState<'channels'|'accounts'>('channels');
 const [editor,setEditor]=useState<{entry:ChannelEntry;record:any}|null>(null);
 const [error,setError]=useState(''),[info,setInfo]=useState(''),[probe,setProbe]=useState<Record<string,string>>({}),[saving,setSaving]=useState(false);
 const installed=useMemo(()=>new Set(channels.value.map(x=>x.kind)),[channels.value]);
 const filtered=useMemo(()=>registry.value.filter(x=>(category==='all'||x.category===category)&&[x.name,x.id,x.description].join(' ').toLowerCase().includes(query.toLowerCase())),[registry.value,category,query]);
 const online=channels.value.filter(c=>active(c.status)).length;
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
   setEditor(null);setInfo(editor.entry.name+' 설정이 저장되었습니다.');
   await channels.reload();
  }catch(e){setError((e as Error).message)}finally{setSaving(false)}
 };
 const diagnose=async(id:string)=>{
  setProbe(x=>({...x,[id]:'진단 중'}));
  try{const r=await post<any>('/api/channels/'+id+'/diagnose',{});
   setProbe(x=>({...x,[id]:r.verified?'인증 확인됨':r.message||'연결 상태 확인 필요'}));
  }catch(e){setProbe(x=>({...x,[id]:(e as Error).message}))}
 };
 const connect=async(id:string)=>{
  setError('');
  try{await openOAuth(id)}catch(e){setError((e as Error).message)}
 };
 return <div className="mx-auto w-full max-w-[1320px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
  <Headline eyebrow="CHANNEL & INTEGRATION HUB" title="연결할 수 있는 모든 세계." description="메신저는 카탈로그에서 자동 발견합니다. 설치된 드라이버와 실제 인증 상태를 따로 표시합니다." actions={<Action variant="secondary" onClick={()=>{void registry.reload();void channels.reload()}}><Activity size={16}/> 상태 새로고침</Action>}/>
  <div className="mb-5 grid grid-cols-1 gap-4 md:grid-cols-3">
   <div className="flex items-center gap-4 rounded-xl border border-[#E7E0D5] bg-white p-5 shadow-sm"><span className="grid size-12 place-items-center rounded-xl bg-violet-50 text-violet-600"><Globe2 size={20}/></span><div><small>발견된 플랫폼</small><strong>{registry.value.length}</strong></div></div>
   <div className="flex items-center gap-4 rounded-xl border border-[#E7E0D5] bg-white p-5 shadow-sm"><span className="grid size-12 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]"><RadioTower size={20}/></span><div><small>연결 코드 있음</small><strong>{ready}</strong></div></div>
   <div className="flex items-center gap-4 rounded-xl border border-[#E7E0D5] bg-white p-5 shadow-sm"><span className="grid size-12 place-items-center rounded-xl bg-teal-50 text-teal-600"><Check size={20}/></span><div><small>연결됨</small><strong>{online}</strong></div></div>
  </div>
  <div className="mb-6 flex flex-wrap items-center gap-2 border-b border-[#E7E0D5] pb-3"><Button variant={view==='channels'?'primary':'ghost'} onClick={()=>setView('channels')}><RadioTower size={17}/> 메신저 · 채널 <b>{registry.value.length}</b></Button><Button variant={view==='accounts'?'primary':'ghost'} onClick={()=>setView('accounts')}><Link2 size={17}/> OAuth · 계정 <b>{accounts.value.length}</b></Button></div>
  {error&&<Notice message={error}/>}
  {info&&<div className="my-3 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-700"><Check size={16}/>{info}</div>}
  {view==='channels'&&<>
   <div className="mb-5 flex flex-wrap items-center gap-3"><div className="flex min-w-0 flex-1 items-center gap-2 rounded-xl border border-[#E7E0D5] bg-white px-3 py-2"><Search size={17}/><TextInput value={query} onChange={e=>setQuery(e.target.value)} placeholder="Telegram, Slack, Matrix, WhatsApp 등 검색"/></div><div className="flex flex-wrap items-center gap-2"><Filter size={15}/>{groups.map(g=><Button variant={category===g.id?'primary':'ghost'} onClick={()=>setCategory(g.id)} key={g.id}>{g.name}</Button>)}</div></div>
   {registry.error&&<Notice message={registry.error}/>}
   <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
    {filtered.map(entry=>{const bindings=channels.value.filter(c=>c.kind===entry.id);return <Panel key={entry.id} className="flex min-w-0 flex-col gap-3 rounded-xl border border-[#E7E0D5] bg-white p-5 shadow-sm">
     <div className="flex items-start justify-between gap-3"><div className="grid size-11 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]"><MessageCircle size={23}/></div><StatusPill status={entry.available?'available':'planned'}>{entry.available?'연결 코드 있음':'연결 코드 없음'}</StatusPill></div>
     <h3>{entry.name}</h3><p>{entry.description}</p>
     <div className="flex flex-wrap gap-1.5">{entry.features.map(f=><span key={f}>{f}</span>)}<span>{entry.mode==='outbound'?'송신 전용':entry.mode}</span></div>
     {bindings.map(c=><div className="flex items-center gap-2 text-xs text-[#7C7469]" key={c.id}><div ><strong>{c.name}</strong>{c.roomId&&<small>회의실 · {rooms.value.find(r=>r.id===c.roomId)?.name||'삭제된 회의실'}</small>}{entry.mode==='outbound'&&(c.chatId||c.channelId)&&<small>전송 대상 · {c.chatId||c.channelId}</small>}<small>{badge(c.status)}{c.runtimeError?' · '+c.runtimeError:''}</small>{entry.mode==='outbound'&&c.delivery&&<small>대기 {c.delivery.pending??0}건 · 전송 중 {c.delivery.sending??0}건 · 완료 {c.delivery.sent??0}건 · 확인 필요 {c.delivery.needsReview??0}건</small>}{entry.mode==='outbound'&&c.delivery?.lastError&&<small className="text-[#A94E30]">마지막 오류 · {c.delivery.lastError}</small>}{probe[c.id]&&<small>{probe[c.id]}</small>}</div><div className={active(c.status)?'size-2 rounded-full bg-emerald-500':'size-2 rounded-full bg-slate-300'}/><Button size="icon" variant="ghost" title="설정" onClick={()=>openEditor(entry,c)}><Settings2 size={16}/></Button><Button size="icon" variant="ghost" title="진단" onClick={()=>diagnose(c.id)}><Activity size={16}/></Button><Button size="icon" variant="ghost" title="삭제" onClick={async()=>{if(confirm(c.name+' 연결을 제거하시겠습니까?')){try{await del('/api/channels/'+c.id);await channels.reload()}catch(e){setError((e as Error).message)}}}}><Trash2 size={15}/></Button></div>)}
     <div className="mt-auto flex flex-wrap items-center justify-between gap-3 border-t border-[#EFE9DF] pt-3"><a href={entry.docs} target="_blank" rel="noreferrer">공식 문서 <ExternalLink size={13}/></a>{entry.available?<Button onClick={()=>openEditor(entry)}><Plus size={15}/> {bindings.length?'계정 추가':'연결 설정'}</Button>:<span className="rounded-xl border border-amber-200 bg-amber-50 p-3 text-xs text-amber-800">연결 코드 없음 · 설정 불가</span>}</div>
    </Panel>})}
   </div>
   {!filtered.length&&<Panel><div className="flex min-h-40 flex-col items-center justify-center gap-2 p-5 text-center text-[#7C7469]">검색된 채널이 없습니다.</div></Panel>}
   <Panel className="flex min-w-0 items-start gap-3 rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-4 text-sm leading-6"><div className="grid size-10 shrink-0 place-items-center rounded-xl bg-[#F1DCCF] text-[#A94E30]"><ShieldCheck size={21}/></div><div><strong>송신 설정과 인증 상태</strong><p>송신 전용 채널은 회의실 응답을 지정한 대상으로 보냅니다. 설정이 저장돼도 원격 인증이나 메시지 전송이 확인된 것은 아닙니다.</p></div></Panel>
  </>}
  {view==='accounts'&&<>
   <Subheading title="연결된 계정" extra={<span className="text-[10px] font-bold uppercase tracking-widest text-[#9C9388]">OAUTH CONNECTIONS</span>}/>
   <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">{oauth.value.map(item=>{const account=accounts.value.find(a=>a.provider===item.id);const Icon=item.id==='github'?Github:Globe2;return <Panel className="flex min-w-0 flex-col gap-3 p-5" key={item.id}><div className="grid size-11 shrink-0 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]"><Icon size={28}/></div><h3>{item.id==='github'?'GitHub':'Google · YouTube'}</h3><p>{item.id==='github'?'저장소와 계정 연결':'Google 계정과 YouTube 읽기 연결'}</p><div className="integration-status"><StateTag online={Boolean(account)}>{account?'연결됨 · '+account.displayName:item.configured?'OAuth 앱 준비됨':'OAuth 앱 설정 필요'}</StateTag></div><div className="mt-auto flex flex-wrap items-center gap-2"><Action variant="secondary" disabled={!item.configured} onClick={()=>connect(item.id)}><Link2 size={16}/>{account?'다시 연결':'계정 연결'}</Action></div></Panel>})}</div>
  </>}
  {editor&&<DialogBackdrop onDismiss={()=>{if(!saving)setEditor(null)}}><DialogPanel size="wide">
   <div className="flex items-center justify-between border-b border-[#EFE9DF] p-5"><div><small>CHANNEL CONFIGURATION · {editor.entry.id.toUpperCase()}</small><h2>{editor.record.id?'연결 설정 변경':'새 연결 설정'}</h2></div><Button size="icon" variant="ghost" onClick={()=>setEditor(null)}><X size={18}/></Button></div>
   <div className="grid gap-4 p-5">
    <div className="mb-5 flex items-center gap-4 rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-5"><div className="grid size-12 shrink-0 place-items-center rounded-xl bg-[#F1DCCF] text-[#C96442]"><MessageCircle size={21}/></div><div><strong>{editor.entry.name}</strong><p>{editor.entry.description}</p></div></div>
    <FormField label="연결 이름"><TextInput  value={editor.record.name||''} onChange={e=>setEditor({...editor,record:{...editor.record,name:e.target.value}})}/></FormField>
    {editor.entry.fields.map(field=><FormField key={field.key} label={field.label+(field.required?' *':'')} hint={field.type==='secret'&&editor.record.id?'기존 키는 표시하지 않습니다. 변경할 때만 입력하세요.':undefined}><TextInput  type={field.type==='secret'?'password':field.type==='url'?'url':'text'} value={editor.record[field.key]||''} placeholder={field.placeholder||''} onChange={e=>setEditor({...editor,record:{...editor.record,[field.key]:e.target.value}})}/></FormField>)}
    <FormField label={editor.entry.requiresRoom?"연결할 회의실":"응답 방식"}><SelectInput  value={editor.record.roomId||''} onChange={e=>setEditor({...editor,record:{...editor.record,roomId:e.target.value}})}><option value="">{editor.entry.requiresRoom?"회의실 선택":"단일 에이전트"}</option>{rooms.value.map(r=><option value={r.id} key={r.id} disabled={!!editor.entry.maxRoomParticipants&&(r.agentIds?.length||0)>editor.entry.maxRoomParticipants}>회의실 · {r.name} ({r.agentIds?.length||0}명)</option>)}</SelectInput></FormField>
    {editor.record.roomId&&<div className="rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-3 text-xs leading-6 text-blue-800"><UsersRound size={19}/><div><strong>이 채널은 회의실 실행기로 연결됩니다.</strong><p>{editor.entry.maxRoomParticipants?'이 채널은 참가자 최대 '+editor.entry.maxRoomParticipants+'명의 회의실을 연결할 수 있습니다.':editor.entry.mode==='outbound'?'회의실에서 생성된 응답을 지정한 채널로 보냅니다.':'회의실의 발언 설정에 따라 에이전트가 응답합니다.'}</p></div></div>}
    {editor.record.roomId&&editor.entry.mode!=='outbound'&&<FormField label="발언자 선택 정책"><SelectInput  value={editor.record.roomPolicy||'all_or_named'} onChange={e=>setEditor({...editor,record:{...editor.record,roomPolicy:e.target.value}})}><option value="all_or_named">이름 지정 시 해당 인원 · 지정 없으면 전체</option><option value="named_only">이름을 지정한 에이전트만</option><option value="round_robin">회차마다 1명씩 돌아가며 발언</option></SelectInput></FormField>}
    {editor.entry.mode!=='outbound'&&<>
    <div className="grid grid-cols-1 gap-4 sm:grid-cols-2"><FormField label="에이전트"><SelectInput  value={editor.record.agentId||''} onChange={e=>setEditor({...editor,record:{...editor.record,agentId:e.target.value}})}><option value="">에이전트 선택</option>{agents.value.map(a=><option key={a.id} value={a.id}>{a.name}</option>)}</SelectInput></FormField><FormField label="모델 제공자"><SelectInput  value={editor.record.providerId||''} onChange={e=>setEditor({...editor,record:{...editor.record,providerId:e.target.value}})}><option value="">에이전트 기본값</option>{providers.value.map(p=><option key={p.id} value={p.id}>{p.name}</option>)}</SelectInput></FormField></div>
    <FormField label="모델 ID · 선택"><TextInput  value={editor.record.model||''} onChange={e=>setEditor({...editor,record:{...editor.record,model:e.target.value}})} placeholder="에이전트 기본 모델 사용"/></FormField>
    <label className="inline-flex items-center gap-2 text-sm text-[#4A443C]"><TextInput type="checkbox" checked={editor.record.mentionOnly!==false} onChange={e=>setEditor({...editor,record:{...editor.record,mentionOnly:e.target.checked}})}/> 그룹에서는 봇 멘션이 있을 때만 답변</label>
    </>}
    {editor.entry.id==='discord'&&<label className="inline-flex items-center gap-2 text-sm text-[#4A443C]"><TextInput type="checkbox" checked={Boolean(editor.record.replyMentionBots)} onChange={e=>setEditor({...editor,record:{...editor.record,replyMentionBots:e.target.checked}})}/> 허용한 외부 봇에게 답할 때 해당 봇만 멘션 (자동 응답은 분당 2회)</label>}
    {editor.entry.id==='discord'&&<label className="inline-flex items-center gap-2 text-sm text-[#4A443C]"><TextInput type="checkbox" checked={Boolean(editor.record.toolsEnabled)} onChange={e=>setEditor({...editor,record:{...editor.record,toolsEnabled:e.target.checked}})}/> 허용 사용자 ID에 지정한 사람의 도구 사용 허용 (외부 봇은 대화만)</label>}
    {editor.record.roomId&&<label className="inline-flex items-center gap-2 text-sm text-[#4A443C]"><TextInput type="checkbox" checked={Boolean(editor.record.syncRoomMessages)} onChange={e=>setEditor({...editor,record:{...editor.record,syncRoomMessages:e.target.checked}})}/> 회의실에서 작성한 메시지와 응답을 이 채널로 보내기</label>}
    <label className="inline-flex items-center gap-2 text-sm text-[#4A443C]"><TextInput type="checkbox" checked={Boolean(editor.record.enabled)} onChange={e=>setEditor({...editor,record:{...editor.record,enabled:e.target.checked}})}/> 연결 활성화</label>
    {editor.record.webhookPath&&<div className="rounded-xl bg-[#FAF7F1] p-3 font-mono text-xs text-[#4A443C]"><strong>Webhook 경로</strong><code>{editor.record.webhookPath}</code><small>외부 플랫폼에서 접근할 수 있는 HTTPS 서버 주소를 이 경로 앞에 붙여 등록하세요.</small></div>}
    {(editor.entry.instructions?.length||0)>0&&<div className="rounded-xl border border-[#E7E0D5] p-4 text-xs leading-6"><strong>연결 안내</strong><ol>{(editor.entry.instructions||[]).map((t,i)=><li key={i}>{t}</li>)}</ol><a href={editor.entry.docs} target="_blank" rel="noreferrer">공식 문서 열기 <ExternalLink size={13}/></a></div>}
   </div>
   {error&&<Notice message={error}/>}
   <div className="flex flex-wrap justify-end gap-2 border-t border-[#EFE9DF] p-5"><Action variant="secondary" onClick={()=>setEditor(null)}>취소</Action><Action onClick={save} disabled={saving||Boolean(editor.entry.requiresRoom&&!editor.record.roomId)}><Check size={16}/>{saving?'저장 중':'설정 저장'}</Action></div>
  </DialogPanel></DialogBackdrop>}
 </div>
}
