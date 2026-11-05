import {useEffect,useState,type ReactNode} from 'react'
import {createPortal} from 'react-dom'

/** Page controls occupy the single workspace toolbar instead of another row. */
export function PageToolbar({children}:{children:ReactNode}){
 const [target,setTarget]=useState<HTMLElement|null>(null)
 useEffect(()=>{setTarget(document.getElementById('dominion-page-toolbar'))},[])
 return target?createPortal(<div className="page-toolbar-content">{children}</div>,target):null
}
