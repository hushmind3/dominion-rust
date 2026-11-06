import {useLayoutEffect, useRef, useState, type CSSProperties} from 'react'

export function useAnchoredPanel(open: boolean, close: () => void, width: number, preferred: 'top' | 'bottom' = 'bottom', matchTrigger = false) {
 const trigger = useRef<HTMLButtonElement>(null)
 const panel = useRef<HTMLDivElement>(null)
 const onClose = useRef(close)
 const [position, setPosition] = useState<CSSProperties>()
 onClose.current = close

 useLayoutEffect(() => {
  if (!open) { setPosition(undefined); return }
  const viewport = window.visualViewport
  const place = () => {
   const rect = trigger.current?.getBoundingClientRect()
   if (!rect) return
   const x = viewport?.offsetLeft ?? 0
   const y = viewport?.offsetTop ?? 0
   const viewportWidth = viewport?.width ?? window.innerWidth
   const viewportHeight = viewport?.height ?? window.innerHeight
   const margin = 12
   const gap = 4
   const above = Math.max(0, rect.top - y - margin - gap)
   const below = Math.max(0, y + viewportHeight - rect.bottom - margin - gap)
   const up = preferred === 'top' ? above >= Math.min(320, below) : below < Math.min(320, above)
   const actualWidth = Math.max(0, Math.min(matchTrigger ? Math.max(width, rect.width) : width, viewportWidth - margin * 2))
   setPosition({
    position: 'fixed',
    left: Math.max(x + margin, Math.min(rect.left, x + viewportWidth - actualWidth - margin)),
    width: actualWidth,
    maxHeight: up ? above : below,
    ...(up ? {bottom: window.innerHeight - rect.top + gap} : {top: rect.bottom + gap}),
   })
  }
  const onScroll = (event: Event) => {
   if (!(event.target instanceof Node) || !panel.current?.contains(event.target)) place()
  }
  const outside = (event: PointerEvent) => {
   if (!event.composedPath().includes(panel.current!) && !event.composedPath().includes(trigger.current!)) onClose.current()
  }
  const escape = (event: KeyboardEvent) => {
   if (event.key !== 'Escape' || event.isComposing) return
   event.stopPropagation()
   onClose.current()
   trigger.current?.focus({preventScroll:true})
  }
  place()
  document.addEventListener('pointerdown', outside)
  document.addEventListener('keydown', escape, true)
  window.addEventListener('resize', place)
  window.addEventListener('scroll', onScroll, true)
  viewport?.addEventListener('resize', place)
  viewport?.addEventListener('scroll', place)
  return () => {
   document.removeEventListener('pointerdown', outside)
   document.removeEventListener('keydown', escape, true)
   window.removeEventListener('resize', place)
   window.removeEventListener('scroll', onScroll, true)
   viewport?.removeEventListener('resize', place)
   viewport?.removeEventListener('scroll', place)
  }
 }, [open, width, preferred, matchTrigger])

 return {trigger, panel, position}
}
