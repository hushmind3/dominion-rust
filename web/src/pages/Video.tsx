import {useEffect,useRef,useState} from 'react'
import {Check,ChevronRight,Clapperboard,Download,ExternalLink,Film,LoaderCircle,Play,RefreshCw,Settings2,Upload} from 'lucide-react'
import {get,post} from '../api'
import {Button,TextArea,TextInput,SelectInput} from '../components/Primitives'
import {FormField,Notice,useRemote} from '../components/UI'
import {PageToolbar} from '../components/PageToolbar'
import './Video.css'

type Engine='native'|'comfyui'
type Privacy='private'|'unlisted'|'public'
type VideoConfig={engine?:Engine;comfyUrl?:string;comfyWorkflow?:unknown;workflowDir?:string;outputDir?:string;[key:string]:unknown}
type VideoJob={id:string;kind?:string;videoJobId?:string;status:string;phase?:string;progress?:number;title?:string;brief?:string;duration?:number;aspectRatio?:string;engine?:Engine;error?:string;outputUrl?:string;createdAt?:string;created_at?:string;url?:string;result?:{url?:string;filename?:string};upload?:{id?:string;status?:string;url?:string;error?:string}}
type VideoStatus={config:VideoConfig;native?:{available:boolean};ffmpeg?:{available:boolean;path?:string};comfy?:{reachable:boolean};engines?:{id:Engine;name:string;available:boolean;reason?:string}[];jobs?:VideoJob[];available?:boolean}
const terminal=(status?:string)=>Boolean(status&&['completed','failed','cancelled','needs_review','interrupted'].includes(status))
const phaseLabel:Record<string,string>={queued:'대기 중',running:'작업 중',completed:'완료',failed:'실패',needs_review:'확인 필요',cancelled:'취소됨',storyboard:'구성 작성',drafting:'구성 작성',rendering:'장면 렌더링',encoding:'영상 저장',uploading:'YouTube 업로드',uploaded:'업로드 완료'}
const label=(value?:string)=>value?(phaseLabel[value]||value):'대기'
const output=(job?:VideoJob|null)=>job?.outputUrl||job?.result?.url||''
const jobTime=(value?:string)=>{if(!value)return '';const date=new Date(value.includes('T')?value:value.replace(' ','T')+'Z');return Number.isNaN(date.getTime())?'':date.toLocaleString('ko-KR',{month:'short',day:'numeric',hour:'2-digit',minute:'2-digit'})}

export function VideoPage({navigate}:{navigate:(page:any)=>void}){
 const status=useRemote<VideoStatus>('/api/video/status',{config:{}},false)
 const jobs=useRemote<VideoJob[]>('/api/video/jobs',[],false)
 const [brief,setBrief]=useState(''),[title,setTitle]=useState(''),[duration,setDuration]=useState(15),[aspectRatio,setAspectRatio]=useState('9:16'),[engine,setEngine]=useState<Engine>('native'),[selected,setSelected]=useState(''),[job,setJob]=useState<VideoJob|null>(null)
 const [starting,setStarting]=useState(false),[error,setError]=useState(''),[notice,setNotice]=useState(''),[refreshing,setRefreshing]=useState(false)
 const [comfyUrl,setComfyUrl]=useState(''),[ffmpegPath,setFfmpegPath]=useState(''),[workflow,setWorkflow]=useState(''),[configDirty,setConfigDirty]=useState(false),[saving,setSaving]=useState(false),[advanced,setAdvanced]=useState(false)
 const [privacy,setPrivacy]=useState<Privacy>('private'),[uploading,setUploading]=useState(false),[uploadJob,setUploadJob]=useState<VideoJob|null>(null)
 const pollInFlight=useRef(false),selectedRef=useRef(selected)
 selectedRef.current=selected
 const nativeAvailable=status.value.native?.available??status.value.ffmpeg?.available??false
 const comfyAvailable=status.value.engines?.find(item=>item.id==='comfyui')?.available??Boolean(status.value.comfy?.reachable&&status.value.config.comfyWorkflow)
 const available=engine==='native'?nativeAvailable:comfyAvailable
 const currentJob=job?.id===selected?job:jobs.value.find(item=>item.id===selected)||null
 const currentOutput=output(currentJob)
 const allJobs=jobs.value.length?jobs.value:status.value.jobs||[]
 const rows=allJobs.filter(item=>item.kind!=='youtube_upload')
 const uploadUrl=uploadJob?.result?.url||uploadJob?.url||currentJob?.upload?.url
 useEffect(()=>{if(!configDirty){setComfyUrl(status.value.config.comfyUrl||'');setFfmpegPath(typeof status.value.config.ffmpegPath==='string'?status.value.config.ffmpegPath:'');setWorkflow(status.value.config.comfyWorkflow?JSON.stringify(status.value.config.comfyWorkflow,null,2):'')}},[status.value.config,configDirty])
 useEffect(()=>{if(!selected&&rows.length){setSelected(rows[0].id);setJob(rows[0])}},[rows,selected])
 useEffect(()=>{setJob(rows.find(item=>item.id===selected)||null);setUploadJob(allJobs.find(item=>item.kind==='youtube_upload'&&item.videoJobId===selected)||null)},[selected])
 const refreshAll=async()=>{setRefreshing(true);try{await Promise.all([status.reload(),jobs.reload()]);if(selected)setJob(await get<VideoJob>('/api/video/jobs/'+selected))}catch(e){setError((e as Error).message)}finally{setRefreshing(false)}}
 useEffect(()=>{
  if(!currentJob||terminal(currentJob.status))return
  let mounted=true
  const timer=setInterval(async()=>{if(pollInFlight.current)return;pollInFlight.current=true;try{const next=await get<VideoJob>('/api/video/jobs/'+currentJob.id);if(mounted){setJob(next);if(terminal(next.status))void jobs.reload()}}catch(e){if(mounted)setError((e as Error).message)}finally{pollInFlight.current=false}},1500)
  return()=>{mounted=false;clearInterval(timer)}
 },[currentJob?.id,currentJob?.status])
 useEffect(()=>{
  if(!uploadJob||terminal(uploadJob.status))return
  let mounted=true
  const timer=setInterval(async()=>{try{const next=await get<VideoJob>('/api/video/jobs/'+uploadJob.id);if(mounted)setUploadJob(next)}catch(e){if(mounted)setError((e as Error).message)}},1800)
  return()=>{mounted=false;clearInterval(timer)}
 },[uploadJob?.id,uploadJob?.status])
 const generate=async()=>{
  if(starting||!brief.trim())return;setStarting(true);setError('');setNotice('')
  try{const created=await post<VideoJob>('/api/video/jobs',{brief:brief.trim(),title:title.trim(),duration,aspectRatio,engine,publish:false});setSelected(created.id);setJob(created);setUploadJob(null);await jobs.reload();setNotice('영상 작업을 등록했습니다. Rust 서버에서 생성하고 결과를 저장합니다.')}
  catch(e){setError((e as Error).message)}finally{setStarting(false)}
 }
 const saveConfig=async()=>{if(saving)return;setSaving(true);setError('');try{let template:unknown=null;if(workflow.trim()){try{template=JSON.parse(workflow);if(!template||typeof template!=='object'||Array.isArray(template)||!Object.keys(template).length)throw Error('empty')}catch{throw Error('ComfyUI 워크플로는 API 형식 JSON으로 입력하세요.')}}await post('/api/video/config',{...status.value.config,comfyUrl:comfyUrl.trim(),comfyWorkflow:template,ffmpegPath:ffmpegPath.trim()});setConfigDirty(false);await status.reload();setNotice('영상 엔진 설정을 저장했습니다.')}catch(e){setError((e as Error).message)}finally{setSaving(false)}}
 const connectYouTube=async()=>{setError('');try{const result=await post<{opened:boolean}>('/api/oauth/youtube/open',{});if(!result.opened)throw Error('시스템 브라우저를 열 수 없습니다.');setNotice('시스템 브라우저에서 YouTube 업로드 권한을 연결한 뒤 돌아오세요.')}catch(e){setError((e as Error).message)}}
 const upload=async()=>{if(!currentJob||uploading||!currentOutput)return;setUploading(true);setError('');try{const videoId=currentJob.id;const queued=await post<VideoJob>('/api/video/jobs/'+videoId+'/upload',{privacyStatus:privacy});if(selectedRef.current===videoId)setUploadJob(queued);void jobs.reload();setNotice('YouTube 업로드 작업을 등록했습니다. 업로드 완료 상태와 링크를 확인하세요.')}catch(e){setError((e as Error).message)}finally{setUploading(false)}}
 return <div className="video-studio">
  <PageToolbar><strong className="page-toolbar-title">영상 제작</strong><Button size="sm" variant="ghost" onClick={()=>void refreshAll()} disabled={refreshing}><RefreshCw size={15} className={refreshing?'animate-spin':''}/>새로고침</Button><Button size="sm" variant="ghost" onClick={()=>setAdvanced(value=>!value)}><Settings2 size={15}/>엔진 설정</Button></PageToolbar>
  <Notice message={error||status.error||jobs.error}/>{notice&&<div className="video-notice" role="status"><Check size={15}/>{notice}</div>}
  <div className="video-studio-grid"><section className="video-create-panel"><div className="video-section-title"><Clapperboard size={19}/><h2>주제에서 영상까지</h2></div><FormField label="어떤 영상을 만들까요?"><TextArea rows={5} value={brief} onChange={e=>setBrief(e.target.value)} placeholder="예: 오늘의 집중 루틴을 세 장면으로 소개하는 짧은 영상. 첫 장면은 핵심 질문, 다음은 방법, 마지막은 한 줄 정리."/></FormField><FormField label="영상 제목 · 선택"><TextInput value={title} onChange={e=>setTitle(e.target.value)} maxLength={100} placeholder="제목을 비우면 주제에서 만듭니다."/></FormField><div className="video-form-pair"><FormField label="길이"><SelectInput value={duration} onChange={e=>setDuration(Number(e.target.value))}>{[5,10,15,30,60,120,180].map(seconds=><option key={seconds} value={seconds}>{seconds}초</option>)}</SelectInput></FormField><FormField label="화면 비율"><SelectInput value={aspectRatio} onChange={e=>setAspectRatio(e.target.value)}><option value="9:16">세로 · 9:16</option><option value="16:9">가로 · 16:9</option><option value="1:1">정사각 · 1:1</option></SelectInput></FormField></div>
   <fieldset className="video-engines"><legend>실행할 영상 엔진</legend><label className={engine==='native'?'is-selected':''}><input type="radio" name="video-engine" value="native" checked={engine==='native'} onChange={()=>setEngine('native')}/><Film size={18}/><span><strong>DOMINION 자막 영상</strong><small>장면별 자막·배경 카드 · 무음 MP4</small></span><em>{status.loading?'확인 중':nativeAvailable?'사용 가능':'FFmpeg 필요'}</em></label><label className={engine==='comfyui'?'is-selected':''}><input type="radio" name="video-engine" value="comfyui" checked={engine==='comfyui'} onChange={()=>{setEngine('comfyui');if(!comfyAvailable)setAdvanced(true)}}/><Clapperboard size={18}/><span><strong>연결한 ComfyUI</strong><small>내 API 워크플로로 영상 생성</small></span><em>{status.loading?'확인 중':comfyAvailable?'연결됨':'설정 필요'}</em></label></fieldset>
   {!available&&!status.loading&&<div className="video-setup-note">{engine==='native'?'FFmpeg가 확인되지 않았습니다. 엔진 설정에서 실행 환경을 확인하세요.':'ComfyUI 주소와 API 워크플로를 먼저 설정하세요.'}<button onClick={()=>setAdvanced(true)}>엔진 설정 <ChevronRight size={13}/></button></div>}
   <Button variant="primary" className="video-generate" disabled={starting||!brief.trim()||!available} onClick={()=>void generate()}>{starting?<LoaderCircle size={17} className="animate-spin"/>:<Play size={17}/>}영상 만들기</Button><p className="video-generation-note">생성 결과를 먼저 확인한 뒤 업로드할 수 있습니다.</p>
  </section>
  <section className="video-output-panel"><div className="video-section-title"><Film size={18}/><h2>{currentJob?.title||'생성 결과'}</h2>{currentJob&&<span>{label(currentJob.phase||currentJob.status)}</span>}</div>{currentJob?<><div className="video-job-summary"><span>{currentJob.engine==='comfyui'?'ComfyUI':currentJob.engine==='native'?'자막 영상':'영상 작업'}</span>{currentJob.duration&&<span>{currentJob.duration}초</span>}{currentJob.aspectRatio&&<span>{currentJob.aspectRatio}</span>}<span>{jobTime(currentJob.createdAt||currentJob.created_at)}</span></div>{currentOutput?<video key={currentOutput} src={currentOutput} controls playsInline className="video-player"/>:<div className="video-job-progress">{terminal(currentJob.status)?<Clapperboard size={30}/>:<LoaderCircle size={30} className="animate-spin"/>}<strong>{label(currentJob.phase||currentJob.status)}</strong>{typeof currentJob.progress==='number'&&<><div><i style={{width:Math.min(100,Math.max(0,currentJob.progress))+'%'}}/></div><span>{Math.round(currentJob.progress)}%</span></>}{currentJob.brief&&<p>{currentJob.brief}</p>}</div>}{currentJob.error&&<Notice message={currentJob.error}/>}<div className="video-output-actions">{currentOutput&&<a href={currentOutput} download><Download size={15}/>MP4 저장</a>}<button onClick={()=>void refreshAll()}>작업 상태 확인</button></div>{currentOutput&&<div className="video-upload"><div><strong>YouTube에 업로드</strong><button onClick={()=>void connectYouTube()}>YouTube 계정 연결 <ExternalLink size={13}/></button></div><div><SelectInput aria-label="YouTube 공개 범위" value={privacy} onChange={e=>setPrivacy(e.target.value as Privacy)}><option value="private">비공개</option><option value="unlisted">일부 공개</option><option value="public">공개</option></SelectInput><Button variant="primary" onClick={()=>void upload()} disabled={uploading||Boolean(uploadJob&&!terminal(uploadJob.status))}>{uploading?<LoaderCircle size={15} className="animate-spin"/>:<Upload size={15}/>}업로드 시작</Button></div>{uploadJob&&<p>{label(uploadJob.phase||uploadJob.status)}{typeof uploadJob.progress==='number'?' · '+Math.round(uploadJob.progress)+'%':''}</p>}{uploadJob?.error&&<Notice message={uploadJob.error}/>} {uploadUrl&&<a href={uploadUrl} target="_blank" rel="noreferrer">YouTube 결과 열기 <ExternalLink size={14}/></a>}<small>계정에 YouTube 업로드 권한이 필요합니다.</small></div>}</>:<div className="video-empty-output"><Clapperboard size={30}/><strong>만들 영상을 간단히 설명하세요.</strong><p>엔진이 준비되면 생성 버튼 한 번으로 작업을 시작하고,<br/>완료된 MP4를 이곳에서 재생·저장할 수 있습니다.</p></div>}</section></div>
  {advanced&&<section className="video-advanced"><div className="video-section-title"><Settings2 size={17}/><h2>엔진 고급 설정</h2><button onClick={()=>setAdvanced(false)}>접기</button></div><div className="video-environment"><span>FFmpeg</span><strong>{nativeAvailable?'확인됨':'확인되지 않음'}</strong>{status.value.ffmpeg?.path&&<code>{status.value.ffmpeg.path}</code>}</div><FormField label="FFmpeg 실행 파일 · 비우면 자동 검색"><TextInput value={ffmpegPath} onChange={e=>{setFfmpegPath(e.target.value);setConfigDirty(true)}} placeholder="/opt/homebrew/bin/ffmpeg"/></FormField><div className="video-advanced-grid"><FormField label="ComfyUI 서버 주소"><TextInput value={comfyUrl} onChange={e=>{setComfyUrl(e.target.value);setConfigDirty(true)}} placeholder="http://127.0.0.1:8188"/></FormField><div><strong className="video-advanced-label">API 워크플로</strong><p>ComfyUI의 API 형식 JSON을 저장합니다. 연결 주소만으로 모델이나 프로그램을 자동 설치하지 않습니다.</p></div></div><TextArea rows={7} value={workflow} onChange={e=>{setWorkflow(e.target.value);setConfigDirty(true)}} placeholder="ComfyUI에서 내보낸 API 워크플로 JSON"/><div className="video-advanced-actions"><Button disabled={saving||!configDirty} onClick={()=>void saveConfig()}>{saving?'저장 중':'엔진 설정 저장'}</Button><Button variant="ghost" onClick={()=>navigate('connections')}>계정·채널 연결</Button></div></section>}
  <section className="video-history"><div className="video-section-title"><h2>영상 작업 기록</h2><span>{rows.length}개</span></div>{rows.length?<div>{rows.map(item=><button key={item.id} className={item.id===selected?'is-selected':''} onClick={()=>{setSelected(item.id);setJob(item)}}><Clapperboard size={17}/><span><strong>{item.title||item.brief||'영상 작업'}</strong><small>{jobTime(item.createdAt||item.created_at)}</small></span><em>{label(item.status)}</em></button>)}</div>:<p>아직 등록된 영상 작업이 없습니다.</p>}</section>
 </div>
}
