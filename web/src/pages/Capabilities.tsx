import {useMemo,useState} from 'react'
import {ArrowRight,ExternalLink,Layers3,MessageCircle,RefreshCw,Search,TerminalSquare} from 'lucide-react'
import {Button,TextInput} from '../components/Primitives'
import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {Headline,Notice,Panel,StateTag,useRemote} from '../components/UI'

type Capability={id:string;name:string;description?:string;namespace:string;available?:boolean}
type Channel=Omit<Capability,'namespace'>&{mode:string;features?:string[];docs?:string;requiresRoom?:boolean;maxRoomParticipants?:number}
type Item=Capability&Partial<Channel>
type Page='connections'|'extensions'
const families=[
 {id:'all',label:'전체',icon:Layers3},
 {id:'channel',label:'메신저',icon:MessageCircle},
 {id:'builtin',label:'내장 도구',icon:TerminalSquare},
] as const

export function CapabilitiesPage({navigate}:{navigate:(page:Page)=>void}){
 const data=useRemote<{items:Capability[]}>('/api/capabilities',{items:[]})
 const catalog=useRemote<Channel[]>('/api/channels/catalog',[])
 const [family,setFamily]=useState<(typeof families)[number]['id']>('all')
 const [query,setQuery]=useState(''),[showPlanned,setShowPlanned]=useState(false)
 const [detail,setDetail]=useState<Item>(),[refreshing,setRefreshing]=useState(false)
 const items=useMemo<Item[]>(()=>[
  ...data.value.items.filter(item=>item.namespace==='builtin'),
  ...catalog.value.filter(item=>item.available||showPlanned).map(item=>({...item,namespace:'channel'})),
 ],[data.value.items,catalog.value,showPlanned])
 const needle=query.trim().toLowerCase()
 const visible=items.filter(item=>(family==='all'||item.namespace===family)&&
  [item.name,item.id,item.description].join(' ').toLowerCase().includes(needle))
 const reload=async()=>{
  setRefreshing(true)
  try{await Promise.all([data.reload(),catalog.reload()])}finally{setRefreshing(false)}
 }
 return <div className="mx-auto w-full max-w-[1320px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
  <Headline eyebrow="CAPABILITY CATALOG" title="역량 마켓" description="Rust 엔진이 제공하는 도구와 메신저 지원 범위를 확인하세요. 계정과 수신·전송 대상은 연결 · 채널에서 설정합니다." actions={<Button variant="secondary" disabled={refreshing} onClick={()=>void reload()}><RefreshCw size={16}/> 목록 새로고침</Button>}/>
  <Notice message={data.error||catalog.error}/>
  <div className="mb-4 flex items-center gap-2"><Search size={16}/><TextInput aria-label="역량 검색" value={query} onChange={event=>setQuery(event.target.value)} placeholder="이름이나 기능 검색"/></div>
  <div className="mb-5 flex flex-wrap items-center gap-2 border-b border-[#E7E0D5] pb-3">
   {families.map(({id,label,icon:Icon})=><Button key={id} variant={id===family?'primary':'ghost'} aria-pressed={id===family} onClick={()=>setFamily(id)}><Icon size={16}/>{label} {items.filter(item=>id==='all'||item.namespace===id).length}</Button>)}
   {family!=='builtin'&&<label className="ml-auto flex items-center gap-2 text-sm"><TextInput type="checkbox" checked={showPlanned} onChange={event=>setShowPlanned(event.target.checked)}/> 미지원 채널 포함</label>}
  </div>
  <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">{visible.map(item=><Button key={item.namespace+':'+item.id} onClick={()=>setDetail(item)} className="!flex-col !items-start !justify-start !p-4 text-left">
   <span className="flex w-full items-center justify-between gap-2"><strong className="break-all text-sm">{item.name}</strong><StateTag online={item.available}>{item.namespace==='builtin'?'내장 도구':!item.available?'미지원':item.mode==='outbound'?'송신 전용':'연결 지원'}</StateTag></span>
   <span className="text-sm font-normal text-[#7C7469]">{item.description}</span>
  </Button>)}</div>
  {!visible.length&&<Panel className="p-5 text-sm text-[#7C7469]">{data.loading||catalog.loading?'역량을 불러오는 중…':'해당하는 역량이 없습니다.'}</Panel>}
  {detail&&<DialogBackdrop onDismiss={()=>setDetail(undefined)}><DialogPanel aria-label={detail.name} className="p-5">
   <h2>{detail.name}</h2><p className="mt-2">{detail.description}</p>
   {detail.maxRoomParticipants&&<p className="mt-2">연결 가능한 회의실은 최대 {detail.maxRoomParticipants}명입니다.</p>}
   {detail.namespace==='builtin'&&<p className="mt-2 text-sm">채팅 실행 중 도구 권한과 작업 조건을 충족하면 사용할 수 있습니다.</p>}
   <div className="mt-4 flex flex-wrap justify-end gap-2">
    {detail.docs&&<a className="inline-flex items-center gap-1 p-2 text-sm text-[#A94E30]" href={detail.docs} target="_blank" rel="noreferrer">공식 문서 <ExternalLink size={14}/></a>}
    {detail.namespace==='channel'&&detail.available&&<Button onClick={()=>navigate('connections')}>연결 · 채널 열기 <ArrowRight size={15}/></Button>}
    <Button variant="ghost" onClick={()=>setDetail(undefined)}>닫기</Button>
   </div>
  </DialogPanel></DialogBackdrop>}
 </div>
}
