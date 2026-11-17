import {createPortal} from 'react-dom'
import {BrainCircuit,SlidersHorizontal} from 'lucide-react'
import {Button,TextInput} from './Primitives'
import {useAnchoredPanel} from '../hooks/useAnchoredPanel'
import {koreanReasoningKey} from '../api'
import {t,useLocale} from '../i18n'

type Props={
 open:boolean;onOpenChange:(open:boolean)=>void
 showReasoning:boolean;setShowReasoning:(value:boolean)=>void
 reasoningKo:boolean;setReasoningKo:(value:boolean)=>void
 thinkingEnabled:boolean;setThinkingEnabled:(value:boolean)=>void;canDisableThinking?:boolean
 reasoningEffort:string;setReasoningEffort:(value:string)=>void
 toolsEnabled:boolean;setToolsEnabled:(value:boolean)=>void
 autoLearn:boolean;setAutoLearn:(value:boolean)=>void
 learning:boolean;learnDisabled:boolean;onLearn:()=>void|Promise<void>
 optionsBusy?:boolean;notice?:string
}
/** Shared chat controls, with each conversation providing its own execution callbacks. */
export function ConversationSettings({open,onOpenChange,showReasoning,setShowReasoning,reasoningKo,setReasoningKo,thinkingEnabled,setThinkingEnabled,canDisableThinking=true,reasoningEffort,setReasoningEffort,toolsEnabled,setToolsEnabled,autoLearn,setAutoLearn,learning,learnDisabled,onLearn,optionsBusy=false,notice}:Props){
 useLocale()
 const settingsPanel=useAnchoredPanel(open,()=>onOpenChange(false),250,'top')
 return <div className="relative">
        <Button type="button" variant="ghost" ref={settingsPanel.trigger} onClick={()=>onOpenChange(!open)} className="inline-flex !h-7 items-center gap-1.5 !rounded-md !border-0 !px-2 !py-0 text-[11px] font-medium text-[#7C7469] hover:bg-[#F3EEE5]" title={t('chat.settings')} aria-expanded={open} aria-haspopup="dialog"><SlidersHorizontal size={13}/><span className="hidden sm:inline">{t('chat.settings')}</span></Button>
        {open&&settingsPanel.position&&createPortal(<div ref={settingsPanel.panel} style={settingsPanel.position} role="dialog" aria-label={t('chat.settings')} className="z-[80] max-w-full overflow-y-auto overscroll-contain rounded-xl border border-[#E7E0D5] bg-white p-2 shadow-xl"><label className="flex cursor-pointer items-center justify-between gap-2 rounded-lg px-2 py-2 text-[11px] text-[#5C544A] hover:bg-[#FAF7F1]"><span className="inline-flex items-center gap-1.5"><BrainCircuit size={13}/> {t('chat.showReasoning')}</span><TextInput type="checkbox" checked={showReasoning} onChange={e=>setShowReasoning(e.target.checked)}/></label><label className="flex cursor-pointer items-center justify-between gap-2 rounded-lg px-2 py-2 text-[11px] text-[#5C544A] hover:bg-[#FAF7F1]"><span>{t('chat.koreanReasoning')}</span><TextInput type="checkbox" checked={reasoningKo} onChange={e=>{const enabled=e.target.checked;setReasoningKo(enabled);try{localStorage.setItem(koreanReasoningKey,enabled?'1':'0')}catch{}}}/></label><label className="flex cursor-pointer items-center justify-between gap-2 rounded-lg px-2 py-2 text-[11px] text-[#5C544A] hover:bg-[#FAF7F1]"><span>{t('chat.useThinking')}</span><TextInput type="checkbox" checked={canDisableThinking?thinkingEnabled:true} disabled={!canDisableThinking} title={!canDisableThinking?'이 모델은 추론 끄기를 지원하지 않습니다. 낮은 강도를 선택하세요.':undefined} onChange={e=>{setThinkingEnabled(e.target.checked);if(e.target.checked&&reasoningEffort==='none')setReasoningEffort('')}}/></label><label className="flex cursor-pointer items-center justify-between gap-2 rounded-lg px-2 py-2 text-[11px] text-[#5C544A] hover:bg-[#FAF7F1]"><span>{t('chat.useTools')}</span><TextInput type="checkbox" checked={toolsEnabled} disabled={optionsBusy} onChange={e=>setToolsEnabled(e.target.checked)}/></label><label className="flex cursor-pointer items-center justify-between gap-2 rounded-lg px-2 py-2 text-[11px] text-[#5C544A] hover:bg-[#FAF7F1]"><span>{t('chat.autoLearn')} <small className="text-[10px] text-[#9C9388]">{t('chat.autoLearnDesc')}</small></span><TextInput type="checkbox" checked={autoLearn} disabled={optionsBusy} onChange={e=>setAutoLearn(e.target.checked)}/></label><Button type="button" variant="ghost" disabled={learnDisabled||learning} onClick={()=>void onLearn()} className="mt-1 flex w-full !justify-start gap-2 !border-0 rounded-lg px-2 py-2 text-[11px] text-[#5C544A] hover:bg-[#FAF7F1] disabled:opacity-40"><BrainCircuit size={13}/>{learning?t('chat.learning'):t('chat.learnNow')}</Button>{notice&&<p role="status" className="px-2 py-1 text-[11px] text-[#7C7469]">{notice}</p>}</div>,document.body)}
 </div>
}
