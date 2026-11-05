import {useRef,useState,type FormEvent,type RefObject,type ReactNode,type KeyboardEvent} from 'react'
import {ArrowUp,BrainCircuit,FileText,LoaderCircle,Paperclip,Square,X} from 'lucide-react'
import {post,type Agent,type Provider} from '../api'
import {Button} from './Primitives'

export type Attachment={id:string;name:string;mimeType:string;size:number;url:string;isImage:boolean;supportedForModel?:'image'|'text'|'download';sessionId?:string;roomId?:string}
export type AttachmentBinding={sessionId?:string;roomId?:string}
export type ReasoningEffort=''|'none'|'minimal'|'low'|'medium'|'high'
export type ResponseModel={id:string;name?:string}
export function AttachmentList({attachments,onRemove}:{attachments:Attachment[];onRemove?:(id:string)=>void}){
 if(!attachments.length)return null
 return <div className="conversation-attachments">{attachments.map(item=><div className={'conversation-attachment '+(item.isImage?'is-image':'')} key={item.id}><a href={item.url} target="_blank" rel="noreferrer" title={item.name}>{item.isImage?<img src={item.url} alt={item.name}/>:<FileText size={23}/>}<span><strong>{item.name}</strong><small>{item.size>1024*1024?(item.size/1024/1024).toFixed(1)+' MB':Math.max(1,Math.ceil(item.size/1024))+' KB'}{item.supportedForModel==='download'?' · 다운로드 파일':''}</small></span></a>{onRemove&&<button type="button" onClick={()=>onRemove(item.id)} aria-label={item.name+' 첨부 제거'}><X size={13}/></button>}</div>)}</div>
}

export function ResponseControls({providers,models,providerId,model,onProvider,onModel,agents,agentId,onAgent,agentPlaceholder,thinking,onThinking,effort,onEffort,disabled=false}:{providers:Provider[];models:ResponseModel[];providerId:string;model:string;onProvider:(id:string)=>void;onModel:(id:string)=>void;agents?:Agent[];agentId?:string;onAgent?:(id:string)=>void;agentPlaceholder?:string;thinking:boolean;onThinking:(value:boolean)=>void;effort:ReasoningEffort;onEffort:(value:ReasoningEffort)=>void;disabled?:boolean}){
 return <div className="response-controls">
  {agents&&onAgent&&<label><span className="sr-only">에이전트</span><select aria-label="에이전트" value={agentId||''} onChange={e=>onAgent(e.target.value)} disabled={disabled}>{agentPlaceholder&&<option value="">{agentPlaceholder}</option>}{agents.map(agent=><option key={agent.id} value={agent.id}>{agent.name}</option>)}</select></label>}
  <label><span className="sr-only">모델 제공자</span><select aria-label="모델 제공자" value={providerId} onChange={e=>onProvider(e.target.value)} disabled={disabled}><option value="">에이전트 기본 제공자</option>{providers.filter(provider=>provider.enabled!==false).map(provider=><option key={provider.id} value={provider.id}>{provider.name}</option>)}</select></label>
  <label><span className="sr-only">응답 모델</span><select aria-label="응답 모델" value={model} onChange={e=>onModel(e.target.value)} disabled={disabled}><option value="">에이전트 기본 모델</option>{models.map(item=><option value={item.id} key={item.id}>{item.name||item.id}</option>)}{model&&!models.some(item=>item.id===model)&&<option value={model}>{model}</option>}</select></label>
  <button type="button" className={'thinking-toggle '+(thinking?'is-on':'')} aria-pressed={thinking} disabled={disabled} onClick={()=>onThinking(!thinking)} title="모델의 추론 사용 요청"><BrainCircuit size={15}/>{thinking?'추론 켜짐':'추론 꺼짐'}</button>
  <label><span className="sr-only">추론 강도</span><select aria-label="추론 강도" value={effort} onChange={e=>onEffort(e.target.value as ReasoningEffort)} disabled={disabled}><option value="">강도 자동</option><option value="none">없음 · none</option><option value="minimal">최소 · minimal</option><option value="low">낮음 · low</option><option value="medium">보통 · medium</option><option value="high">높음 · high</option></select></label>
 </div>
}

const readFile=(file:File)=>new Promise<string>((resolve,reject)=>{const reader=new FileReader();reader.onload=()=>resolve(String(reader.result).split(',')[1]||'');reader.onerror=()=>reject(Error(file.name+' 파일을 읽을 수 없습니다.'));reader.readAsDataURL(file)})
export function ConversationComposer({input,onInput,onSend,inputRef,attachments,onAddAttachments,onRemoveAttachment,resolveBinding,busy=false,isRunning=false,onStop,placeholder='메시지를 입력하세요…',onKeyDown,controls,actions,children,footer}:{input:string;onInput:(value:string)=>void;onSend:(event?:FormEvent)=>void|Promise<void>;inputRef?:RefObject<HTMLTextAreaElement|null>;attachments:Attachment[];onAddAttachments:(items:Attachment[],binding:AttachmentBinding)=>void;onRemoveAttachment:(id:string)=>void;resolveBinding:()=>Promise<AttachmentBinding>;busy?:boolean;isRunning?:boolean;onStop?:()=>void|Promise<void>;placeholder?:string;onKeyDown?:(event:KeyboardEvent<HTMLTextAreaElement>)=>void;controls?:ReactNode;actions?:ReactNode;children?:ReactNode;footer?:ReactNode}){
 const picker=useRef<HTMLInputElement>(null),uploadingRef=useRef(false)
 const [uploading,setUploading]=useState(false),[uploadError,setUploadError]=useState(''),[dragging,setDragging]=useState(false)
 const upload=async(files:File[])=>{
  if(uploadingRef.current||busy||!files.length)return
  setUploadError('')
  if(attachments.length+files.length>8){setUploadError('한 메시지에 파일은 최대 8개까지 첨부할 수 있습니다.');return}
  if(files.some(file=>file.size>8*1024*1024)){setUploadError('파일 하나의 크기는 8MB 이하로 선택하세요.');return}
  if(attachments.reduce((sum,file)=>sum+file.size,0)+files.reduce((sum,file)=>sum+file.size,0)>24*1024*1024){setUploadError('첨부 파일의 합계는 24MB 이하로 선택하세요.');return}
  uploadingRef.current=true;setUploading(true)
  try{
   const binding=await resolveBinding()
   for(const file of files){const attachment=await post<Attachment>('/api/attachments',{name:file.name||'붙여넣은 이미지',mimeType:file.type||'application/octet-stream',data:await readFile(file),...binding});onAddAttachments([attachment],binding)}
  }catch(error){setUploadError((error as Error).message)}finally{uploadingRef.current=false;setUploading(false);if(picker.current)picker.current.value=''}
 }
 const submit=(event:FormEvent)=>{event.preventDefault();if(!uploading&&!busy&&(input.trim()||attachments.length))void onSend(event)}
 return <div className="conversation-composer-container">
  {children}
  {uploadError&&<div role="alert" className="conversation-upload-error">{uploadError}<button type="button" onClick={()=>setUploadError('')} aria-label="첨부 오류 닫기"><X size={14}/></button></div>}
  <form className={'conversation-composer '+(dragging?'is-dragging':'')} onSubmit={submit} onDragOver={event=>{event.preventDefault();setDragging(true)}} onDragLeave={()=>setDragging(false)} onDrop={event=>{event.preventDefault();setDragging(false);void upload(Array.from(event.dataTransfer.files))}}>
   <AttachmentList attachments={attachments} onRemove={onRemoveAttachment}/>
   <textarea ref={inputRef} aria-label="메시지 입력" rows={2} placeholder={placeholder} value={input} onChange={event=>{onInput(event.target.value);event.currentTarget.style.height='auto';event.currentTarget.style.height=Math.min(180,event.currentTarget.scrollHeight)+'px'}} onPaste={event=>{const files=Array.from(event.clipboardData.files);if(files.length){event.preventDefault();void upload(files)}}} onKeyDown={event=>{if(uploading&&event.key==='Enter'&&!event.shiftKey){event.preventDefault();return}if(onKeyDown){onKeyDown(event);return}if(event.key==='Enter'&&!event.shiftKey&&!event.nativeEvent.isComposing){event.preventDefault();if(!busy&&!uploading&&(input.trim()||attachments.length))void onSend()}}} disabled={busy}/>
   {controls&&<div className="conversation-response-options">{controls}</div>}
   <div className="conversation-composer-actions"><div><input ref={picker} type="file" multiple hidden onChange={event=>void upload(Array.from(event.target.files||[]))}/><button type="button" className="conversation-attach-button" onClick={()=>picker.current?.click()} disabled={busy||uploading} title="파일 첨부 · 이미지 붙여넣기도 가능">{uploading?<LoaderCircle size={17} className="animate-spin"/>:<Paperclip size={17}/>}<span>{uploading?'첨부 중':'파일 첨부'}</span></button>{actions}</div><div>{onStop&&(busy||isRunning)&&<Button size="sm" variant="ghost" onClick={()=>void onStop()}><Square size={12} fill="currentColor"/> 중지</Button>}<Button type="submit" variant="primary" size="icon" aria-label="메시지 보내기" disabled={busy||uploading||(!input.trim()&&!attachments.length)}><ArrowUp size={19}/></Button></div></div>
  </form>
  {footer&&<div className="conversation-composer-footer">{footer}</div>}
 </div>
}
