import {useCallback,useEffect,useMemo,useState} from 'react'
import {BookOpenText,Check,Clock3,Database,FolderPlus,Pencil,Plus,RefreshCw,Save,Search,Trash2,X} from 'lucide-react'
import {get,post,del} from '../api'
import {Button,TextArea,TextInput,SelectInput} from '../components/Primitives'
import {Action,FormField,Headline,Notice,Panel,Subheading} from '../components/UI'

type Note={id:string;scope:string;title:string;content:string;created_at_ms:number;updated_at_ms:number}
const localTime=(value:number)=>value?new Date(value).toLocaleString('ko-KR'):'—'
const countWords=(notes:Note[])=>notes.reduce((total,note)=>total+note.content.length,0)

export function MemoryPage(){
 const [scope,setScope]=useState('global')
 const [scopes,setScopes]=useState<string[]>(['global'])
 const [notes,setNotes]=useState<Note[]>([])
 const [loading,setLoading]=useState(true)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [notice,setNotice]=useState('')
 const [title,setTitle]=useState('')
 const [content,setContent]=useState('')
 const [newScope,setNewScope]=useState('')
 const [showNewScope,setShowNewScope]=useState(false)
 const [query,setQuery]=useState('')
 const [limit,setLimit]=useState(40)
 const [editing,setEditing]=useState<string|null>(null)
 const [editTitle,setEditTitle]=useState('')
 const [editContent,setEditContent]=useState('')

 const refresh=useCallback(async()=>{
  const [entries,available]=await Promise.all([
   get<Note[]>('/api/v1/memory/all?scope='+encodeURIComponent(scope)),
   get<string[]>('/api/v1/memory/scopes')
  ])
  setNotes(entries)
  setScopes([...new Set(['global',scope,...available])].sort((a,b)=>a==='global'?-1:b==='global'?1:a.localeCompare(b)))
 },[scope])
 useEffect(()=>{
  let live=true
  setLoading(true)
  const url='/api/v1/memory/all?scope='+encodeURIComponent(scope)
  const controller=new AbortController()
  void Promise.all([get<Note[]>(url,controller.signal),get<string[]>('/api/v1/memory/scopes',controller.signal)])
   .then(([entries,available])=>{
    if(!live)return
    setNotes(entries)
    setScopes([...new Set(['global',scope,...available])].sort((a,b)=>a==='global'?-1:b==='global'?1:a.localeCompare(b)))
    setError('')
   }).catch(err=>{if(live)setError((err as Error).message)})
   .finally(()=>{if(live)setLoading(false)})
  return()=>{live=false;controller.abort()}
 },[scope])
 useEffect(()=>{setLimit(40);setEditing(null);setNotice('')},[scope,query])
 const filtered=useMemo(()=>notes.filter(note=>(note.title+' '+note.content).toLowerCase().includes(query.trim().toLowerCase())),[notes,query])
 const latest=notes.reduce((date,note)=>Math.max(date,note.updated_at_ms),0)
 const perform=async(action:()=>Promise<void>,message:string)=>{
  if(busy)return
  setBusy(true);setError('');setNotice('')
  try{await action();await refresh();setNotice(message)}
  catch(err){setError((err as Error).message)}
  finally{setBusy(false)}
 }
 const save=()=>void perform(async()=>{
  await post('/api/v1/memory',{scope,title:title.trim(),content:content.trim()})
  setTitle('');setContent('')
 },'기억을 저장했습니다.')
 const beginEdit=(note:Note)=>{setEditing(note.id);setEditTitle(note.title);setEditContent(note.content)}
 const update=(id:string)=>void perform(async()=>{
  await post('/api/v1/memory/'+encodeURIComponent(id),{scope,title:editTitle.trim(),content:editContent.trim()})
  setEditing(null)
 },'수정 내용을 저장했습니다.')
 const remove=(id:string)=>{
  if(!window.confirm('이 기억을 영구 삭제하시겠습니까?'))return
  void perform(async()=>{
   await del('/api/v1/memory/'+encodeURIComponent(id)+'?scope='+encodeURIComponent(scope))
   if(editing===id)setEditing(null)
  },'기억을 삭제했습니다.')
 }
 const addScope=()=>{
  const next=newScope.trim()
  if(!next||next.length>512||next.includes('\n')||next.includes('\0')){
   setError('범위 이름은 1~512자로 입력해 주세요.')
   return
  }
  setScope(next)
  setScopes(old=>[...new Set([...old,next])])
  setNewScope('');setShowNewScope(false)
 }
 return <div className="mx-auto w-full max-w-[1320px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
  <Headline eyebrow="DOMINION · Rust Memory" title="기억 관리" description="모델에 의존하지 않고 저장되는 기억을 프로젝트별로 찾고, 수정하고, 삭제하세요."/>
  <Notice message={error}/>
  {notice&&<div role="status" className="my-3 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-700"><Check size={16}/>{notice}</div>}
  <div className="mb-5 grid gap-3 sm:grid-cols-3">
   <Panel className="flex items-center gap-3 p-4"><Database size={20} className="text-[#C96442]"/><div><div className="text-xl font-bold">{notes.length}</div><div className="text-xs text-[#756D63]">선택한 범위의 기억</div></div></Panel>
   <Panel className="flex items-center gap-3 p-4"><BookOpenText size={20} className="text-[#C96442]"/><div><div className="text-xl font-bold">{countWords(notes).toLocaleString('ko-KR')}</div><div className="text-xs text-[#756D63]">저장된 본문 글자 수</div></div></Panel>
   <Panel className="flex items-center gap-3 p-4"><Clock3 size={20} className="text-[#C96442]"/><div><div className="text-sm font-semibold">{localTime(latest)}</div><div className="text-xs text-[#756D63]">최근 변경</div></div></Panel>
  </div>
  <Panel className="mb-5 p-5">
   <div className="flex flex-wrap items-end justify-between gap-3">
    <div className="min-w-[220px] flex-1">
     <FormField label="기억을 관리할 범위" hint="전역 기억은 모든 프로젝트에서 참고할 수 있습니다. 프로젝트 기억은 해당 저장소에서만 참조됩니다.">
      <SelectInput value={scope} onChange={e=>setScope(e.target.value)} aria-label="기억 범위">
       {scopes.map(name=><option key={name} value={name}>{name==='global'?'전체 공용 기억':name}</option>)}
      </SelectInput>
     </FormField>
    </div>
    <div className="flex gap-2">
     <Button onClick={()=>setShowNewScope(v=>!v)}><FolderPlus size={15}/> 프로젝트 범위 추가</Button>
     <Button disabled={busy} onClick={()=>void perform(async()=>{},'목록을 새로 고쳤습니다.')}><RefreshCw size={15}/> 새로 고침</Button>
    </div>
   </div>
   {showNewScope&&<div className="mt-3 flex flex-wrap gap-2">
    <TextInput className="min-w-[200px] flex-1" aria-label="새 범위 이름" value={newScope} placeholder="저장소 이름 또는 프로젝트 ID" onChange={e=>setNewScope(e.target.value)} onKeyDown={e=>{if(e.key==='Enter')addScope()}}/>
    <Button variant="primary" onClick={addScope} disabled={!newScope.trim()}>선택</Button>
   </div>}
  </Panel>
  <div className="grid grid-cols-1 items-start gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)]">
   <Panel className="p-5">
    <Subheading title="새 기억 저장" extra={<Action onClick={save} disabled={busy||!title.trim()||!content.trim()}><Plus size={15}/> 저장</Action>}/>
    <FormField label="기억 제목" hint="같은 범위에서 제목이 같으면 기존 기억을 갱신합니다.">
     <TextInput value={title} maxLength={120} onChange={e=>setTitle(e.target.value)} placeholder="예: 이 프로젝트의 설계 원칙"/>
    </FormField>
    <div className="mt-3"><FormField label="기억 내용">
     <TextArea rows={7} maxLength={4000} value={content} onChange={e=>setContent(e.target.value)} placeholder="나중에도 적용할 사실이나 지침을 기록하세요."/>
    </FormField></div>
    <div className="mt-3 flex items-center justify-between text-xs text-[#82776D]">
     <span>저장 범위: {scope==='global'?'공용':scope}</span><span>{content.length}/4000</span>
    </div>
   </Panel>
   <div className="min-w-0">
    <Subheading title={'저장된 기억 · '+filtered.length+'개'}/>
    <div className="mb-3 flex items-center gap-2 rounded-xl border border-[#E1D9CC] bg-[#FFFDF9] px-3">
     <Search size={16} className="text-[#938778]"/>
     <TextInput className="border-0 bg-transparent shadow-none focus:ring-0" aria-label="기억 검색" placeholder="제목이나 내용으로 검색" value={query} onChange={e=>setQuery(e.target.value)}/>
    </div>
    <div className="flex flex-col gap-3">
     {filtered.slice(0,limit).map(note=><Panel key={note.id} className="p-4">
      {editing===note.id?
       <div className="space-y-3">
        <FormField label="제목 수정"><TextInput value={editTitle} maxLength={120} onChange={e=>setEditTitle(e.target.value)}/></FormField>
        <FormField label="내용 수정"><TextArea rows={6} maxLength={4000} value={editContent} onChange={e=>setEditContent(e.target.value)}/></FormField>
        <div className="flex justify-end gap-2">
         <Button disabled={busy} onClick={()=>setEditing(null)}><X size={14}/> 취소</Button>
         <Button variant="primary" disabled={busy||!editTitle.trim()||!editContent.trim()} onClick={()=>update(note.id)}><Save size={14}/> 변경 저장</Button>
        </div>
       </div>:
       <>
        <div className="flex items-start gap-2">
         <BookOpenText size={17} className="mt-0.5 shrink-0 text-[#C96442]"/>
         <strong className="min-w-0 flex-1 break-words text-sm">{note.title}</strong>
         <Button size="icon" variant="ghost" disabled={busy} onClick={()=>beginEdit(note)} aria-label="기억 수정"><Pencil size={15}/></Button>
         <Button size="icon" variant="ghost" disabled={busy} onClick={()=>remove(note.id)} aria-label="기억 삭제"><Trash2 size={15}/></Button>
        </div>
        <p className="mt-2 whitespace-pre-wrap break-words text-[13px] text-[#4F463D]">{note.content}</p>
        <div className="mt-3 text-[11px] text-[#8A7E70]">최근 변경: {localTime(note.updated_at_ms)}</div>
        <details className="mt-2 text-[11px] text-[#80766B]">
         <summary className="cursor-pointer">기록 상세</summary>
         <p>최초 저장: {localTime(note.created_at_ms)}</p><p>범위: {note.scope}</p><p>기록 ID: {note.id}</p>
        </details>
       </>}
     </Panel>)}
    </div>
    {!loading&&filtered.length===0&&<Panel className="p-6 text-center text-sm text-[#82776D]">{query?'검색 결과가 없습니다.':'이 범위에 저장된 기억이 없습니다.'}</Panel>}
    {loading&&<Panel className="p-6 text-center text-sm text-[#82776D]">기억을 불러오고 있습니다.</Panel>}
    {filtered.length>limit&&<Button className="mt-3 w-full" onClick={()=>setLimit(n=>n+40)}>더 보기 · 남은 {filtered.length-limit}개</Button>}
   </div>
  </div>
 </div>
}
