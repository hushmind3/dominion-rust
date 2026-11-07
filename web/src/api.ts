import {t} from './i18n'
import {translateError} from './i18n/errors'

export type RecordItem = {id:string;name?:string;createdAt?:string;updatedAt?:string;[key:string]:any}
export type Provider = RecordItem & {name:string;baseUrl:string;kind:string;enabled:boolean;hasKey?:boolean;model?:string;learning?:boolean;reflection?:boolean;reflectionModel?:string;accountId?:string}
export type Agent = RecordItem & {name:string;description?:string;systemPrompt:string;providerId:string;model:string;color?:string;aliases?:string[];avatarUrl?:string;personaSource?:string;sourceProfile?:string;role?:string}
export type ChatMessage = {id:string;session_id:string;role:string;content:string;reasoning?:string;agent_id?:string;tool_calls?:any[];tool_call_id?:string;metadata?:{authorKind?:string;roomId?:string;replyToMessageId?:string;executionId?:string;usage?:{promptTokens?:number;completionTokens?:number;totalTokens?:number;estimated?:boolean;model?:string;providerId?:string};[key:string]:unknown};created_at:string}
export type ChatSession = RecordItem & {title:string;agentId:string;providerId?:string;model?:string;roomId?:string}
export type Room = RecordItem & {name:string;agentIds:string[];rounds:number;maxTurns?:number;sessionId?:string}
export type StreamEvent = {type:string;[key:string]:any}
export const koreanReasoningKey='dominion.chat.reasoningKo'
export function koreanReasoningEnabled(){
 try{return localStorage.getItem(koreanReasoningKey)!=='0'}catch{return true}
}
let authToken = ''
export function setAuthToken(value: string) { authToken = value.trim() }

function apiFetch(url: string, init: RequestInit = {}) {
 const headers = new Headers(init.headers)
 if (typeof init.body === 'string' && !headers.has('Content-Type')) {
  headers.set('Content-Type', 'application/json')
 }
 if (authToken) headers.set('X-Dominion-Token', authToken)
 return fetch(url, {...init, headers, cache: 'no-store'})
}

export class ApiError extends Error {
 constructor(readonly status: number, message: string) { super(message) }
}

async function readJson(res: Response) {
 const content = await res.text()
 try { return JSON.parse(content) }
 catch {
  throw new ApiError(res.status, !res.ok && content && !content.trimStart().startsWith('<')
   ? content : 'Server response is not JSON (' + res.status + ')')
 }
}

export async function request<T = any>(url: string, init?: RequestInit): Promise<T> {
 const res = await apiFetch(url, init)
 const data = await readJson(res)
 if (!res.ok) throw new ApiError(res.status, translateError(data?.error || 'HTTP ' + res.status))
 return data as T
}

export const get = <T = any>(url: string, signal?: AbortSignal) => request<T>(url, {signal})
export const post = <T = any>(url: string, data: unknown) =>
 request<T>(url, {method: 'POST', body: JSON.stringify(data)})
export const del = (url: string) => request(url, {method: 'DELETE'})

export type ModelOption = {id: string; name?: string; free?: boolean}
export type Approval = {id: string; tool: string; args: unknown; status: string; sessionId: string}
export async function resolveApproval(id: string, allow: boolean) {
 const result = await post<{resolved: boolean; error?: string}>('/api/approvals', {id, allow})
 if (!result.resolved) throw Error(result.error ? translateError(result.error) : t('chat.approvalGone'))
}

export async function openOAuth(id: string) {
 const path = '/api/oauth/' + encodeURIComponent(id)
 if ('__DOMINION_NATIVE__' in window) {
  await post(path + '/open', {})
  return
 }
 const popup = window.open('', 'dominion_oauth', 'width=650,height=780')
 if (!popup) throw Error(t('oauth.popupBlocked'))
 try {
  const {url} = await get<{url: string}>(path + '/start')
  popup.location.replace(url)
 } catch (error) {
  popup.close()
  throw error
 }
}

export type ChatRequest = {
 sessionId: string; agentId: string; providerId?: string; model?: string; input: string;
 toolsEnabled: boolean; toolsExecutionEnabled: boolean; autoLearning: boolean;
 reasoningLanguage?: string; reasoningEffort?: string; thinking: boolean;
}

export async function streamChat(data: ChatRequest, onEvent: (e: StreamEvent) => void, signal: AbortSignal) {
 const res = await apiFetch('/api/chat', {method: 'POST', body: JSON.stringify(data), signal})
 if (!res.ok) throw new ApiError(res.status, translateError((await readJson(res))?.error || 'HTTP ' + res.status))
 if (!res.body || !res.headers.get('Content-Type')?.startsWith('text/event-stream')) {
  throw Error('No streaming response')
 }
 const reader = res.body.pipeThrough(new TextDecoderStream()).getReader()
 let buffer = ''
 try {
  while (true) {
   const {done, value} = await reader.read()
   if (done) throw Error(t('chat.streamInterrupted'))
   buffer += value
   let boundary: RegExpExecArray | null
   while ((boundary = /\r?\n\r?\n/.exec(buffer))) {
    const frame = buffer.slice(0, boundary.index)
    buffer = buffer.slice(boundary.index + boundary[0].length)
    const payload = frame.split(/\r?\n/).filter(line => line.startsWith('data:'))
     .map(line => line.slice(5).replace(/^ /, '')).join('\n')
    if (!payload) continue
    const event: StreamEvent = JSON.parse(payload)
    onEvent(event)
    if (event.type === 'done') return
   }
  }
 } finally {
  await reader.cancel().catch(() => {})
 }
}
