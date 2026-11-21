import type {ReactNode} from 'react'

/**
 * Readable, safe rendering for the chat's most common Markdown structures.
 * No HTML injection, no remote dependency, and no changes to the streamed
 * or persisted message text. Partial streaming fences stay legible.
 */
const TICK=String.fromCharCode(96)
const inlinePattern=/(\x60[^\x60\n]+\x60|\*\*[^*\n]+\*\*|\*[^*\n]+\*|\[[^\]\n]+\]\(https?:\/\/[^()\s]+\))/g

function inline(source:string):ReactNode[] {
 const output:ReactNode[]=[]
 let index=0
 for(const match of source.matchAll(inlinePattern)){
  const start=match.index??0
  if(start>index)output.push(source.slice(index,start))
  const item=match[0],key=start
  if(item.startsWith(TICK)){
   output.push(<code key={key} className="rounded-md bg-[#F3EEE5] px-1.5 py-0.5 font-mono text-[.88em] text-[#A34D30]">{item.slice(1,-1)}</code>)
  }else if(item.startsWith('**')){
   output.push(<strong key={key} className="font-semibold">{item.slice(2,-2)}</strong>)
  }else if(item.startsWith('*')){
   output.push(<em key={key}>{item.slice(1,-1)}</em>)
  }else{
   const label=item.slice(1,item.indexOf(']'))
   const url=item.slice(item.indexOf('](')+2,-1)
   output.push(<a key={key} href={url} target="_blank" rel="noopener noreferrer" className="font-medium text-[#A94E30] underline underline-offset-2 hover:text-[#813A24]">{label}</a>)
  }
  index=start+item.length
 }
 if(index<source.length)output.push(source.slice(index))
 return output
}

function splitTableCells(line:string){
 return line.trim().replace(/^\|/,'').replace(/\|$/,'').split('|').map(value=>value.trim())
}
function isTableDivider(line:string){
 const cells=splitTableCells(line)
 return cells.length>1&&cells.every(cell=>/^:?-{3,}:?$/.test(cell))
}
function isBlockStart(line:string){
 const value=line.trim()
 return value.startsWith(TICK.repeat(3))||/^#{1,4}\s/.test(value)
  ||/^\s*[-*+]\s/.test(line)||/^\s*\d+[.)]\s/.test(line)
  ||/^\s*>\s?/.test(line)||/^(?:-{3,}|\*{3,})$/.test(value)
}

export function ChatAnswer({content}:{content:string}){
 const lines=content.replace(/\r\n?/g,'\n').split('\n')
 const blocks:ReactNode[]=[]
 let at=0
 while(at<lines.length){
  const line=lines[at],trimmed=line.trim()
  if(!trimmed){at++;continue}
  const key=at
  if(trimmed.startsWith(TICK.repeat(3))){
   const language=trimmed.slice(3).trim()
   at++
   const code:string[]=[]
   while(at<lines.length&&!lines[at].trim().startsWith(TICK.repeat(3))){
    code.push(lines[at]);at++
   }
   if(at<lines.length)at++
   blocks.push(<div key={key} className="my-3 min-w-0 overflow-hidden rounded-xl border border-[#E5E2DE] bg-[#24252A]">
    <div className="flex items-center justify-between border-b border-white/10 px-4 py-2 text-xs text-[#CACDD1]">
     <span>{language||'Code'}</span>
     <button type="button" className="rounded px-2 py-1 text-xs hover:bg-white/10" onClick={()=>void navigator.clipboard.writeText(code.join('\n'))}>Copy</button>
    </div>
    <pre className="overflow-x-auto px-4 py-3 text-[13px] leading-6 text-[#F5F5F5]"><code>{code.join('\n')}</code></pre>
   </div>)
   continue
  }
  const heading=trimmed.match(/^(#{1,4})\s+(.+)$/)
  if(heading){
   const sizes=['text-[20px]','text-[18px]','text-[16px]','text-[15px]']
   blocks.push(<h3 key={key} className={'mt-5 mb-2 font-semibold leading-snug text-[#24211D] '+sizes[heading[1].length-1]}>{inline(heading[2])}</h3>)
   at++;continue
  }
  if(/^(?:-{3,}|\*{3,})$/.test(trimmed)){
   blocks.push(<hr key={key} className="my-4 border-[#E7E0D5]"/>)
   at++;continue
  }
  if(at+1<lines.length&&line.includes('|')&&isTableDivider(lines[at+1])){
   const columns=splitTableCells(line)
   at+=2
   const rows:string[][]=[]
   while(at<lines.length&&lines[at].trim()&&lines[at].includes('|')){
    rows.push(splitTableCells(lines[at]));at++
   }
   blocks.push(<div key={key} className="my-3 w-full overflow-x-auto rounded-xl border border-[#E7E0D5]">
    <table className="w-full min-w-max border-collapse text-left text-[13px]">
     <thead className="bg-[#F5F1EA]"><tr>{columns.map((c,i)=><th key={i} className="border-b border-[#E7E0D5] px-3 py-2 font-semibold">{inline(c)}</th>)}</tr></thead>
     <tbody>{rows.map((row,i)=><tr key={i} className="border-b border-[#EEE8DF] last:border-0">{columns.map((_,j)=><td key={j} className="px-3 py-2 align-top">{inline(row[j]||'')}</td>)}</tr>)}</tbody>
    </table>
   </div>)
   continue
  }
  const bullet=line.match(/^\s*([-*+])\s+(.+)$/)
  const numbered=line.match(/^\s*(\d+)[.)]\s+(.+)$/)
  if(bullet||numbered){
   const ordered=Boolean(numbered),entries:string[]=[]
   const initial=numbered?Number(numbered[1]):1
   while(at<lines.length){
    const hit=ordered?lines[at].match(/^\s*\d+[.)]\s+(.+)$/):lines[at].match(/^\s*[-*+]\s+(.+)$/)
    if(!hit)break
    entries.push(hit[1]);at++
   }
   const children=entries.map((entry,i)=><li key={i} className="pl-1">{inline(entry)}</li>)
   blocks.push(ordered?
    <ol key={key} start={initial} className="my-2 list-decimal space-y-1 pl-6 marker:text-[#A94E30]">{children}</ol>:
    <ul key={key} className="my-2 list-disc space-y-1 pl-6 marker:text-[#A94E30]">{children}</ul>)
   continue
  }
  if(/^\s*>\s?/.test(line)){
   const quotes:string[]=[]
   while(at<lines.length&&/^\s*>\s?/.test(lines[at])){
    quotes.push(lines[at].replace(/^\s*>\s?/,''));at++
   }
   blocks.push(<blockquote key={key} className="my-3 border-l-[3px] border-[#D7B1A1] pl-4 text-[#655A50] whitespace-pre-line">{quotes.map((quote,i)=><span key={i}>{i>0?'\n':''}{inline(quote)}</span>)}</blockquote>)
   continue
  }
  const paragraph:string[]=[line];at++
  while(at<lines.length&&lines[at].trim()&&!isBlockStart(lines[at])
      &&!(at+1<lines.length&&lines[at].includes('|')&&isTableDivider(lines[at+1]))){
   paragraph.push(lines[at]);at++
  }
  blocks.push(<p key={key} className="my-2 whitespace-pre-line break-words">{paragraph.map((part,i)=><span key={i}>{i>0?'\n':''}{inline(part)}</span>)}</p>)
 }
 return <div className="chat-answer min-w-0 text-[15.5px] leading-[1.8] text-[#302C28] sm:text-[16px]">{blocks}</div>
}
