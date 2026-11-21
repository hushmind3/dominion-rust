// Pure preflight for the Rust TeamGraph::new DAG contract. The server remains authoritative.
export type TeamDraftNode={
 id:string;depends:string;instruction:string;workspace:string;budget:string;steps:string
}
export type TeamDraftIssue={
 code:'task_count'|'invalid_total_budget'|'invalid_id'|'duplicate_id'|
  'invalid_instruction'|'invalid_workspace'|'invalid_steps'|'invalid_budget'|
  'budget_exceeded'|'self_dependency'|'unknown_dependency'|'duplicate_dependency'|'cycle';
 taskId?:string;dependency?:string
}
export type TeamDraftAnalysis={issue:TeamDraftIssue|null;waves:string[][];usedBudget:number}
export function analyzeTeamDraft(nodes:readonly TeamDraftNode[],totalText:string):TeamDraftAnalysis{
 let usedBudget=0
 const fail=(code:TeamDraftIssue['code'],taskId?:string,dependency?:string):TeamDraftAnalysis=>
  ({issue:{code,taskId,dependency},waves:[],usedBudget})
 if(nodes.length<1||nodes.length>16)return fail('task_count')
 if(!totalText.trim())return fail('invalid_total_budget')
 const total=Number(totalText)
 if(!Number.isFinite(total)||total<=0||total>1_000_000)return fail('invalid_total_budget')
 const names=new Set<string>(),order:string[]=[],deps=new Map<string,string[]>()
 const bytes=(value:string)=>new TextEncoder().encode(value).length
 for(const task of nodes){
  const id=task.id.trim()
  if(!/^[A-Za-z0-9_-]{1,64}$/.test(id))return fail('invalid_id',id)
  if(names.has(id))return fail('duplicate_id',id)
  names.add(id);order.push(id)
  if(!task.instruction.trim()||bytes(task.instruction)>20_000)return fail('invalid_instruction',id)
  const workspace=task.workspace.trim()||'project'
  if(bytes(workspace)>160||workspace.includes('..')||workspace.includes('\0'))
   return fail('invalid_workspace',id)
  const stepCount=Number(task.steps)
  if(!task.steps.trim()||!Number.isInteger(stepCount)||stepCount<1||stepCount>100)
   return fail('invalid_steps',id)
  const allocation=Number(task.budget)
  if(!task.budget.trim()||!Number.isFinite(allocation)||allocation<0)
   return fail('invalid_budget',id)
  usedBudget+=allocation
  if(!Number.isFinite(usedBudget))return fail('invalid_budget',id)
  deps.set(id,task.depends.split(',').map(value=>value.trim()).filter(Boolean))
 }
 if(usedBudget>total+0.000001)return fail('budget_exceeded')
 for(const [id,dependencies] of deps){
  const seen=new Set<string>()
  for(const dep of dependencies){
   if(dep===id)return fail('self_dependency',id,dep)
   if(!names.has(dep))return fail('unknown_dependency',id,dep)
   if(seen.has(dep))return fail('duplicate_dependency',id,dep)
   seen.add(dep)
  }
 }
 // Topological levels are a dependency preview, not a promise of parallel scheduling.
 const remaining=new Set(order),waves:string[][]=[]
 while(remaining.size){
  const wave=order.filter(id=>remaining.has(id)&&deps.get(id)!.every(dep=>!remaining.has(dep)))
  if(!wave.length)return fail('cycle',order.find(id=>remaining.has(id)))
  waves.push(wave)
  for(const id of wave)remaining.delete(id)
 }
 return {issue:null,waves,usedBudget}
}
