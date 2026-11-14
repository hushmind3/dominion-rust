import { Plus, Trash2, ShieldCheck } from "lucide-react";
import { Button, TextInput, SelectInput } from "../../components/Primitives";

export type VerificationCheck={
  type:"file_exists"|"file_contains"|"file_equals"|"tool_succeeded"|"command_succeeded";
  path?:string;contains?:string;content?:string;tool?:string;command?:string;
};

const blank=():VerificationCheck=>({type:"file_exists",path:""});

export function StepVerification({
  checks,onChange
}:{
  checks:VerificationCheck[];
  onChange:(checks:VerificationCheck[])=>void;
}){
  const update=(index:number,patch:Partial<VerificationCheck>)=>
    onChange(checks.map((check,i)=>i===index?{...check,...patch}:check));
  return <section className="space-y-2 rounded-xl border border-[#DAD7CB] bg-white p-3" aria-label="단계별 결과 검증">
    <div className="flex items-center justify-between gap-2">
      <div className="flex items-center gap-2"><ShieldCheck size={16}/><strong className="text-[12px]">산출물 검증 조건</strong></div>
      <Button variant="ghost" disabled={checks.length>=8}
        onClick={()=>onChange([...checks,blank()])}><Plus size={14}/> 검증 추가</Button>
    </div>
    <p className="text-[11px] text-[#8C7F6B]">검증 조건이 있는 단계는 실제 파일·도구 증거가 통과해야 다음 에이전트에게 결과를 넘깁니다. 없으면 기존 대화형 단계로 실행됩니다.</p>
    {checks.map((check,i)=><div className="flex flex-wrap items-center gap-2 rounded-lg bg-[#FAF7F1] p-2" key={i}>
      <SelectInput value={check.type} onChange={e=>{
        const type=e.target.value as VerificationCheck["type"];
        onChange(checks.map((v,j)=>i===j?(
          type==="tool_succeeded"?{type,tool:""}:
          type==="command_succeeded"?{type,command:""}:
          type==="file_equals"?{type,path:"",content:""}:
          type==="file_contains"?{type,path:"",contains:""}:{type,path:""}
        ):v))
      }}>
        <option value="file_exists">파일 존재</option>
        <option value="file_contains">파일에 내용 포함</option>
        <option value="file_equals">파일 내용 정확히 일치</option>
        <option value="tool_succeeded">도구 성공</option>
        <option value="command_succeeded">명령 검증 성공</option>
      </SelectInput>
      {check.type==="tool_succeeded"?
        <TextInput aria-label="도구 이름" placeholder="workspace_write" value={check.tool||""} onChange={e=>update(i,{tool:e.target.value})}/>:
      check.type==="command_succeeded"?
        <TextInput aria-label="검증 명령" placeholder="cargo test" value={check.command||""} onChange={e=>update(i,{command:e.target.value})}/>:
        <TextInput aria-label="산출물 경로" placeholder="reports/output.md" value={check.path||""} onChange={e=>update(i,{path:e.target.value})}/>
      }
      {check.type==="file_contains"&&<TextInput aria-label="포함할 내용" placeholder="필수 문구" value={check.contains||""} onChange={e=>update(i,{contains:e.target.value})}/>}
      {check.type==="file_equals"&&<TextInput aria-label="정확한 파일 내용" placeholder="정확한 내용" value={check.content||""} onChange={e=>update(i,{content:e.target.value})}/>}
      <Button size="icon" variant="ghost" title="검증 제거" onClick={()=>onChange(checks.filter((_,j)=>j!==i))}><Trash2 size={14}/></Button>
    </div>)}
  </section>
}

export function validStepChecks(checks:VerificationCheck[]|undefined):boolean {
  return (checks||[]).length<=8&&(checks||[]).every(check=>{
    switch(check.type){
      case "file_exists":return Boolean(check.path?.trim());
      case "file_contains":return Boolean(check.path?.trim()&&check.contains?.trim());
      case "file_equals":return Boolean(check.path?.trim()&&check.content?.trim());
      case "tool_succeeded":return Boolean(check.tool?.trim());
      case "command_succeeded":return Boolean(check.command?.trim());
    }
    return false;
  });
}
