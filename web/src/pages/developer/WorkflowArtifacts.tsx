import {useState} from 'react'
import {FileCheck2,RefreshCw} from 'lucide-react'
import {get} from '../../api'
import {Button,SelectInput} from '../../components/Primitives'
import {Subheading} from '../../components/UI'

type Run={id:string;kind:string;status:string;createdAt?:string;
 input?:{workflowId?:string;definition?:{name?:string}};
 result?:{artifacts?:unknown[];outputs?:{artifacts?:unknown[]}[]};
 workflowState?:{outputs?:{artifacts?:unknown[]}[]}}
type Check={jobId:string;count:number;intact:number;
 files:{path:string;status:string;expectedSha256:string;currentSha256?:string}[]}
export function WorkflowArtifacts({jobs,reload}:{jobs:Run[];reload:()=>Promise<unknown>}){
 const runs=jobs.filter(job=>job.kind==='workflow'&&
  ((job.result?.artifacts?.length||0)>0||
   job.workflowState?.outputs?.some(step=>step.artifacts?.length)))
  .slice(0,20)
 const [selected,setSelected]=useState(''),[check,setCheck]=useState<Check|null>(null)
 const [busy,setBusy]=useState(false),[error,setError]=useState('')
 const active=runs.some(job=>job.id===selected)?selected:runs[0]?.id||''
 const inspect=async()=>{
  if(!active||busy)return
  setBusy(true);setError('')
  try{setCheck(await get<Check>('/api/dev/workflow-artifacts/'+encodeURIComponent(active)))}
  catch(e){setError((e as Error).message)}finally{setBusy(false)}
 }
 return <section className="space-y-3">
  <Subheading title="워크플로 산출물 검증" extra={<FileCheck2 size={18}/>}/>
  <p className="text-[11px] leading-5 text-[#71685D]">실제 파일 도구가 남긴 SHA-256 영수증을 현재 작업공간 파일과 다시 비교합니다. 파일 존재·무결성 검사이며 내용 품질이나 테스트 통과를 의미하지 않습니다.</p>
  {runs.length?<><div className="flex flex-wrap gap-2">
   <SelectInput aria-label="산출물 검증할 워크플로 실행" className="min-w-0 flex-1"
    value={active} onChange={e=>{setSelected(e.target.value);setCheck(null)}}>
    {runs.map(job=><option key={job.id} value={job.id}>
     {job.input?.definition?.name||job.input?.workflowId||'워크플로'} · {job.status} · {job.id.slice(0,8)}
    </option>)}
   </SelectInput>
   <Button variant="secondary" disabled={busy} onClick={()=>void inspect()}>
    <FileCheck2 size={14}/> {busy?'검증 중':'파일 검증'}
   </Button>
   <Button size="icon" variant="ghost" aria-label="워크플로 기록 새로고침" onClick={()=>void reload()}><RefreshCw size={14}/></Button>
  </div>
  {error&&<p role="alert" className="text-[11px] text-red-700">{error}</p>}
  {check&&check.jobId===active&&<div className="rounded-lg border border-[#E9E0D4] bg-[#FAF7F1] p-3 text-[11px]">
   <strong>실제 파일 일치 {check.intact}/{check.count}개</strong>
   <ul className="mt-2 space-y-2">{check.files.map((file,i)=><li key={file.path+'-'+i} className="break-all border-t border-[#E7DDD0] pt-2">
    <span className={file.status==='intact'?'text-emerald-700':'text-amber-800'}>
     {file.status==='intact'?'✓ 일치':file.status==='changed'?'⚠ 변경':'검증 불가: '+file.status}
    </span>
    {' '}<code>{file.path}</code>
    <span className="block text-[#8A8176]">기록 SHA {file.expectedSha256.slice(0,16)}
     {file.currentSha256&&' · 현재 '+file.currentSha256.slice(0,16)}</span>
   </li>)}</ul>
  </div>}
  </>:<p className="text-[11px] text-[#8A8176]">아직 검증 가능한 워크플로 파일 산출물이 없습니다.</p>}
 </section>
}
