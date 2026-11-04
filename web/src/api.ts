export type RecordItem = {id:string;name?:string;createdAt?:string;updatedAt?:string;[key:string]:any}
export type Provider = RecordItem & {name:string;baseUrl:string;kind:string;enabled:boolean;hasKey?:boolean}
export type Agent = RecordItem & {name:string;description?:string;systemPrompt:string;providerId:string;model:string;color?:string;aliases?:string[]}
export type ChatMessage = {id:string;session_id:string;role:string;content:string;reasoning?:string;agent_id?:string;tool_calls?:any[];tool_call_id?:string;created_at:string}
export type ChatSession = RecordItem & {title:string;agentId:string;providerId?:string;model?:string;roomId?:string}
export type Room = RecordItem & {name:string;agentIds:string[];rounds:number;maxTurns?:number;sessionId?:string}
export type StreamEvent = {type:string;[key:string]:any}
let authToken='';
export function setAuthToken(value:string){authToken=value.trim()}
export async function request<T=any>(url:string,init:RequestInit={}):Promise<T>{
 const res=await fetch(url,{...init,headers:{...(init.body?{'Content-Type':'application/json'}:{}),...(authToken?{'X-Dominion-Token':authToken}:{}),...init.headers},cache:'no-store'});
 let data:any;try{data=await res.json()}catch{throw Error('Server response is not JSON ('+res.status+')')}
 if(!res.ok)throw Error(data.error||'HTTP '+res.status);
 return data as T;
}
export const get=<T=any>(url:string)=>request<T>(url)
export const post=<T=any>(url:string,data:any)=>request<T>(url,{method:'POST',body:JSON.stringify(data)})
export const del=(url:string)=>request(url,{method:'DELETE'})
export async function streamChat(data:any,onEvent:(e:StreamEvent)=>void){
 const res=await fetch('/api/chat',{method:'POST',headers:{'Content-Type':'application/json',...(authToken?{'X-Dominion-Token':authToken}:{})},body:JSON.stringify(data)});
 if(!res.ok){let x:any={};try{x=await res.json()}catch{}throw Error(x.error||'HTTP '+res.status)}
 if(!res.body)throw Error('No streaming response');
 const reader=res.body.getReader(), decoder=new TextDecoder();let buf='';
 while(true){
  const {done,value}=await reader.read();if(done)break;
  buf+=decoder.decode(value,{stream:true});
  const parts=buf.split('\n\n');buf=parts.pop()||'';
  for(const part of parts)for(const line of part.split('\n'))if(line.startsWith('data: ')){
    try{onEvent(JSON.parse(line.slice(6)))}catch{/* ignore incomplete event */}
  }
 }
}
