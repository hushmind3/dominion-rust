import {useLayoutEffect, useRef} from 'react'

export function useTranscriptScroll(scope: string | undefined, revision: string, onMore?: (more: boolean) => void) {
 const scrollRef = useRef<HTMLDivElement>(null)
 const following = useRef(true)

 useLayoutEffect(() => { following.current = true }, [scope])
 useLayoutEffect(() => {
  const el = scrollRef.current
  if (!el) return
  if (following.current) el.scrollTop = el.scrollHeight
  onMore?.(!following.current)
 }, [scope, revision, onMore])

 useLayoutEffect(() => {
  const el = scrollRef.current
  if (!el) return
  const observer = new ResizeObserver(() => {
   if (following.current) el.scrollTop = el.scrollHeight
  })
  observer.observe(el)
  return () => observer.disconnect()
 }, [])

 function onScroll() {
  const el = scrollRef.current
  if (!el) return
  following.current = el.scrollHeight - el.scrollTop - el.clientHeight < 100
  if (following.current) onMore?.(false)
 }

 function jumpDown() {
  following.current = true
  const el = scrollRef.current
  if (el) el.scrollTop = el.scrollHeight
  onMore?.(false)
 }

 return {scrollRef, onScroll, jumpDown}
}
