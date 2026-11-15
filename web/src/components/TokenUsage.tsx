import type {ChatMessage} from '../api'
import {t} from '../i18n'

type Usage=NonNullable<NonNullable<ChatMessage['metadata']>['usage']>

export function TokenUsage({usage,className}:{usage?:Usage;className?:string}){
 if(!usage?.totalTokens)return null
 const count=(value?:number|null)=>typeof value==='number'?value.toLocaleString():'—'
 const prompt=usage.promptTokens
 const cache=usage.cachedPromptTokens
 // Unknown cache usage is not a zero hit. Derive new input from the same
 // provider totals rather than treating a missing legacy field as zero.
 const known=!usage.estimated&&typeof prompt==='number'&&typeof cache==='number'&&cache>=0&&cache<=prompt
 const input=known?t('usage.newInput',{count:count(prompt-cache)}):t('usage.input',{count:count(prompt)})
 const cached=known?t('usage.cached',{count:count(cache)}):t('usage.cacheUnknown')
 const output=t('usage.output',{count:count(usage.completionTokens)})
 const detail=`${input} · ${cached} · ${output}`
 return <span className={className} title={detail}>
  {t(usage.estimated?'usage.tokensEstimated':'usage.tokens',{count:count(usage.totalTokens)})}
  <span> · {detail}</span>
 </span>
}
