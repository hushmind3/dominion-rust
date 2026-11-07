import {t,useLocale,formatDateTime} from '../i18n'
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
 const loc=useLocale()
 const status=useRemote<DevStatus>('/api/dev/status',{config:{sourceDir:'',repoUrl:'',branch:'main',previewPort:5174},exists:false,nodeModules:false,git:false,preview:{status:'stopped',url:''},platform:'',arch:'',installedExe:''})
 const [sourceDir,setSourceDir]=useState(''),[repoUrl,setRepoUrl]=useState(''),[branch,setBranch]=useState('main')
 const [job,setJob]=useState<DevJob|null>(null),[jobId,setJobId]=useState('')
 const [diff,setDiff]=useState<DiffResult|null>(null),[commitMessage,setCommitMessage]=useState('')
 const [error,setError]=useState(''),[message,setMessage]=useState(''),[busy,setBusy]=useState('')
 useEffect(()=>{setSourceDir(status.value.config.sourceDir||'');setRepoUrl(status.value.config.repoUrl||'');setBranch(status.value.config.branch||'main')},[status.value.config.sourceDir,status.value.config.repoUrl,status.value.config.branch])
 useEffect(()=>{
  if(!jobId)return
  const timer=setInterval(async()=>{
   try{const next=await get<DevJob>('/api/dev/jobs/'+jobId);setJob(next);if(terminal(next.status)){clearInterval(timer);setBusy('');void status.reload();if(next.status==='completed'){setMessage(next.kind==='commit'&&typeof next.result?.oid==='string'?t('dev.commitDone',{oid:next.result.oid.slice(0,12)}):t('dev.jobDone',{kind:next.kind}));if(next.kind==='commit')setCommitMessage('');void loadDiff()}else setError(next.error||t('dev.jobFailed'))}}
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
 const commit=async()=>{
  const message=commitMessage.trim();
  if(!message){setError(t('dev.commitMessageRequired'));return}
  await run('commit',{message});
 }
 const s=status.value
 return <div className="mx-auto w-full max-w-[1280px] px-5 py-4 sm:px-7 lg:px-6 lg:pb-8">
  <Headline eyebrow="SELF DEVELOPMENT" title={t('dev.title')} description={t('dev.description')} actions={<Button variant="secondary" onClick={()=>{void status.reload();void loadDiff()}}><RefreshCw size={16}/> {t('dev.statusRefresh')}</Button>}/>
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
     <Subheading title={t('dev.changes')} extra={<div className="flex items-center gap-2"><Button variant="ghost" className="!px-2 !py-1 !text-[10px]" onClick={()=>void loadDiff()}><RefreshCw size={12}/> {t('dev.refresh')}</Button><span className="text-[11px] text-[#8A8176]">{t('dev.count',{count:diff?.count||0})}</span></div>}/>
     <div className="mb-3 flex gap-2"><TextInput value={commitMessage} onChange={e=>setCommitMessage(e.target.value)} placeholder={t('dev.commitMessage')}/><Button variant="secondary" className="shrink-0 whitespace-nowrap" disabled={!diff?.count} onClick={()=>void commit()}><GitCommit size={14}/> {t('dev.commit')}</Button></div>
     {diff?.changes.length?<div className="grid max-h-[420px] gap-2 overflow-auto">{diff.changes.map(change=><details key={change.path} className="rounded-lg border border-[#EFE9DF] bg-[#FAF7F1] p-2"><summary className="cursor-pointer text-[11px] font-semibold text-[#5C544A]">{change.status} · {change.path}</summary><pre className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-all text-[10px] leading-4 text-[#3A342D]">{change.patch}</pre></details>)}</div>:<p className="text-[11px] text-[#8A8176]">{t('dev.noChanges')}</p>}
    </Panel>
   </div>
   <div className="grid min-w-0 gap-4">
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
