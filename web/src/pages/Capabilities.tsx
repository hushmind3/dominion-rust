import {ExtensionsPage} from './Extensions'

/** Existing bookmarks use the same inventory and search as the tool hub. */
export function CapabilitiesPage(_props:{navigate:(page:'models'|'connections'|'extensions'|'memory')=>void}){
 return <ExtensionsPage/>
}
