import {useEffect,useState} from 'react'
import {Github,Globe2,Link2,RefreshCw,ShieldCheck} from 'lucide-react'
import {Action,Notice,Panel,StateTag} from '../components/UI'
import {openOAuth,post} from '../api'

export type OAuthProvider={id:string;configured:boolean;managed?:boolean;cliAvailable?:boolean;callback?:string|null}
export type AccountConnection={id:string;provider:string;displayName:string;scopes:string;hasToken:boolean;repositoryAccess?:boolean;workspaceAccess?:boolean;tools?:string[]}

/** A user connects an account, not a provider developer project.
 * OAuth app credentials are registered centrally by the product operator. */
export function AccountConnector({providers,accounts,reload}:{
 providers:OAuthProvider[];accounts:AccountConnection[];reload:()=>void
}){
 const [busy,setBusy]=useState(''),[error,setError]=useState(''),[info,setInfo]=useState('')
 useEffect(()=>{
  const done=(event:MessageEvent)=>{
   if(event.origin!==window.location.origin||event.data?.type!=='dominion-oauth-completed')return
   setError('');setInfo('계정 인증이 완료됐습니다. 연결 권한을 갱신했습니다.')
   reload()
  }
  window.addEventListener('message',done)
  return ()=>window.removeEventListener('message',done)
 },[reload])
 const connect=async(id:string)=>{
  setError('');setInfo('')
  try{await openOAuth(id,id==='github',id==='google')}
  catch(e){setError((e as Error).message)}
 }
 const useLocalGithub=async()=>{
  setBusy('github');setError('');setInfo('')
  try{
   await post('/api/oauth/github/cli/import',{})
   setInfo('로컬 GitHub CLI에 로그인된 계정을 연결했습니다.')
   reload()
  }catch(e){setError((e as Error).message)}
  finally{setBusy('')}
 }
 return <>
  {error&&<Notice message={error}/>}
  {info&&<p role="status" className="rounded-xl bg-[#F1F8F2] p-3 text-sm text-[#25633D]">{info}</p>}
  <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
   {providers.map(provider=>{
    const account=accounts.find(a=>a.provider===provider.id)
    const gh=provider.id==='github'
    const Icon=gh?Github:Globe2
    const granted=gh?Boolean(account?.repositoryAccess):Boolean(account?.workspaceAccess)
    const connectable=Boolean(provider.configured)
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
      <Action disabled={!connectable} onClick={()=>void connect(provider.id)}>
       <Link2 size={15}/> {granted?'계정 다시 연결':gh?'GitHub로 로그인':'Google로 로그인'}
      </Action>
      {gh&&provider.cliAvailable&&<Action variant="secondary" disabled={busy==='github'} onClick={()=>void useLocalGithub()}>
       <ShieldCheck size={15}/> {busy==='github'?'연결 중':'기존 GitHub CLI 계정 연결'}
      </Action>}
      <Action variant="secondary" onClick={reload}><RefreshCw size={15}/> 상태 갱신</Action>
     </div>
     {!connectable&&<p className="text-xs leading-5 text-[#8B5B36]">
      {gh&&provider.cliAvailable?'현재 컴퓨터의 GitHub CLI 로그인을 그대로 연결할 수 있습니다.'
       :'서비스 운영자의 OAuth 등록이 아직 완료되지 않았습니다. 개인 API 키나 Client ID를 입력할 필요는 없습니다.'}
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
