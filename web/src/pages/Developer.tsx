import {formatDateTime} from '../i18n'
import {useEffect,useState} from 'react'
import {Check,Code2,Eye,GitCommit,Hammer,Package,Play,RefreshCw,Square,Terminal,TestTube2,UploadCloud} from 'lucide-react'
import {Button,TextInput} from '../components/Primitives'
import {Action,FormField,Headline,Notice,Panel,StateTag,Subheading,useRemote} from '../components/UI'
import {get,post} from '../api'

type DevStatus={
 config:{sourceDir:string;repoUrl:string;branch:string;previewPort:number};
 exists:boolean;nodeModules:boolean;version?:string;name?:string;git:boolean;branch?:string;commit?:string;remote?:string;dirty?:number;
 preview:{status:string;url:string;logs?:string[];error?:string};
 installer?:{name:string;path:string;bytes:number;mtime:string}|null;
 platform:string;arch:string;installedExe:string;
}
type DevJob={id:string;kind:string;status:string;logs:string[];error?:string;result?:any}
type DiffResult={changes:{path:string;status:string;patch:string}[];count:number;truncated:boolean}

const terminal=(status:string)=>['completed','failed'].includes(status)
const fmtBytes=(bytes=0)=>bytes>=1024**3?(bytes/1024**3).toFixed(1)+' GB':bytes>=1024**2?Math.round(bytes/1024**2)+' MB':'0 MB'

export function DeveloperPage(){
 const status=useRemote<DevStatus>('/api/dev/status',{config:{sourceDir:'',repoUrl:'',branch:'main',previewPort:5174},exists:false,nodeModules:false,git:false,preview:{status:'stopped',url:''},platform:'',arch:'',installedExe:''})
 const [sourceDir,setSourceDir]=useState(''),[repoUrl,setRepoUrl]=useState(''),[branch,setBranch]=useState('main')
 const [job,setJob]=useState<DevJob|null>(null),[jobId,setJobId]=useState('')
 const [diff,setDiff]=useState<DiffResult|null>(null),[commitMessage,setCommitMessage]=useState('')
 const [error,setError]=useState(''),[message,setMessage]=useState(''),[busy,setBusy]=useState('')
 useEffect(()=>{setSourceDir(status.value.config.sourceDir||'');setRepoUrl(status.value.config.repoUrl||'');setBranch(status.value.config.branch||'main')},[status.value.config.sourceDir,status.value.config.repoUrl,status.value.config.branch])
 useEffect(()=>{
  if(!jobId)return
  const timer=setInterval(async()=>{
   try{const next=await get<DevJob>('/api/dev/jobs/'+jobId);setJob(next);if(terminal(next.status)){clearInterval(timer);setBusy('');void status.reload();if(next.status==='completed'){setMessage(next.kind==='commit'&&typeof next.result?.oid==='string'?'커밋: '+next.result.oid.slice(0,12):next.kind+' 완료');if(next.kind==='commit')setCommitMessage('');void loadDiff()}else setError(next.error||'작업 실패')}}
   catch(e){clearInterval(timer);setBusy('');setError((e as Error).message)}
  },1200)
  return()=>clearInterval(timer)
 },[jobId])
 const loadDiff=async()=>{try{setDiff(await get<DiffResult>('/api/dev/diff'))}catch(e){setError((e as Error).message)}}
 const run=async(kind:string,body:Record<string,unknown>={})=>{
  if(busy)return;
  setError('');setMessage('');setBusy(kind)
  try{const started=await post<{id:string}>(`/api/dev/${kind}`,body);setJobId(started.id);setJob({id:started.id,kind,status:'running',logs:[]})}
  catch(e){setBusy('');setError((e as Error).message)}
 }
 const connect=async()=>{
  setError('');setMessage('');setBusy('connect')
  try{const result=await post<any>('/api/dev/connect',{sourceDir,repoUrl,branch});setMessage(result.cloned?'저장소를 클론했습니다.':'기존 소스 작업공간에 연결했습니다.');await status.reload();void loadDiff()}
  catch(e){setError((e as Error).message)}finally{setBusy('')}
 }
 const saveConfig=async()=>{
  setError('');setMessage('')
  try{await post('/api/dev/config',{sourceDir,repoUrl,branch});await status.reload();setMessage('소스 작업공간 설정을 저장했습니다.')}
  catch(e){setError((e as Error).message)}
 }
 const pickSource=async()=>{try{const {path}=await post<{path?:string}>('/api/system/pick-folder',{});if(path)setSourceDir(path)}catch(e){setError((e as Error).message)}}
 const preview=async(action:'start'|'stop')=>{
  setError('');setMessage('')
  try{const result=await post<any>(`/api/dev/preview/${action}`,{});await status.reload();if(action==='start'&&result.status==='running')setMessage('개발 미리보기가 시작되었습니다: '+result.url);else setMessage('개발 미리보기를 중지했습니다.')}
  catch(e){setError((e as Error).message)}
 }
 const openPreview=async()=>{const url=status.value.preview.url;if(!url){setError('먼저 개발 미리보기를 시작하세요.');return}try{const bridge=(window as any).dominionDesktop;if(bridge?.openPreview)await bridge.openPreview(url);else window.open(url,'_blank')}catch(e){setError((e as Error).message)}}
 const commit=async()=>{
  const message=commitMessage.trim();
  if(!message){setError('커밋 메시지를 입력하세요.');return}
  await run('commit',{message});
 }
 const s=status.value
 return <div className="mx-auto w-full max-w-[1280px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
  <Headline eyebrow="SELF DEVELOPMENT" title="소스 개발 스튜디오 v0.2.9" description="설치된 앱과 분리된 Git 소스 작업공간에서 UI·엔진을 수정하고, 테스트·빌드·미리보기를 실행하는 개발 스튜디오입니다." actions={<Button variant="secondary" onClick={()=>{void status.reload();void loadDiff()}}><RefreshCw size={16}/> 상태 새로고침</Button>}/>
  {error&&<Notice message={error}/>}
  {message&&<div className="mb-4 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-700"><Check size={16}/>{message}</div>}
  <div className="grid grid-cols-1 items-start gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(0,1.15fr)]">
   <div className="grid min-w-0 gap-4">
    <Panel className="min-w-0 p-5">
     <Subheading title="작업공간" extra={<StateTag online={s.exists&&s.git}>{s.exists?s.git?'Git 연결됨':'소스만 있음':'미연결'}</StateTag>}/>
     <div className="grid gap-3">
      <FormField label="소스 폴더" hint="설치된 앱의 resources 폴더가 아닌 별도 경로를 사용합니다."><div className="flex gap-2"><TextInput value={sourceDir} onChange={e=>setSourceDir(e.target.value)} placeholder="예: ~/DOMINION Source/dominion"/><Button variant="secondary" className="shrink-0 whitespace-nowrap" onClick={()=>void pickSource()}>폴더 선택</Button></div></FormField>
      <FormField label="Git 저장소"><TextInput value={repoUrl} onChange={e=>setRepoUrl(e.target.value)} placeholder="https://github.com/hushmind3/dominion.git"/></FormField>
      <FormField label="브랜치"><TextInput value={branch} onChange={e=>setBranch(e.target.value)} placeholder="main"/></FormField>
      <div className="flex flex-wrap gap-2"><Button variant="secondary" onClick={()=>void saveConfig()}>설정 저장</Button><Button variant="primary" disabled={busy==='connect'||!sourceDir.trim()} onClick={()=>void connect()}>{busy==='connect'?'연결 중':'연결 / 클론'}</Button></div>
     </div>
     <div className="mt-4 grid gap-2 border-t border-[#EFE9DF] pt-4 text-[11px] text-[#5C544A]">
      <div className="flex justify-between gap-3"><span>Git</span><strong className="truncate">{s.git?(s.branch||'?')+' · '+(s.commit||'').slice(0,8):'없음'}</strong></div>
      <div className="flex justify-between gap-3"><span>변경 파일</span><strong>{s.dirty||0}개</strong></div>
      <div className="flex justify-between gap-3"><span>의존성</span><strong>{s.nodeModules?'설치됨':'미설치'}</strong></div>
      <div className="flex justify-between gap-3"><span>설치된 앱</span><strong className="truncate" title={s.installedExe}>{s.installedExe}</strong></div>
     </div>
    </Panel>
    <Panel className="min-w-0 p-5">
     <Subheading title="개발 실행" extra={<Code2 size={18}/>}/>
     <div className="grid gap-2">
      <div className="flex flex-wrap gap-2"><Button variant="secondary" disabled={!s.exists||Boolean(busy)} onClick={()=>void run('setup')}><Terminal size={14}/> 개발 환경 구성</Button><Button variant="secondary" disabled={!s.exists||Boolean(busy)} onClick={()=>void run('test')}><TestTube2 size={14}/> 테스트</Button><Button variant="secondary" disabled={!s.exists||Boolean(busy)} onClick={()=>void run('build')}><Hammer size={14}/> UI 빌드</Button><Button variant="secondary" disabled={!s.exists||Boolean(busy)} onClick={()=>void run('package')}><Package size={14}/> 설치 파일 생성</Button></div>
      <div className="mt-2 flex flex-wrap items-center gap-2 border-t border-[#EFE9DF] pt-3"><Button variant="secondary" disabled={!s.exists||s.preview.status==='running'} onClick={()=>void preview('start')}><Play size={14}/> 미리보기 시작</Button><Button variant="secondary" disabled={s.preview.status!=='running'} onClick={()=>void preview('stop')}><Square size={14}/> 중지</Button><Button variant="primary" disabled={!s.preview.url} onClick={()=>void openPreview()}><Eye size={14}/> 미리보기 열기</Button><span className="text-[10px] text-[#8A8176]">{s.preview.status==='running'?s.preview.url:s.preview.status}</span></div>
     </div>
     {s.installer&&<div className="mt-4 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3"><div className="flex flex-wrap items-center justify-between gap-2"><div className="min-w-0"><strong className="block truncate text-[12px] text-[#3A342D]">{s.installer.name}</strong><small className="text-[10px] text-[#8A8176]">{fmtBytes(s.installer.bytes)} · {formatDateTime(s.installer.mtime)}</small></div></div><p className="mt-2 text-[10px] leading-4 text-[#8A8176]">설치 파일은 소스 작업공간의 release 폴더에서 가져옵니다. 설치된 앱 내부 파일은 직접 수정하지 않습니다.</p></div>}
    </Panel>
    <Panel className="min-w-0 p-5">
     <Subheading title="변경 사항" extra={<div className="flex items-center gap-2"><Button variant="ghost" className="!px-2 !py-1 !text-[10px]" onClick={()=>void loadDiff()}><RefreshCw size={12}/> 새로고침</Button><span className="text-[11px] text-[#8A8176]">{diff?.count||0}개</span></div>}/>
     <div className="mb-3 flex gap-2"><TextInput value={commitMessage} onChange={e=>setCommitMessage(e.target.value)} placeholder="커밋 메시지"/><Button variant="secondary" className="shrink-0 whitespace-nowrap" disabled={!diff?.count} onClick={()=>void commit()}><GitCommit size={14}/> 커밋</Button></div>
     {diff?.changes.length?<div className="grid max-h-[420px] gap-2 overflow-auto">{diff.changes.map(change=><details key={change.path} className="rounded-lg border border-[#EFE9DF] bg-[#FAF7F1] p-2"><summary className="cursor-pointer text-[11px] font-semibold text-[#5C544A]">{change.status} · {change.path}</summary><pre className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-all text-[10px] leading-4 text-[#3A342D]">{change.patch}</pre></details>)}</div>:<p className="text-[11px] text-[#8A8176]">변경된 파일이 없습니다.</p>}
    </Panel>
   </div>
   <div className="grid min-w-0 gap-4">
    <Panel className="min-w-0 p-5">
     <Subheading title="작업 로그" extra={<Terminal size={18}/>}/>
     {job?<div><div className="mb-2 flex items-center gap-2"><StateTag online={job.status==='completed'}>{job.status}</StateTag><span className="text-[11px] text-[#8A8176]">{job.kind}</span></div><pre className="max-h-[520px] min-h-40 overflow-auto whitespace-pre-wrap break-all rounded-xl bg-[#24211D] p-3 text-[10px] leading-5 text-[#E7E0D5]">{job.logs.join('\n')||'로그 대기 중...'}</pre></div>:<p className="text-[11px] text-[#8A8176]">개발 작업을 실행하면 로그가 표시됩니다.</p>}
     {s.preview.logs?.length?<pre className="mt-3 max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-xl bg-[#FAF7F1] p-3 text-[10px] leading-4 text-[#5C544A]">{s.preview.logs.slice(-40).join('\n')}</pre>:null}
    </Panel>
    <Panel className="min-w-0 p-5">
     <Subheading title="작업 원칙"/>
     <ul className="grid gap-2 text-[11px] leading-5 text-[#5C544A]">
      <li>· 설치된 앱의 <code>resources/app</code>은 직접 수정하지 않습니다.</li>
      <li>· 모든 변경은 별도 Git 소스 작업공간에서 수행합니다.</li>
      <li>· 업데이트는 빌드한 설치 파일을 열어 적용합니다.</li>
      <li>· 모델·기억·설정·API 키·스킬·작업공간은 업데이트와 분리해 보존합니다.</li>
     </ul>
    </Panel>
   </div>
  </div>
 </div>
}
