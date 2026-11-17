import {useExtensionsController,type ExtensionTab} from './extensions/ExtensionsController';
import {ExtensionsView} from './extensions/ExtensionsView';
export type {ExtensionTab} from './extensions/ExtensionsController';

/** Compatibility page entry point: the main Connections hub embeds these tabs. */
export function ExtensionsPage({section,embedded=false}:{section?:ExtensionTab;embedded?:boolean}={}){
 const state=useExtensionsController({section,embedded});
 return <ExtensionsView {...state}/>;
}
