import {useEffect,useState} from 'react'
import {Check,ImagePlus,LoaderCircle,Pencil,UserRound,X} from 'lucide-react'
import {post} from '../api'
import {Button,TextInput} from './Primitives'

export type OwnerProfile={id?:string;name:string;avatar:string;updatedAt?:string|null}
export const defaultOwnerProfile:OwnerProfile={name:'나',avatar:''}

/** One owner identity, shared by the workspace, chats and multi-agent rooms. */
export function OwnerAvatar({profile,size='normal',className=''}:{profile:OwnerProfile;size?:'small'|'normal';className?:string}){
 return <span className={'inline-grid shrink-0 place-items-center overflow-hidden rounded-xl bg-[#E9E2D7] text-[#6C594A] '+(size==='small'?'size-7':'size-9')+' '+className}>
  {profile.avatar?<img src={profile.avatar} alt="" className="h-full w-full object-cover"/>:<UserRound size={size==='small'?15:19}/>}
 </span>
}

export function OwnerProfileEditor({profile,onSaved}:{profile:OwnerProfile;onSaved:(profile:OwnerProfile)=>void}){
 const [open,setOpen]=useState(false),[name,setName]=useState(profile.name),[avatar,setAvatar]=useState(profile.avatar)
 const [working,setWorking]=useState(false),[error,setError]=useState('')
 useEffect(()=>{if(!open){setName(profile.name);setAvatar(profile.avatar)}},[profile,open])
 const close=()=>{setOpen(false);setError('')}
 const save=async()=>{
  if(!name.trim()){setError('표시할 이름을 입력하세요.');return}
  setWorking(true);setError('')
  try{
   const next=await post<OwnerProfile>('/api/settings/owner-profile',{name:name.trim(),avatar})
   onSaved(next);close()
  }catch(e){setError((e as Error).message)}finally{setWorking(false)}
 }
 const upload=(file?:File)=>{
  if(!file)return
  if(!['image/png','image/jpeg','image/webp'].includes(file.type)){setError('PNG/JPEG/WebP 이미지를 선택하세요.');return}
  if(file.size>1024*1024){setError('이미지는 1MB 이하여야 합니다.');return}
  const reader=new FileReader()
  reader.onload=()=>{setAvatar(String(reader.result||''));setError('')}
  reader.onerror=()=>setError('이미지를 읽지 못했습니다.')
  reader.readAsDataURL(file)
 }
 return <div className="relative shrink-0 border-t border-[#E0D8CC] p-3">
  <Button variant="bare" type="button" onClick={()=>setOpen(x=>!x)} aria-expanded={open} aria-label="내 프로필 편집"
   className="group flex w-full items-center gap-2.5 rounded-xl px-2 py-2 text-left hover:bg-[#E8E0D3]">
   <OwnerAvatar profile={profile}/>
   <span className="min-w-0 flex-1"><strong className="block truncate text-[12px] text-[#3A342D]">{profile.name}</strong><small className="block text-[10px] text-[#8A8176]">내 프로필</small></span>
   <Pencil size={13} className="text-[#998B7B] group-hover:text-[#A94E30]"/>
  </Button>
  {open&&<div role="dialog" aria-label="내 프로필" className="absolute bottom-full left-3 z-[70] w-[min(304px,calc(100vw-24px))] rounded-2xl border border-[#E7D8C7] bg-[#FFFDF9] p-4 shadow-[0_18px_56px_rgba(43,36,29,.2)]">
   <div className="mb-3 flex items-center justify-between"><strong className="text-[13px] text-[#3A342D]">내 프로필</strong><Button type="button" variant="bare" aria-label="닫기" onClick={close} className="rounded-lg p-1 hover:bg-[#F3EEE5]"><X size={16}/></Button></div>
   <div className="mb-3 flex items-center gap-3">
    <OwnerAvatar profile={{name,avatar}} className="!size-14"/>
    <label className="flex cursor-pointer items-center gap-1.5 rounded-lg border border-[#E7E0D5] bg-white px-2.5 py-2 text-[11px] font-semibold hover:bg-[#FAF7F1]"><ImagePlus size={14}/> 이미지 변경
     <input type="file" accept="image/png,image/jpeg,image/webp" className="sr-only" onChange={e=>{upload(e.target.files?.[0]);e.target.value=''}}/>
    </label>
    {avatar&&<Button type="button" variant="bare" onClick={()=>setAvatar('')} className="text-[11px] text-[#A94E30]">제거</Button>}
   </div>
   <label className="block text-[11px] font-semibold text-[#6D6052]">표시 이름</label>
   <TextInput value={name} maxLength={60} onChange={e=>setName(e.target.value)} className="mt-1 w-full" onKeyDown={e=>{if(e.key==='Enter')void save()}}/>
   {error&&<p role="alert" className="mt-2 text-[11px] text-rose-600">{error}</p>}
   <Button type="button" variant="primary" onClick={()=>void save()} disabled={working||!name.trim()} className="mt-3 flex w-full items-center justify-center gap-1.5">
    {working?<LoaderCircle size={14} className="animate-spin"/>:<Check size={14}/>} 저장
   </Button>
  </div>}
 </div>
}
