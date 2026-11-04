import {Button,TextInput,TextArea,SelectInput} from '../components/Primitives'
import {useMemo,useState} from 'react'
import {BookOpenText,BrainCircuit,Check,Plus,Search,Trash2} from 'lucide-react'
import {del,post} from '../api'
import {Action,FormField,Headline,Notice,Panel,Subheading,useRemote} from '../components/UI'
type Memory={id:string;text:string;scope:string;createdAt?:string;updatedAt?:string}
export function MemoryPage(){
 const memories=useRemote<Memory[]>('/api/memories',[]);
 const overview=useRemote<any>('/api/memory/overview',{});
 const agents=useRemote<any[]>('/api/agents',[]);
 const [query,setQuery]=useState(''),[text,setText]=useState(''),[scope,setScope]=useState('shared'),[error,setError]=useState(''),[notice,setNotice]=useState('');
 const filtered=useMemo(()=>memories.value.filter(x=>(x.text+' '+x.scope).toLowerCase().includes(query.toLowerCase())),[memories.value,query]);
 const save=async()=>{if(!text.trim())return;try{await post('/api/memories',{text:text.trim(),scope});setText('');setError('');setNotice('기억이 저장되었습니다.');void memories.reload()}catch(e){setError((e as Error).message)}};
 const drop=async(id:string)=>{if(!confirm('이 기억을 삭제할까요?'))return;try{await del('/api/memories/'+id);void memories.reload()}catch(e){setError((e as Error).message)}};
 return <div className="mx-auto w-full max-w-[1320px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
  <Headline eyebrow="MEMORY LIBRARY" title="기억을 지식으로." description="기억은 로컬 SQLite에 저장되고 에이전트의 다음 실행에 참고됩니다. 자동 학습·자기개선은 별개 기능입니다."/>
  {overview.value?.path&&<div className="mb-3 grid gap-1 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3 text-[11px] text-[#5C544A]"><div className="flex flex-wrap gap-3"><span>저장 위치 <code className="break-all text-[#A94E30]">{overview.value.path}</code></span><span>테이블 <code>{overview.value.table}</code></span></div><div className="flex flex-wrap gap-3"><span>총 {overview.value.total}건</span><span>유형 {Object.entries(overview.value.byType||{}).map(([k,v])=>k+' '+v).join(' · ')||'-'}</span><span>출처 {Object.entries(overview.value.bySource||{}).map(([k,v])=>k+' '+v).join(' · ')||'-'}</span></div></div>}
  {error&&<Notice message={error}/>}
  {notice&&<div className="my-3 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-700"><Check size={16}/>{notice}</div>}
  <div className="grid grid-cols-1 items-start gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)]"><Panel className="min-w-0 p-5"><Subheading title="새 기억 등록" extra={<Action disabled={!text.trim()} onClick={save}><Plus size={16}/> 기억 저장</Action>}/><FormField label="저장할 내용"><TextArea className="min-h-28" rows={6} placeholder="예: 프로젝트 기본 스택은 React + Node.js, UI는 모듈식 컴포넌트로 구성한다." value={text} onChange={e=>setText(e.target.value)}/></FormField><FormField label="적용 범위"><SelectInput  value={scope} onChange={e=>setScope(e.target.value)}><option value="shared">전체 에이전트</option>{agents.value.map(a=><option key={a.id} value={a.id}>{a.name}</option>)}</SelectInput></FormField><p className="rounded-xl border border-[#EED9CC] bg-[#F7E8E1] p-4 text-xs leading-5 text-[#A94E30]"><BrainCircuit size={16}/> 현재 기억 검색은 텍스트 기반이며 의미 검색·시간적 정합성 평가·자동 리플렉션은 아직 연결되지 않았습니다.</p></Panel>
  <div><div className="mb-4 flex flex-wrap items-center justify-between gap-2"><h3>저장된 기억 <span>{memories.value.length}</span></h3><div className="flex min-w-0 flex-1 items-center gap-2 rounded-xl border border-[#E7E0D5] bg-white px-3 py-2"><Search size={16}/><TextInput value={query} onChange={e=>setQuery(e.target.value)} placeholder="기억 검색"/></div></div><div className="flex min-w-0 flex-col gap-3">{filtered.map(m=><Panel key={m.id} className="min-w-0 rounded-xl border border-[#E7E0D5] bg-white p-4 shadow-sm"><div className="mb-3 flex flex-wrap items-center justify-between gap-2"><BookOpenText size={18}/><span>{m.scope==='shared'?'전체 공통':agents.value.find(a=>a.id===m.scope)?.name||m.scope}</span><Button size="icon" variant="ghost" onClick={()=>drop(m.id)} title="삭제"><Trash2 size={16}/></Button></div><p>{m.text}</p><small>{m.createdAt?new Date(m.createdAt).toLocaleString('ko-KR'):''}</small></Panel>)}{!filtered.length&&<Panel><div className="flex min-h-40 flex-col items-center justify-center gap-2 p-5 text-center text-[#7C7469]"><BookOpenText size={24}/><strong>해당 기억이 없습니다.</strong></div></Panel>}</div></div></div>
 </div>
}
