import {useCallback,useLayoutEffect,useRef,useState,type Dispatch,type SetStateAction} from 'react'

// A draft belongs to its exact chat session, never to the globally mounted
// composer. Switching sessions must neither leak the old owner's text into
// another conversation nor discard unsent work.
const scopeKey=(id?:string)=>id?'session:'+id:'new-chat'
export function useScopedChatDraft(sessionId?:string){
 const cache=useRef(new Map<string,string>())
 const scope=useRef(scopeKey(sessionId))
 const latest=useRef('')
 const [value,setValue]=useState('')
 useLayoutEffect(()=>{
  const next=scopeKey(sessionId)
  if(scope.current===next)return
  scope.current=next
  const restored=cache.current.get(next)??''
  latest.current=restored
  setValue(restored)
 },[sessionId])
 const setDraft:Dispatch<SetStateAction<string>>=useCallback(next=>{
  const updated=typeof next==='function'?next(latest.current):next
  latest.current=updated
  cache.current.set(scope.current,updated)
  setValue(updated)
 },[])
 const carryToNew=useCallback(()=>{
  cache.current.set(scopeKey(undefined),latest.current)
 },[])
 const clearNew=useCallback(()=>{
  cache.current.set(scopeKey(undefined),'')
 },[])
 return {input:value,setInput:setDraft,carryToNew,clearNew}
}
