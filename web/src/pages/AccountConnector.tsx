import {useEffect,useState} from 'react'
import {Github,Globe2,Link2,RefreshCw,ShieldCheck} from 'lucide-react'
import {Action,Notice,Panel,StateTag} from '../components/UI'
import {openOAuth,get,post} from '../api'

export type OAuthProvider={id:string;configured:boolean;managed?:boolean;cliAvailable?:boolean;callback?:string|null}
export type AccountConnection={id:string;provider:string;displayName:string;scopes:string;hasToken:boolean;repositoryAccess?:boolean;workspaceAccess?:boolean;tools?:string[]}
type Device={phase:string;message?:string;verificationUrl?:string;userCode?:string;startedAt?:number}

export function AccountConnector({providers,accounts,reload}:{
 providers:OAuthProvider[];accounts:AccountConnection[];reload:()=>void
}){
 const [device,setDevice]=useState<Device|null>(null)
 const [busy,setBusy]=useState(''),[error,setError]=useState(''),[info,setInfo]=useState('')
 useEffect(()=>{
  const done=(event:MessageEvent)=>{
   if(event.origin!==window.location.origin||event.data?.type!=='dominion-oauth-completed')return
   setError('');setInfo('계정 인증이 완료됐습니다. 작업 권한을 다시 조회했습니다.')
   reload()
  }
  window.addEventListener('message',done)
  return ()=>window.removeEventListener('message',done)
 },[reload])
 useEffect(()=>{
  if(!device||!['starting','waiting'].includes(device.phase))return
  let cancelled=false
  let pending:ReturnType<typeof setTimeout>|undefined
  const check=async()=>{
   try{
    const current=await get<Device>('/api/oauth/github/cli/login/status')
    if(cancelled)return
    setDevice(current)
    if(current.phase==='completed'){
     const account=await post('/api/oauth/github/cli/import',{})
     if(cancelled)return
     setDevice(null);setInfo('GitHub 계정이 연결됐습니다.')
     setBusy('');reload();void account;return
    }
    if(['failed','cancelled'].includes(current.phase)){
     setBusy('');if(current.phase==='failed')setError(current.message||'GitHub 로그인 실패')
     return
    }
   }catch(e){
    if(cancelled)return
    setBusy('');setError((e as Error).message);return
   }
   if(!cancelled)pending=setTimeout(()=>void check(),1200)
  }
  pending=setTimeout(()=>void check(),1200)
  return ()=>{cancelled=true;if(pending)clearTimeout(pending)}
 },[device?.startedAt,device?.phase,reload])
 const cliLogin=async()=>{
  setBusy('github');setError('');setInfo('')
  try{
   // Reuse an already authenticated GitHub CLI before starting a new device flow.
   try{
    await post('/api/oauth/github/cli/import',{})
    setBusy('');setInfo('기존 GitHub CLI 로그인 계정을 바로 연결했습니다.')
    reload();return
   }catch{ /* A user with no local account follows official device login. */ }
   const login=await post<Device>('/api/oauth/github/cli/login',{})
   setDevice(login)
  }catch(e){setBusy('');setError((e as Error).message)}
 }
 const cancelLogin=async()=>{
  try{await post('/api/oauth/github/cli/login/cancel',{})}
  catch(e){setError((e as Error).message)}
  setBusy('');setDevice(null)
 }
 const connect=async(provider:OAuthProvider)=>{
  setError('');setInfo('')
  if(provider.id==='github'&&!provider.configured&&provider.cliAvailable){
   await cliLogin();return
  }
  try{await openOAuth(provider.id,provider.id==='github',provider.id==='google')}
  catch(e){setError((e as Error).message)}
 }
 return <>
  {error&&<Notice message={error}/>}
  {info&&<p role="status" className="rounded-xl bg-[#F1F8F2] p-3 text-sm text-[#25633D]">{info}</p>}
  {device&&<div role="status" className="mb-3 rounded-xl border border-[#E7D4B9] bg-[#FFF8EF] p-4 text-sm">
   <strong>GitHub 브라우저 로그인</strong>
   <p className="mt-2">{device.message||'인증 진행 중'}</p>
   {device.userCode&&<p className="mt-2 font-mono text-2xl font-bold tracking-widest">{device.userCode}</p>}
   <div className="mt-3 flex flex-wrap gap-2">
    <a className="rounded-lg bg-[#315C8A] px-3 py-2 text-white" href="https://github.com/login/device" target="_blank" rel="noreferrer">GitHub 인증 페이지 열기</a>
    <Action variant="secondary" onClick={()=>void cancelLogin()}>인증 취소</Action>
   </div>
  </div>}
  <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
   {providers.map(provider=>{
    const account=accounts.find(a=>a.provider===provider.id)
    const gh=provider.id==='github'
    const Icon=gh?Github:Globe2
    const granted=gh?Boolean(account?.repositoryAccess):Boolean(account?.workspaceAccess)
    const connectable=Boolean(provider.configured)||(gh&&Boolean(provider.cliAvailable))
    return <Panel key={provider.id} className="flex min-w-0 flex-col gap-3 p-5">
     <div className="flex items-center gap-3">
      <div className="grid size-10 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]"><Icon size={22}/></div>
      <div><strong>{gh?'GitHub':'Google Workspace'}</strong>
       <p className="text-xs text-[#7C7469]">{gh?'저장소 · PR · 브랜치 · 이슈':'Drive · Docs · Sheets'}</p></div>
     </div>
     <StateTag online={granted}>{granted?'연결됨 · 에이전트 도구 활성화'
       :account?'로그인됨 · 추가 권한 필요':connectable?'로그인 가능':'서비스 연결 미설정'}</StateTag>
     {account&&<p className="text-xs text-[#524A41]">{account.displayName} · 사용 가능한 도구 {account.tools?.length||0}개</p>}
     <div className="flex flex-wrap gap-2">
      <Action disabled={!connectable||busy==='github'} onClick={()=>void connect(provider)}>
       <Link2 size={15}/> {granted?'계정 다시 연결':gh?'GitHub로 로그인':'Google로 로그인'}
      </Action>
      {gh&&provider.cliAvailable&&provider.configured&&<Action variant="secondary" disabled={busy==='github'} onClick={()=>void cliLogin()}>
       <ShieldCheck size={15}/> 기존 GitHub CLI 계정 연결
      </Action>}
      <Action variant="secondary" onClick={reload}><RefreshCw size={15}/> 상태 갱신</Action>
     </div>
     {!connectable&&<p className="text-xs leading-5 text-[#8B5B36]">
      서비스 운영자의 OAuth 등록이 아직 완료되지 않았습니다. 사용자 API 키나 Client ID 입력은 필요하지 않습니다.
     </p>}
     <p className="text-xs leading-5 text-[#766D62]">
      {gh?'로그인 시 저장소 접근을 허용합니다. 파일 변경·PR·이슈·댓글 생성은 별도로 승인합니다.'
       :'로그인 시 Drive·Docs·Sheets 작업 권한을 허용합니다. 파일 생성과 내용 변경은 별도로 승인합니다.'}
     </p>
     {account?.tools?.length?<details className="text-xs text-[#5B5247]">
      <summary className="cursor-pointer">연결된 작업 도구 ({account.tools.length}개)</summary>
      <ul className="mt-2 space-y-1">{account.tools.map(name=><li className="break-all font-mono" key={name}>{name}</li>)}</ul>
     </details>:null}
    </Panel>
   })}
  </div>
 </>
}
