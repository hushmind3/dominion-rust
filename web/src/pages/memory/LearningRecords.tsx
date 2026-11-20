import {useState} from 'react'
import {formatDateTime} from '../../i18n'
import {Button} from '../../components/Primitives'
import {Notice,useRemote} from '../../components/UI'

type Experience={
 id:string;lesson?:string;request?:string;status?:string;scope?:string;
 lastSeenAt?:string;occurrences?:number;evidenceMessageIds?:string[]
}
export function LearningRecords(){
 const records=useRemote<Experience[]>('/api/experience',[])
 const [limit,setLimit]=useState(20)
 return <div className="mt-3 grid gap-2">
  <p className="text-xs text-[#7C7469]">실제 실행 기록에서 검증된 복구 사례만 표시합니다.</p>
  <Notice message={records.error}/>
  {records.loading?<p>기록을 불러오는 중…</p>:!records.error&&!records.value.length?
   <p>검증된 실행 기록이 없습니다.</p>:records.value.slice(0,limit).map(row=>
   <details key={row.id} className="rounded-lg border border-[#EFE9DF] p-3">
    <summary className="cursor-pointer text-sm"><strong>{row.lesson||row.id}</strong>{row.status&&<span className="ml-2 text-xs text-[#7C7469]">{row.status}</span>}</summary>
    {row.request&&<p className="mt-2 text-xs text-[#4A443C]">{row.request}</p>}
    <p className="mt-2 text-xs text-[#7C7469]">저장소: {row.scope||'미지정'} · 관측: {row.occurrences??1}회</p>
    {row.lastSeenAt&&<p className="text-xs text-[#8A8176]">{formatDateTime(row.lastSeenAt)}</p>}
    {row.evidenceMessageIds?.length?<p className="mt-1 text-xs text-[#8A8176]">근거: {row.evidenceMessageIds.length}개</p>:null}
   </details>)}
  {records.value.length>limit&&<Button variant="secondary" onClick={()=>setLimit(n=>n+20)}>더 보기</Button>}
 </div>
}
