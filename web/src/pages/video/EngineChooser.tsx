import {Button} from '../../components/Primitives'

export type VideoEngineChoice='native'|'comfyui'|'runway'
type Option={id:VideoEngineChoice;name:string;description:string;ready:boolean}
export function EngineChooser({engine,nativeReady,comfyReady,paidReady,busy,onSelect,onSettings}:{
 engine:VideoEngineChoice
 nativeReady:boolean
 comfyReady:boolean
 paidReady:boolean
 busy:boolean
 onSelect:(engine:VideoEngineChoice)=>void
 onSettings:()=>void
}){
 const options:Option[]=[
  {id:'native',name:'간단한 자막 영상',description:'글과 자막으로 MP4 제작 · FFmpeg 사용',ready:nativeReady},
  {id:'comfyui',name:'내 컴퓨터 AI 영상',description:'ComfyUI와 GPU로 장면 생성',ready:comfyReady},
  {id:'runway',name:'영화 같은 AI 영상',description:'Kling 시네마틱 · 유료 견적 확인 필수',ready:paidReady},
 ]
 const selected=options.find(option=>option.id===engine)
 return <section aria-label="영상 제작 방식 선택" className="mb-4 rounded-xl border border-[#E7E0D5] bg-[#FFFDF9] p-4">
  <h2 className="text-[15px] font-bold text-[#302821]">어떤 영상을 만들까요?</h2>
  <p className="mb-3 mt-1 text-[12px] text-[#7C7469]">원하는 방식을 고르고 만들 장면을 적어 주세요. 전문 설정은 따로 볼 수 있습니다.</p>
  <div className="grid gap-2 sm:grid-cols-3">{options.map(option=>
   <Button key={option.id} type="button" variant="bare" disabled={busy}
    aria-pressed={engine===option.id} onClick={()=>onSelect(option.id)}
    className={'flex min-h-28 flex-col items-start rounded-xl border p-3 text-left '+(engine===option.id
      ?'border-[#C96442] bg-[#FCF2EB] ring-1 ring-[#C96442]'
      :'border-[#E7E0D5] bg-white hover:border-[#C96442]')}>
    <strong className="text-[13px] text-[#322C26]">{option.name}</strong>
    <span className="mt-1 text-[11px] leading-5 text-[#73695E]">{option.description}</span>
    <span className={'mt-auto pt-2 text-[11px] font-semibold '+(option.ready?'text-emerald-700':'text-amber-700')}>{option.ready?'제작 가능':'연결 설정 필요'}</span>
   </Button>
  )}</div>
  {!selected?.ready&&<div className="mt-3 flex flex-wrap items-center justify-between gap-2 rounded-lg border border-amber-200 bg-amber-50 p-3 text-[12px] text-amber-900">
   <span>선택한 제작 방식의 엔진이나 워크플로 연결을 확인해 주세요.</span>
   <Button variant="secondary" onClick={onSettings}>연결 설정 보기</Button>
  </div>}
 </section>
}
