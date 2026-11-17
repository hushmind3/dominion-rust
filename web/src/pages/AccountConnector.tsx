import {useEffect,useState} from 'react'
import {Github,Globe2,KeyRound,Link2,ShieldCheck,X} from 'lucide-react'
import {DialogBackdrop,DialogPanel} from '../components/Dialog'
import {Action,FormField,Notice,Panel,StateTag} from '../components/UI'
import {TextInput} from '../components/Primitives'
import {openOAuth,post} from '../api'
import {t} from '../i18n'

export type OAuthProvider={id:string;configured:boolean;callback?:string|null}
export type AccountConnection={id:string;provider:string;displayName:string;scopes:string;hasToken:boolean;repositoryAccess?:boolean;workspaceAccess?:boolean;tools?:string[]}

export function AccountConnector({providers,accounts,reload}:{
 providers:OAuthProvider[];accounts:AccountConnection[];reload:()=>void
}){
 const [edit,setEdit]=useState<{id:string;clientId:string;clientSecret:string}|null>(null)
 const [saving,setSaving]=useState(false),[error,setError]=useState(''),[info,setInfo]=useState('')
 useEffect(()=>{
  const done=(event:MessageEvent)=>{
   if(event.origin!==window.location.origin||event.data?.type!=='dominion-oauth-completed')return
   setInfo('계정 연결 응답을 받았습니다. 허용된 작업 권한을 다시 조회했습니다.')
   reload()
  }
  window.addEventListener('message',done)
  return ()=>window.removeEventListener('message',done)
 },[reload])
 const connect=async(id:string,mode:'identity'|'repository'|'workspace')=>{
  setError('');setInfo('')
  try{await openOAuth(id,mode==='repository',mode==='workspace')}
  catch(e){setError((e as Error).message)}
 }
 const saveApp=async()=>{
  if(!edit||saving)return
  setSaving(true);setError('')
  try{
   await post('/api/oauth/providers/'+encodeURIComponent(edit.id),{
    clientId:edit.clientId.trim(),clientSecret:edit.clientSecret
   })
   setEdit(null);setInfo('OAuth 앱 설정이 서버의 암호화 저장소에 저장됐습니다.')
   reload()
  }catch(e){setError((e as Error).message)}finally{setSaving(false)}
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
    return <Panel key={provider.id} className="flex min-w-0 flex-col gap-3 p-5">
      <div className="flex items-center gap-3"><div className="grid size-10 place-items-center rounded-xl bg-[#F7E8E1] text-[#C96442]"><Icon size={22}/></div>
       <div><strong>{gh?'GitHub':'Google Workspace'}</strong><p className="text-xs text-[#7C7469]">{gh?'저장소 · 브랜치 · 파일 · PR · 이슈':'Drive · Docs · Sheets'}</p></div>
      </div>
      <StateTag online={granted}>{granted?'작업 권한 연결됨':account?'계정 로그인됨 · 작업 권한 필요':provider.configured?'연결 준비 완료':'OAuth 앱 설정 필요'}</StateTag>
      {account&&<p className="text-xs text-[#524A41]">{account.displayName} · 사용 가능한 도구 {account.tools?.length||0}개</p>}
      <div className="flex flex-wrap gap-2">
       <Action variant="secondary" onClick={()=>setEdit({id:provider.id,clientId:'',clientSecret:''})}>
        <KeyRound size={15}/> {provider.configured?'OAuth 앱 수정':'OAuth 앱 설정'}
       </Action>
       <Action variant="secondary" disabled={!provider.configured} onClick={()=>void connect(provider.id,'identity')}>
        <Link2 size={15}/> {account?'계정 재연결':'계정 로그인'}
       </Action>
       <Action disabled={!provider.configured} onClick={()=>void connect(provider.id,gh?'repository':'workspace')}>
        <ShieldCheck size={15}/> {gh?'GitHub 작업 권한 연결':'Google 문서 작업 권한 연결'}
       </Action>
      </div>
      <p className="text-xs leading-5 text-[#766D62]">
       {gh?'별도 repo 권한을 허용하면 비공개 저장소까지 접근할 수 있습니다. 파일 쓰기·PR·이슈·댓글은 작업마다 승인합니다.'
        :'별도 Google Drive 전체 파일 접근 권한과 Docs·Sheets 수정 권한을 요청합니다. 삭제 도구는 제공하지 않으며 모든 쓰기는 승인 후 실행합니다.'}
      </p>
      {provider.callback&&<div className="break-all rounded-lg bg-[#FAF7F1] p-2 text-xs"><strong>OAuth callback URL</strong><code className="mt-1 block break-all">{provider.callback}</code></div>}
      {account?.tools?.length? <details className="text-xs text-[#5B5247]"><summary className="cursor-pointer">실행 가능한 연결 도구 ({account.tools.length}개)</summary>
       <ul className="mt-2 space-y-1">{account.tools.map(name=><li className="break-all font-mono" key={name}>{name}</li>)}</ul></details>:null}
    </Panel>
   })}
  </div>
  {edit&&<DialogBackdrop onDismiss={()=>{if(!saving)setEdit(null)}}>
   <DialogPanel aria-label="OAuth 앱 설정">
    <header className="flex items-center justify-between border-b p-5"><h3>OAuth 앱 설정 · {edit.id==='github'?'GitHub':'Google'}</h3>
     <Action variant="secondary" onClick={()=>setEdit(null)}><X size={16}/>{t('conn.cancel')}</Action></header>
    <div className="grid gap-4 p-5">
     <p className="text-sm leading-6 text-[#62594E]">OAuth 앱을 1회 등록하면 이후에는 계정 로그인과 작업 권한 연결만 사용합니다. 비밀키는 서버에 암호화 저장되며 에이전트에게 전달되지 않습니다.</p>
     <FormField label="Client ID"><TextInput autoComplete="off" value={edit.clientId}
      onChange={e=>setEdit({...edit,clientId:e.target.value})}/></FormField>
     <FormField label="Client Secret"><TextInput type="password" autoComplete="off" value={edit.clientSecret}
      onChange={e=>setEdit({...edit,clientSecret:e.target.value})}/></FormField>
     <p className="text-xs text-[#776D60]">콜백 주소는 위 카드의 URL로 OAuth 콘솔에 등록하세요.</p>
     <a className="text-sm text-[#315C8A] underline" href={edit.id==='github'
       ?'https://github.com/settings/developers':'https://console.cloud.google.com/apis/credentials'}
       target="_blank" rel="noreferrer">OAuth 앱 관리 콘솔 열기</a>
     <div className="flex justify-end gap-2"><Action variant="secondary" onClick={()=>setEdit(null)}>취소</Action>
      <Action disabled={saving||!edit.clientId.trim()||!edit.clientSecret} onClick={()=>void saveApp()}>
       {saving?'저장 중':'앱 설정 저장'}</Action></div>
    </div>
   </DialogPanel>
  </DialogBackdrop>}
 </>
}
