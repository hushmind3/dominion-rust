import type {ModelOption} from '../api'
import {SelectInput} from './Primitives'
import {useLocale} from '../i18n'

const fallback = ['none','minimal','low','medium','high','xhigh','max','ultra']
export function reasoningOptions(model?:ModelOption, requireCatalog=false):string[] {
 return [...new Set(model?.reasoningEfforts ?? (requireCatalog ? [] : fallback))]
}
export function ReasoningSelect({value,onChange,model,requireCatalog=false,disabled=false,className='',label,defaultLabel}:{
 value:string;onChange:(value:string)=>void;model?:ModelOption;requireCatalog?:boolean;disabled?:boolean;className?:string;label?:string;defaultLabel?:string
}) {
 const en=useLocale()==='en'
 const labels:Record<string,string>=en?{none:'Off',minimal:'Minimal',low:'Low',medium:'Medium',high:'High',xhigh:'Extra high',max:'Maximum',ultra:'Ultra'}:{none:'끄기',minimal:'최소',low:'낮음',medium:'보통',high:'높음',xhigh:'매우 높음',max:'최대',ultra:'울트라'}
 const options=reasoningOptions(model,requireCatalog)
 return <SelectInput aria-label={label||(en?'Reasoning effort':'추론 강도')} value={value} onChange={e=>onChange(e.target.value)} disabled={disabled} className={className}>
  <option value="">{defaultLabel||(en?'Use default':'기본값 사용')}{model?.defaultReasoningEffort?` (${labels[model.defaultReasoningEffort]||model.defaultReasoningEffort})`:''}</option>
  {value&&!options.includes(value)&&<option value={value} disabled>{labels[value]||value} · {en?'unavailable for this model':'현재 모델 지원 미확인'}</option>}
  {options.map(effort=><option key={effort} value={effort}>{labels[effort]||effort}</option>)}
 </SelectInput>
}
