import {useEffect,useState} from 'react'
import {RefreshCw,Download} from 'lucide-react'
import {Button} from './Primitives'
import {Panel,Subheading,useRemote} from './UI'
import {get,post} from '../api'

type VersionInfo={supported:boolean;available:boolean;local?:string;remote?:string;reason?:string}
type Job={id:string;kind:string;status:string;error?:string;logs?:string[]}

export function AppUpdatePanel(){
 const version=useRemote<VersionInfo>('/api/dev/update',{supported:false,available:false})
 const [jobId,setJobId]=useState('')
 const [busy,setBusy]=useState(false)
 const [message,setMessage]=useState('')
 const [error,setError]=useState('')
 useEffect(()=>{
  if(!jobId)return
  let attempts=0,closed=false
  const handle=setInterval(()=>{
   void get<Job>('/api/dev/jobs/'+jobId).then(job=>{
    if(closed)return
    attempts=0
    if(job.status==='completed'){
     clearInterval(handle)
     setBusy(false)
     setMessage('업데이트가 빌드되었습니다. 엔진이 자동으로 재시작됩니다.')
     void version.reload()
    }else if(job.status==='failed'){
     clearInterval(handle)
     setBusy(false)
     setError(job.error||'업데이트를 완료하지 못했습니다.')
    }
   }).catch(()=>{
    if(closed)return
    if(++attempts>90){
     clearInterval(handle);setBusy(false)
     setError('업데이트 상태를 확인하지 못했습니다. 서버 연결을 다시 확인하세요.')
    }else setMessage('업데이트 또는 엔진 재시작 진행 중입니다.')
   })
  },1500)
  return()=>{closed=true;clearInterval(handle)}
 },[jobId])
 const apply=async()=>{
  if(busy||!version.value.supported)return
  if(!window.confirm('DOMINION을 업데이트하고 엔진을 재시작합니다. 실행 중인 작업을 마친 뒤 진행하시겠습니까?'))return
  setBusy(true);setError('');setMessage('새 버전을 준비하는 중입니다.')
  try{
   const result=await post<{id:string}>('/api/dev/update',{})
   setJobId(result.id)
  }catch(e){setBusy(false);setMessage('');setError((e as Error).message)}
 }
 return <Panel className="min-w-0 p-5">
  <Subheading title="업데이트" extra={<Button type="button" size="sm" variant="ghost" disabled={busy} onClick={()=>void version.reload()}><RefreshCw size={14}/> 확인</Button>}/>
  <p className="text-[12px] leading-5 text-[#6F675D]">가능한 설치 환경에서는 최신 버전 확인부터 설치와 재시작까지 앱 안에서 처리합니다.</p>
  <div className="mt-3 grid gap-1.5 text-[11px] text-[#8A8176]">
   {version.value.local&&<span>현재 버전 · {version.value.local.slice(0,12)}</span>}
   {version.value.remote&&<span>최신 버전 · {version.value.remote.slice(0,12)}</span>}
   {version.value.reason&&<span className="text-[#A05B48]">{version.value.reason}</span>}
   {version.value.available&&<strong className="text-emerald-700">설치할 새 버전이 있습니다.</strong>}
   {!version.value.available&&version.value.supported&&!version.value.reason&&<span>현재 최신 버전입니다.</span>}
   {message&&<span role="status" className="text-[#5C544A]">{message}</span>}
   {error&&<span role="alert" className="text-rose-700">{error}</span>}
  </div>
  <Button type="button" variant="primary" className="mt-3" disabled={!version.value.supported||busy} onClick={()=>void apply()}><Download size={15}/> {busy?'업데이트 처리 중':version.value.available?'업데이트 및 재시작':'재빌드 및 재시작'}</Button>
 </Panel>
}
