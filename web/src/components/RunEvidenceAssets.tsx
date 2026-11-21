import {useEffect,useRef,useState} from 'react'
import {BookOpenText,ChevronDown,ChevronUp,Clipboard,FileCode2,FolderOpen,RefreshCw} from 'lucide-react'
import {get} from '../api'
import {Button,TextInput} from './Primitives'
import {Notice,Panel,useRemote} from './UI'
import {useLocale} from '../i18n'
type Skill={name:string;path:string}
type FileList={files:unknown;offset?:number;limit?:number;hasMore?:boolean}
type FileEntry={path:string;label:string}
const fileRows=(value:unknown):FileEntry[]=>{
 const raw=(value&&typeof value==='object'&&!Array.isArray(value)&&'files' in value)?
  (value as {files:unknown}).files:value
 const list=Array.isArray(raw)?raw:[]
 return list.flatMap((entry):FileEntry[]=>{
  if(typeof entry==='string')return [{path:entry,label:entry}]
  if(!entry||typeof entry!=='object')return []
  const item=entry as Record<string,unknown>
  const path=typeof item.path==='string'?item.path:typeof item.name==='string'?item.name:''
  return path?[{path,label:path}]:[]
 }).filter(item=>item.path&&!item.path.endsWith('/')).slice(0,200)
}
export function RunEvidenceAssets({runId}:{runId:string}){
 const ko=useLocale()==='ko'
 const [open,setOpen]=useState(false)
 const [fileOffset,setFileOffset]=useState(0)
 const [fileSearch,setFileSearch]=useState('')
 const [fileFilter,setFileFilter]=useState('')
 // Keep previews, page offsets and in-flight read replies tied to the
 // specific Run they came from; switching Runs must never show old files.
 const selectedRun=useRef(runId)
 selectedRun.current=runId
 useEffect(()=>{
  setFileOffset(0)
  setFileSearch('');setFileFilter('')
  setPreview(null)
  setFilePages([])
  setError('')
  setLoading(false)
 },[runId])
 useEffect(()=>{
  const timer=window.setTimeout(()=>{
   setFileFilter(fileSearch.trim())
   setFileOffset(0)
   setPreview(null)
  },300)
  return ()=>window.clearTimeout(timer)
 },[fileSearch,runId])
 const skills=useRemote<Skill[]>(open?'/api/v1/runs/'+encodeURIComponent(runId)+'/skills':null,[])
 const files=useRemote<FileList>(open?'/api/v1/runs/'+encodeURIComponent(runId)+'/files?offset='+fileOffset+'&limit=100'
  +(fileFilter?'&name_contains='+encodeURIComponent(fileFilter):''):null,{files:[],hasMore:false})
 const [preview,setPreview]=useState<{runId:string;kind:'skill'|'file';name:string;
  content:string;startLine?:number;endLine?:number;totalLines?:number;
  nextStartLine?:number|null}|null>(null)
 const [filePages,setFilePages]=useState<number[]>([])
 const [loading,setLoading]=useState(false),[error,setError]=useState('')
 const inspect=async(kind:'skill'|'file',name:string,startLine=1,history:number[]=[])=>{
  if(loading)return
  const sourceRun=runId
  setLoading(true);setError('');setPreview(null)
  try{
   const base='/api/v1/runs/'+encodeURIComponent(sourceRun)+'/'+(kind==='skill'?'skills/':'files/')
   const safePath=kind==='file'?name.split('/').map(encodeURIComponent).join('/'):encodeURIComponent(name)
   const endpoint=base+safePath+(kind==='file'?'?start_line='+startLine:'')
   const entry=await get<{content:string;startLine?:number;endLine?:number;
    totalLines?:number;nextStartLine?:number|null}>(endpoint)
   if(selectedRun.current===sourceRun){
    setPreview({runId:sourceRun,kind,name,...entry})
    setFilePages(history)
   }
  }catch(e){
   if(selectedRun.current===sourceRun)setError((e as Error).message)
  }finally{
   if(selectedRun.current===sourceRun)setLoading(false)
  }
 }
 const rows=fileRows(files.value)
 return <Panel className="mt-4 overflow-hidden">
  <button type="button" aria-expanded={open} onClick={()=>setOpen(v=>!v)}
   className="flex w-full items-center justify-between gap-3 p-3 text-left text-xs font-semibold">
   <span className="flex items-center gap-2"><FolderOpen size={15}/>{ko?'사용 가능한 Skills · 생성 파일':'Skills & generated files'}</span>
   {open?<ChevronUp size={15}/>:<ChevronDown size={15}/>}
  </button>
  {open&&<div className="border-t border-[#E7E0D5] p-3">
   <div className="mb-3 flex justify-end"><Button size="sm" variant="ghost" disabled={loading} onClick={()=>{void skills.reload();void files.reload()}}><RefreshCw size={12}/>{ko?'새로고침':'Refresh'}</Button></div>
   <div className="grid gap-3 sm:grid-cols-2">
    <section><h4 className="mb-2 flex items-center gap-1 text-xs font-semibold"><BookOpenText size={13}/> Skills ({skills.value.length})</h4>
     <div className="max-h-40 space-y-1 overflow-auto">{skills.value.map(s=><Button size="sm" variant="ghost" key={s.path} disabled={loading} onClick={()=>void inspect('skill',s.name)} className="!flex !w-full !justify-start !text-left">{s.name}</Button>)}
      {!skills.value.length&&<p className="text-[11px] text-[#9C9388]">{ko?'이 실행에서 사용 가능한 Skill이 없습니다.':'No available skills for this Run.'}</p>}
     </div></section>
    <section><h4 className="mb-2 flex items-center gap-1 text-xs font-semibold"><FileCode2 size={13}/>{ko?'작업 파일':'Workspace files'} ({rows.length})</h4>
     <TextInput aria-label={ko?'작업 파일 검색':'Search workspace files'} value={fileSearch}
      onChange={event=>setFileSearch(event.target.value)} placeholder={ko?'파일명·경로로 전체 검색':'Search all filenames and paths'} className="mb-2 w-full !text-xs"/>
     <div className="max-h-40 space-y-1 overflow-auto">{rows.map(row=><Button size="sm" variant="ghost" key={row.path} disabled={loading} onClick={()=>void inspect('file',row.path)} className="!flex !w-full !justify-start !text-left break-all">{row.label}</Button>)}
      {!rows.length&&<p className="text-[11px] text-[#9C9388]">{fileFilter?(ko?'일치하는 파일이 없습니다.':'No matching workspace files.'):(ko?'조회 가능한 파일이 없습니다.':'No readable workspace files.')}</p>}</div>
      {(fileOffset>0||files.value.hasMore)&&<div className="mt-2 flex flex-wrap items-center justify-between gap-2">
       <Button size="sm" variant="secondary" disabled={files.loading||fileOffset===0} onClick={()=>{setFileOffset(n=>Math.max(0,n-100));setPreview(null)}}>{ko?'이전 파일':'Previous'}</Button>
       <span className="text-[10px] text-[#8A8176]">{rows.length?fileOffset+1:0}–{fileOffset+rows.length}</span>
       <Button size="sm" variant="secondary" disabled={files.loading||!files.value.hasMore} onClick={()=>{setFileOffset(n=>n+100);setPreview(null)}}>{ko?'다음 파일':'Next'}</Button>
      </div>}</section>
   </div>
   {preview?.runId===runId&&<div className="mt-3 rounded-lg border border-[#E7E0D5] bg-[#FAF7F1] p-3">
    <div className="flex flex-wrap items-center justify-between gap-2"><strong className="break-all text-xs">{preview.name}</strong><Button size="sm" variant="ghost" onClick={()=>void navigator.clipboard.writeText(preview.content)}><Clipboard size={12}/>{ko?'표시된 부분 복사':'Copy excerpt'}</Button></div>
    {preview.kind==='file'&&typeof preview.startLine==='number'&&<div className="mt-2 flex flex-wrap items-center justify-between gap-2 text-[11px] text-[#7C7469]">
     <span>{ko?'줄':'Lines'} {preview.totalLines===0?0:preview.startLine}–{preview.endLine} / {preview.totalLines}</span>
     <div className="flex gap-1">
      {filePages.length>0&&<Button size="sm" variant="secondary" disabled={loading} onClick={()=>void inspect('file',preview.name,filePages[filePages.length-1],filePages.slice(0,-1))}>{ko?'이전 줄':'Previous lines'}</Button>}
      {preview.nextStartLine!=null&&<Button size="sm" variant="secondary" disabled={loading} onClick={()=>void inspect('file',preview.name,preview.nextStartLine!,[...filePages,preview.startLine!])}>{ko?'다음 줄':'Next lines'}</Button>}
     </div>
    </div>}
    <pre className="mt-2 max-h-72 overflow-auto whitespace-pre-wrap break-all text-[11px]">{preview.content}</pre>
   </div>}
   <Notice message={error||skills.error||files.error}/>
  </div>}
 </Panel>
}
