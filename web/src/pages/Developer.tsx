import {t,useLocale,formatDateTime} from '../i18n'
import {useEffect,useState} from 'react'
import {Check,Code2,Eye,GitCommit,Hammer,Package,Play,RefreshCw,Square,Terminal,TestTube2,UploadCloud} from 'lucide-react'
import {Button,TextInput,SelectInput} from '../components/Primitives'
import {Action,FormField,Headline,Notice,Panel,StateTag,Subheading,useRemote} from '../components/UI'
import {Provider,get,post} from '../api'

type DevStatus={
 config:{sourceDir:string;repoUrl:string;branch:string;previewPort:number};
 exists:boolean;nodeModules:boolean;version?:string;name?:string;git:boolean;branch?:string;commit?:string;remote?:string;dirty?:number;
 preview:{status:string;url:string;logs?:string[];error?:string};
 installer?:{name:string;path:string;bytes:number;mtime:string}|null;
 platform:string;arch:string;installedExe:string;
}
type DevJob={id:string;kind:string;status:string;logs:string[];error?:string;result?:any}
type DiffResult={changes:{path:string;status:string;patch:string}[];count:number;truncated:boolean}
type UpdateStatus={supported:boolean;available:boolean;local?:string;remote?:string;running?:string;reason?:string;installMode?:string}

const terminal=(status:string)=>['completed','failed'].includes(status)
const fmtBytes=(bytes=0)=>bytes>=1024**3?(bytes/1024**3).toFixed(1)+' GB':bytes>=1024**2?Math.round(bytes/1024**2)+' MB':'0 MB'

export function DeveloperPage(){
 const loc=useLocale()
 const status=useRemote<DevStatus>('/api/dev/status',{config:{sourceDir:'',repoUrl:'',branch:'main',previewPort:5174},exists:false,nodeModules:false,git:false,preview:{status:'stopped',url:''},platform:'',arch:'',installedExe:''})
 const updates=useRemote<UpdateStatus>('/api/dev/update',{supported:false,available:false})
 const flows=useRemote<{id:string;name:string;steps:{agentId:string}[]}[]>('/api/workflows',[])
 const sdkProviders=useRemote<Provider[]>('/api/providers',[])
 const [sourceDir,setSourceDir]=useState(''),[repoUrl,setRepoUrl]=useState(''),[branch,setBranch]=useState('main')
 const [job,setJob]=useState<DevJob|null>(null),[jobId,setJobId]=useState('')
 const [diff,setDiff]=useState<DiffResult|null>(null),[commitMessage,setCommitMessage]=useState('')
 const [error,setError]=useState(''),[message,setMessage]=useState(''),[busy,setBusy]=useState('')
 const [reviewFlow,setReviewFlow]=useState(''),[reviewBusy,setReviewBusy]=useState(false)
 const [reviewRequestId,setReviewRequestId]=useState('')
 const [sdkPrompt,setSdkPrompt]=useState(''),[sdkProvider,setSdkProvider]=useState('')
 const [sdkModel,setSdkModel]=useState(''),[sdkCheckPath,setSdkCheckPath]=useState('')
 const [sdkApproved,setSdkApproved]=useState(false),[sdkBusy,setSdkBusy]=useState(false)
 const [sdkJobId,setSdkJobId]=useState(''),[sdkJob,setSdkJob]=useState<any|null>(null)
 const [sdkRequestId,setSdkRequestId]=useState('')
 useEffect(()=>{
  if(!sdkJobId)return;
  let active=true;const timer=setInterval(async()=>{
   try{
    const next=await get<any>('/api/jobs/'+encodeURIComponent(sdkJobId))
    if(!active)return
    setSdkJob(next)
    if(['completed','failed','needs_review','cancelled'].includes(next.status))
      clearInterval(timer)
   }catch(e){if(active)setError((e as Error).message)}
  },2300)
  return()=>{active=false;clearInterval(timer)}
 },[sdkJobId])
 useEffect(()=>{setSourceDir(status.value.config.sourceDir||'');setRepoUrl(status.value.config.repoUrl||'');setBranch(status.value.config.branch||'main')},[status.value.config.sourceDir,status.value.config.repoUrl,status.value.config.branch])
 useEffect(()=>{
  if(!jobId)return
  let failedPolls=0
  const timer=setInterval(async()=>{
   try{const next=await get<DevJob>('/api/dev/jobs/'+jobId);setJob(next);if(terminal(next.status)){clearInterval(timer);setBusy('');void status.reload();if(next.kind==='update')void updates.reload();if(next.status==='completed'){setMessage(next.kind==='commit'&&typeof next.result?.oid==='string'?t('dev.commitDone',{oid:next.result.oid.slice(0,12)}):t('dev.jobDone',{kind:next.kind}));if(next.kind==='commit')setCommitMessage('');void loadDiff()}else setError(next.error||t('dev.jobFailed'))}}
   catch(e){
    // Replacing the running binary briefly drops the HTTP connection.
    // Keep the job visible and resume polling after the LaunchAgent restarts.
    if(job?.kind==='update'&&++failedPolls<90){setMessage('엔진 재시작 중입니다. 연결이 돌아오면 업데이트 결과를 확인합니다.');return}
    clearInterval(timer);setBusy('');setError((e as Error).message)
   }
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
  try{const result=await post<any>('/api/dev/connect',{sourceDir,repoUrl,branch});setMessage(result.cloned?t('dev.cloneDone'):t('dev.connected'));await status.reload();void loadDiff()}
  catch(e){setError((e as Error).message)}finally{setBusy('')}
 }
 const saveConfig=async()=>{
  setError('');setMessage('')
  try{await post('/api/dev/config',{sourceDir,repoUrl,branch});await status.reload();setMessage(t('dev.configSaved'))}
  catch(e){setError((e as Error).message)}
 }
 const pickSource=async()=>{try{const {path}=await post<{path?:string}>('/api/system/pick-folder',{});if(path)setSourceDir(path)}catch(e){setError((e as Error).message)}}
 const preview=async(action:'start'|'stop')=>{
  setError('');setMessage('')
  try{const result=await post<any>(`/api/dev/preview/${action}`,{});await status.reload();if(action==='start'&&result.status==='running')setMessage(t('dev.previewStarted',{url:result.url}));else setMessage(t('dev.previewStopped'))}
  catch(e){setError((e as Error).message)}
 }
 const openPreview=async()=>{const url=status.value.preview.url;if(!url){setError(t('dev.startPreviewFirst'));return}try{const bridge=(window as any).dominionDesktop;if(bridge?.openPreview)await bridge.openPreview(url);else window.open(url,'_blank')}catch(e){setError((e as Error).message)}}
 const reviewDiff=async()=>{
  if(!reviewFlow||!diff?.changes.length||reviewBusy)return
  const requestId=reviewRequestId||crypto.randomUUID()
  setReviewRequestId(requestId);setReviewBusy(true);setError('');setMessage('')
  try{
   const patch=diff.changes.map(change=>
    'FILE '+change.path+' ['+change.status+']\n'+change.patch).join('\n\n')
   const text=('아래는 개발자 도구에서 실제 추출한 Git 변경 내용입니다. 이 변경만을 검토하고 결함과 근거를 구분한 검토 보고서를 납품하세요. 소스 수정은 사용자가 명시적으로 요청하지 않는 한 하지 마세요.\n'+
    '저장소: '+(status.value.config.repoUrl||'local')+'\n브랜치: '+(status.value.branch||status.value.config.branch)+'\n\n'+patch)
    .slice(0,15500)
   const job=await post<{id:string}>('/api/workflows/'+encodeURIComponent(reviewFlow)+'/run',
    {input:text,requestId})
   setReviewRequestId('')
   setMessage('Git 실제 변경을 에이전트 검토 워크플로로 등록했습니다. Job '+job.id.slice(0,12)+'. 결과는 워크플로 메뉴에서 확인하세요.')
  }catch(e){setError((e as Error).message+' · 동일 요청 ID로 재시도하면 중복 실행을 피합니다.')}
  finally{setReviewBusy(false)}
 }
 const runLocalSdk=async()=>{
  if(!sdkProvider||!sdkPrompt.trim()||!sdkApproved||sdkBusy)return
  setSdkBusy(true);setError('');setMessage('')
  const requestId=sdkRequestId||crypto.randomUUID()
  setSdkRequestId(requestId)
  try{
   const request={
    requestId,kind:'sdk_local',
    input:{
     providerId:sdkProvider,model:sdkModel.trim()||undefined,
     prompt:sdkPrompt.trim(),maxSeconds:420,maxEdits:12,
     approvedLocalWrite:true,
     successCriteria:sdkCheckPath.trim()?[{type:'file_changed',path:sdkCheckPath.trim()}]:[]
    }
   }
   const job=await post<{id:string}>('/api/jobs',request)
   setSdkRequestId('');setSdkJobId(job.id)
   setSdkJob({id:job.id,kind:'sdk_local',status:'queued'})
   setMessage('공식 Qwen-Agent SDK 로컬 코딩 작업을 등록했습니다. 기존 모델과 도구 엔진 대신 실제 SDK 실행기를 사용합니다.')
  }catch(e){setError((e as Error).message+' · 요청 ID를 유지하므로 다시 누를 때 중복 등록을 막습니다.')}
  finally{setSdkBusy(false)}
 }
 const commit=async()=>{
  const message=commitMessage.trim();
  if(!message){setError(t('dev.commitMessageRequired'));return}
  await run('commit',{message});
 }
 const requestUpdate=()=>{
  if(busy||!updates.value.supported)return
  if(!window.confirm('DOMINION을 업데이트하고 재시작합니다. 실행 중인 작업이 있다면 완료한 뒤 진행하세요.'))return
  void run('update')
 }
 const s=status.value
 return <div className="mx-auto w-full max-w-[1280px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
  <Headline eyebrow={t('ui.eyebrow.developer')} title={t('dev.title')} description={t('dev.description')} actions={<Button variant="secondary" onClick={()=>{void status.reload();void loadDiff()}}><RefreshCw size={16}/> {t('dev.statusRefresh')}</Button>}/>
  {error&&<Notice message={error}/>}
  {message&&<div className="mb-4 flex items-center gap-2 rounded-xl border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-700"><Check size={16}/>{message}</div>}
  <div className="grid grid-cols-1 items-start gap-5 xl:grid-cols-[minmax(0,1fr)_minmax(0,1.15fr)]">
   <div className="grid min-w-0 gap-4">
    <Panel className="min-w-0 p-5">
     <Subheading title={t('dev.workspace')} extra={<StateTag online={s.exists&&s.git}>{s.exists?s.git?t('dev.gitConnected'):t('dev.sourceOnly'):t('dev.notConnected')}</StateTag>}/>
     <div className="grid gap-3">
      <FormField label={t('dev.sourceFolder')} hint={t('dev.sourceFolderHint')}><div className="flex gap-2"><TextInput value={sourceDir} onChange={e=>setSourceDir(e.target.value)} placeholder={t('dev.sourceFolderPlaceholder')}/><Button variant="secondary" className="shrink-0 whitespace-nowrap" onClick={()=>void pickSource()}>{t('dev.pickFolder')}</Button></div></FormField>
      <FormField label={t('dev.gitRepo')}><TextInput value={repoUrl} onChange={e=>setRepoUrl(e.target.value)} placeholder="https://github.com/hushmind3/dominion.git"/></FormField>
      <FormField label={t('dev.branch')}><TextInput value={branch} onChange={e=>setBranch(e.target.value)} placeholder="main"/></FormField>
      <div className="flex flex-wrap gap-2"><Button variant="secondary" onClick={()=>void saveConfig()}>{t('dev.saveConfig')}</Button><Button variant="primary" disabled={busy==='connect'||!sourceDir.trim()} onClick={()=>void connect()}>{busy==='connect'?t('dev.connecting'):t('dev.connectClone')}</Button></div>
     </div>
     <div className="mt-4 grid gap-2 border-t border-[#EFE9DF] pt-4 text-[11px] text-[#5C544A]">
      <div className="flex justify-between gap-3"><span>Git</span><strong className="truncate">{s.git?(s.branch||'?')+' · '+(s.commit||'').slice(0,8):t('dev.none')}</strong></div>
      <div className="flex justify-between gap-3"><span>{t('dev.changedFiles')}</span><strong>{t('dev.count',{count:s.dirty||0})}</strong></div>
      <div className="flex justify-between gap-3"><span>{t('dev.dependencies')}</span><strong>{s.nodeModules?t('dev.installed'):t('dev.notInstalled')}</strong></div>
      <div className="flex justify-between gap-3"><span>{t('dev.installedApp')}</span><strong className="truncate" title={s.installedExe}>{s.installedExe}</strong></div>
     </div>
    </Panel>
    <Panel className="min-w-0 p-5">
     <Subheading title={t('dev.devRun')} extra={<Code2 size={18}/>}/>
     <div className="grid gap-2">
      <div className="flex flex-wrap gap-2"><Button variant="secondary" disabled={!s.exists||Boolean(busy)} onClick={()=>void run('setup')}><Terminal size={14}/> {t('dev.setup')}</Button><Button variant="secondary" disabled={!s.exists||Boolean(busy)} onClick={()=>void run('test')}><TestTube2 size={14}/> {t('dev.test')}</Button><Button variant="secondary" disabled={!s.exists||Boolean(busy)} onClick={()=>void run('build')}><Hammer size={14}/> {t('dev.build')}</Button><Button variant="secondary" disabled={!s.exists||Boolean(busy)} onClick={()=>void run('package')}><Package size={14}/> {t('dev.package')}</Button></div>
      <div className="mt-2 flex flex-wrap items-center gap-2 border-t border-[#EFE9DF] pt-3"><Button variant="secondary" disabled={!s.exists||s.preview.status==='running'} onClick={()=>void preview('start')}><Play size={14}/> {t('dev.previewStart')}</Button><Button variant="secondary" disabled={s.preview.status!=='running'} onClick={()=>void preview('stop')}><Square size={14}/> {t('dev.stop')}</Button><Button variant="primary" disabled={!s.preview.url} onClick={()=>void openPreview()}><Eye size={14}/> {t('dev.previewOpen')}</Button><span className="text-[10px] text-[#8A8176]">{s.preview.status==='running'?s.preview.url:s.preview.status}</span></div>
     </div>
     {s.installer&&<div className="mt-4 rounded-xl border border-[#E7E0D5] bg-[#FAF7F1] p-3"><div className="flex flex-wrap items-center justify-between gap-2"><div className="min-w-0"><strong className="block truncate text-[12px] text-[#3A342D]">{s.installer.name}</strong><small className="text-[10px] text-[#8A8176]">{fmtBytes(s.installer.bytes)} · {formatDateTime(s.installer.mtime)}</small></div></div><p className="mt-2 text-[10px] leading-4 text-[#8A8176]">{t('dev.installerNote')}</p></div>}
    </Panel>
    <Panel className="min-w-0 p-5">
     <Subheading title="오픈소스 Qwen-Agent SDK · 로컬 코딩" extra={<Code2 size={18}/>}/>
     <p className="mb-3 text-[11px] leading-5 text-[#7C7469]">
      Qwen 공식 Apache-2.0 에이전트 SDK의 도구 호출 파서를 사용합니다. 설치: pip install 'qwen-agent&gt;=0.0.26,&lt;0.1'.
      실행 모델은 127.0.0.1 로컬 제공자만 가능하며, 원격 유료 API는 연결하지 않습니다.
      승인된 DOMINION 작업공간의 읽기·쓰기·정확한 부분 교체만 허용됩니다. 셸 실행은 제공하지 않습니다.
     </p>
     <div className="grid gap-2">
      <SelectInput value={sdkProvider} onChange={e=>{setSdkProvider(e.target.value);setSdkModel('')}}>
       <option value="">로컬 Qwen / Ollama 모델 제공자 선택</option>
       {sdkProviders.value.filter(p=>p.enabled!==false&&/^http:\/\/(127\.0\.0\.1|\[::1\])(?::\d+)?\//.test(p.baseUrl||''))
        .map(p=><option value={p.id} key={p.id}>{p.name} · {p.model||'모델 입력 필요'}</option>)}
      </SelectInput>
      <TextInput value={sdkModel} onChange={e=>setSdkModel(e.target.value)}
       placeholder="모델 이름 (비워두면 제공자 설정 사용)"/>
      <FormField label="SDK에 실제로 맡길 작업"><TextInput value={sdkPrompt} onChange={e=>{setSdkPrompt(e.target.value);setSdkRequestId('')}}
       placeholder="예: src/example.rs의 계산 오류를 수정하고 검증용 파일을 작성"/></FormField>
      <FormField label="성공 검증 파일 (선택)" hint="기존 파일의 내용이 실제로 바뀌어야 완료 판정을 합니다. 비워두면 파일을 고쳐도 검토 필요로 남깁니다.">
       <TextInput value={sdkCheckPath} onChange={e=>setSdkCheckPath(e.target.value)} placeholder="src/example.rs"/>
      </FormField>
      <label className="flex gap-2 text-xs text-[#5E5144]">
       <input type="checkbox" checked={sdkApproved} onChange={e=>setSdkApproved(e.target.checked)}/>
       로컬 에이전트가 승인된 워크스페이스 파일을 실제로 수정하는 데 동의합니다.
      </label>
      <div className="flex flex-wrap items-center gap-2">
       <Button variant="secondary" disabled={!sdkApproved||!sdkProvider||!sdkPrompt.trim()||sdkBusy}
        onClick={()=>void runLocalSdk()}>{sdkBusy?'SDK 등록 중':'공식 Qwen-Agent SDK로 실제 파일 작업'}</Button>
       {sdkJobId&&<span className="text-[11px] text-[#7C7469]">Job {sdkJobId.slice(0,12)} · {sdkJob?.status||'조회 중'}</span>}
      </div>
      {sdkJob&&<div className="rounded-xl bg-[#FAF7F1] p-3 text-[11px]">
       <strong>SDK 작업 영수증 · {sdkJob.status}</strong>
       {sdkJob.result?.verifiedFileWrites!==undefined&&<p>실제 SHA 확인 파일 수정 {sdkJob.result.verifiedFileWrites}개 · 독립 작업 품질 검사 미수행</p>}
       {sdkJob.result?.acceptanceGaps?.length>0&&<p className="text-amber-800">미충족 검증: {sdkJob.result.acceptanceGaps.join(' · ')}</p>}
       {sdkJob.result?.reason&&<p>{String(sdkJob.result.reason)}</p>}
       {sdkJob.error&&<p className="text-rose-700">{String(sdkJob.error)}</p>}
       {sdkJob.result?.output&&<pre className="mt-2 max-h-44 overflow-auto whitespace-pre-wrap break-words">{String(sdkJob.result.output).slice(0,6000)}</pre>}
       {sdkJob.result?.fileReceipts?.length>0&&<p className="break-all">변경 파일: {sdkJob.result.fileReceipts.map((r:any)=>r.path).join(', ')}</p>}
      </div>}
     </div>
    </Panel>
    <Panel className="min-w-0 p-5">
     <Subheading title={t('dev.changes')} extra={<div className="flex items-center gap-2"><Button variant="ghost" className="!px-2 !py-1 !text-[10px]" onClick={()=>void loadDiff()}><RefreshCw size={12}/> {t('dev.refresh')}</Button><span className="text-[11px] text-[#8A8176]">{t('dev.count',{count:diff?.count||0})}</span></div>}/>
     <div className="mb-3 flex gap-2"><TextInput value={commitMessage} onChange={e=>setCommitMessage(e.target.value)} placeholder={t('dev.commitMessage')}/><Button variant="secondary" className="shrink-0 whitespace-nowrap" disabled={!diff?.count} onClick={()=>void commit()}><GitCommit size={14}/> {t('dev.commit')}</Button></div>
     <div className="my-3 grid gap-2 rounded-xl border border-[#E7D8C8] bg-[#FFF9F1] p-3">
       <strong className="text-xs">실제 Git 차이 → 에이전트 검토 워크플로</strong>
       <p className="text-[11px] text-[#7C7469]">현재 Git diff를 선택한 워크플로의 실제 입력으로 전달하고, 단계별 도구 영수증과 최종 검토 보고서를 저장합니다. 실행은 선택·버튼 확인 후에만 시작합니다.</p>
       <SelectInput value={reviewFlow} onChange={e=>{setReviewFlow(e.target.value);setReviewRequestId('')}}>
        <option value="">검토할 워크플로 선택</option>
        {flows.value.map(flow=><option key={flow.id} value={flow.id}>{flow.name} · {flow.steps.length}단계</option>)}
       </SelectInput>
       <div className="flex flex-wrap items-center gap-2">
        <Button variant="secondary" disabled={!s.git||!diff?.count||!reviewFlow||reviewBusy} onClick={()=>void reviewDiff()}>
         <Code2 size={14}/> {reviewBusy?'검토 작업 등록 중':'실제 변경 검토 실행'}
        </Button>
        <a href="#/workflows" className="text-xs text-[#A94E30] underline">워크플로·완성 산출물 열기</a>
       </div>
     </div>
     {diff?.changes.length?<div className="grid max-h-[420px] gap-2 overflow-auto">{diff.changes.map(change=><details key={change.path} className="rounded-lg border border-[#EFE9DF] bg-[#FAF7F1] p-2"><summary className="cursor-pointer text-[11px] font-semibold text-[#5C544A]">{change.status} · {change.path}</summary><pre className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-all text-[10px] leading-4 text-[#3A342D]">{change.patch}</pre></details>)}</div>:<p className="text-[11px] text-[#8A8176]">{t('dev.noChanges')}</p>}
    </Panel>
   </div>
   <div className="grid min-w-0 gap-4">
    <Panel className="min-w-0 p-5">
     <Subheading title="DOMINION 업데이트" extra={<Button variant="ghost" size="sm" onClick={()=>void updates.reload()}><RefreshCw size={14}/> 최신 버전 확인</Button>}/>
     <p className="text-[12px] leading-5 text-[#6F675D]">Mac 소스 설치 환경에서는 업데이트 확인부터 빌드·재시작까지 앱 안에서 처리합니다. 기존 사용자 데이터와 연결 설정은 유지합니다.</p>
     <div className="mt-3 grid gap-1 text-[11px] text-[#8A8176]">
      {updates.value.running&&<div>현재 실행 중 · {updates.value.running.slice(0,12)}</div>}
      {updates.value.local&&<div>소스 버전 · {updates.value.local.slice(0,12)}</div>}
      {updates.value.running&&updates.value.local&&updates.value.running!==updates.value.local&&<div className="text-[#A05B48]">소스와 실행 엔진의 버전이 다릅니다. 재빌드가 필요합니다.</div>}
      {updates.value.remote&&<div>최신 버전 · {updates.value.remote.slice(0,12)}</div>}
      {updates.value.reason&&<div className="text-[#A05B48]">{updates.value.reason}</div>}
      {updates.value.available&&<div className="font-semibold text-emerald-700">새 버전 사용 가능</div>}
      {!updates.value.available&&updates.value.supported&&!updates.value.reason&&<div>현재 최신 소스입니다.</div>}
     </div>
     <div className="mt-3"><Button variant="primary" disabled={Boolean(busy)||!updates.value.supported} onClick={requestUpdate}><RefreshCw size={14}/> {busy==='update'?'업데이트 진행 중':updates.value.available?'업데이트 및 재시작':'재빌드 및 재시작'}</Button></div>
    </Panel>
    <Panel className="min-w-0 p-5">
     <Subheading title={t('dev.logs')} extra={<Terminal size={18}/>}/>
     {job?<div><div className="mb-2 flex items-center gap-2"><StateTag online={job.status==='completed'}>{job.status}</StateTag><span className="text-[11px] text-[#8A8176]">{job.kind}</span></div><pre className="max-h-[520px] min-h-40 overflow-auto whitespace-pre-wrap break-all rounded-xl bg-[#24211D] p-3 text-[10px] leading-5 text-[#E7E0D5]">{job.logs.join('\n')||t('dev.waitingLogs')}</pre></div>:<p className="text-[11px] text-[#8A8176]">{t('dev.logsHint')}</p>}
     {s.preview.logs?.length?<pre className="mt-3 max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-xl bg-[#FAF7F1] p-3 text-[10px] leading-4 text-[#5C544A]">{s.preview.logs.slice(-40).join('\n')}</pre>:null}
    </Panel>
    <Panel className="min-w-0 p-5">
     <Subheading title={t('dev.principles')}/>
     <ul className="grid gap-2 text-[11px] leading-5 text-[#5C544A]">
      <li>{t('dev.principle.installed')}</li>
      <li>{t('dev.principle.workspace')}</li>
      <li>{t('dev.principle.update')}</li>
      <li>{t('dev.principle.preserve')}</li>
     </ul>
    </Panel>
   </div>
  </div>
 </div>
}
