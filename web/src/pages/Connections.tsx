import {useState} from 'react'
import {Blocks,BookOpenText,KeyRound,RadioTower,Server,Sparkles} from 'lucide-react'
import {Button} from '../components/Primitives'
import {t,useLocale} from '../i18n'
import {ConnectedAccounts} from '../components/ConnectedAccounts'
import {ExtensionsPage,type ExtensionTab} from './Extensions'
import {CapabilitiesPage} from './Capabilities'
import {ChannelConnections} from './ChannelConnections'

type Section='channels'|'accounts'|'capabilities'|ExtensionTab

// Navigation stays exactly where it was: six existing sections, no new
// sidebar items, duplicate market pages, or extra tabs inside any section.
export function ConnectionsPage(){
 useLocale()
 const [section,setSection]=useState<Section>('channels')
 const tabs=[
  {id:'channels' as const,label:t('conn.tab.channels'),icon:RadioTower},
  {id:'accounts' as const,label:t('conn.tab.accounts'),icon:KeyRound},
  {id:'market' as const,label:t('conn.tab.mcp'),icon:Blocks},
  {id:'installed' as const,label:t('ext.tab.installed'),icon:Server},
  {id:'skills' as const,label:t('ext.tab.skills'),icon:BookOpenText},
  {id:'capabilities' as const,label:t('conn.tab.capabilities'),icon:Sparkles}
 ]
 return <div className="mx-auto w-full max-w-[1320px] px-4 py-3 sm:px-6">
  <header className="mb-4">
   <h1 className="m-0 text-[19px] font-bold text-[#24211D]">{t('conn.title')}</h1>
   <p className="mt-1 text-[12px] leading-5 text-[#7C7469]">{t('conn.description')}</p>
  </header>
  <nav className="connection-tabs" aria-label={t('conn.tabs.aria')}>
   {tabs.map(({id,label,icon:Icon})=><Button key={id} variant="bare"
    aria-current={section===id?'page':undefined}
    onClick={()=>setSection(id)}><Icon size={15}/>{label}</Button>)}
  </nav>
  {section==='channels'&&<ChannelConnections/>}
  {section==='accounts'&&<ConnectedAccounts/>}
  {section==='capabilities'&&<CapabilitiesPage embedded navigate={page=>setSection(page==='extensions'?'market':'channels')}/>}
  {(section==='market'||section==='installed'||section==='skills')&&
   <ExtensionsPage section={section} embedded/>}
 </div>
}
