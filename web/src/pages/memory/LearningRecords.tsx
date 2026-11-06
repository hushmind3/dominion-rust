import {useState} from 'react'
import {Button} from '../../components/Primitives'
import {Notice,useRemote} from '../../components/UI'

const sources=[
 ['경험','/api/experience'],
 ['개선안','/api/improvements'],
 ['알고리즘','/api/algorithms'],
 ['실험 요청','/api/experiment-requests'],
 ['개선 방식 평가','/api/optimizers'],
] as const
type RecordRow={id?:string;title?:string;name?:string;label?:string;kind?:string;status?:string;revision?:string;verdict?:string}
export function LearningRecords(){
 const [source,setSource]=useState(0)
 const data=useRemote<RecordRow[]|{kinds?:RecordRow[];items?:RecordRow[]}|null>(sources[source][1],null)
 const rows:RecordRow[]=Array.isArray(data.value)?data.value:data.value?.kinds??data.value?.items??(data.value?[{...data.value,label:sources[source][0]}]:[])
 const [limit,setLimit]=useState(20)
 return <div className="mt-3 grid gap-2">
  <div className="flex flex-wrap gap-2">{sources.map(([label],i)=><Button key={label} variant={source===i?'primary':'ghost'} onClick={()=>{setSource(i);setLimit(20)}}>{label}</Button>)}</div>
  <Notice message={data.error}/>
  {data.loading?<p>기록을 불러오는 중…</p>:!data.error&&!rows.length?<p>저장된 기록이 없습니다.</p>:rows.slice(0,limit).map((row,i)=><details key={row.id??i} className="rounded-lg border border-[#EFE9DF] p-2">
   <summary className="cursor-pointer text-sm">{row.title??row.name??row.label??row.kind??row.id??sources[source][0]}{row.status?' · '+row.status:''}{row.verdict?' · '+row.verdict:''}{row.revision?' · '+row.revision:''}</summary>
   <pre className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-all text-xs">{JSON.stringify(row,null,2)}</pre>
  </details>)}
  {rows.length>limit&&<Button variant="secondary" onClick={()=>setLimit(n=>n+20)}>더 보기</Button>}
 </div>
}
