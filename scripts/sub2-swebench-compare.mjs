/** Same-issue, independently graded DOMINION SWE-bench trial comparison.
 * No model claims, generated-file counts, or missing official Docker verdicts
 * are converted to solutions. This compares real task outcomes, not promises.
 */
import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

function demand(ok,message){if(!ok)throw Error(message)}
function key(row){return row.instance_id}
function checked(report){
  demand(report?.dataset==='princeton-nlp/SWE-bench_Verified',
    'comparison requires SWE-bench Verified');
  demand(report?.officialGrading?.fullyGraded===true
    &&report.officialGrading?.source==='external_SWE_bench_docker_per_instance_reports',
    'all cases must be graded with imported official Docker evidence');
  demand(Array.isArray(report.receipts)&&report.receipts.length>0,
    'trial receipts are required');
  const byId=new Map();
  for(const row of report.receipts){
    demand(typeof key(row)==='string'&&!byId.has(key(row)),
      'duplicate/missing SWE-bench instance ID');
    demand(typeof row.officialSolved==='boolean','incomplete official verdict');
    demand(row.officialGradeSource==='official_swebench_docker_report'
      ||(row.patchBytes===0 && !row.officialSolved
        &&row.officialGradeSource==='empty_prediction_not_evaluated'),
      'a native goal claim is not official grading evidence');
    demand(typeof row.repo==='string'
      &&/^[a-f0-9]{40}$/.test(row.base_commit),'repo/base commit unknown');
    byId.set(key(row),row);
  }
  if(Array.isArray(report.instanceIds)){
    demand(report.instanceIds.length===byId.size
      &&report.instanceIds.every(id=>byId.has(id)),
      'report manifest and scored instances differ');
  }
  return byId;
}
function nativeTokens(rows){
  if(!rows.every(r=>Number.isSafeInteger(r.usage?.totalTokens)
      &&r.usage.totalTokens>=0))return null;
  return rows.reduce((sum,row)=>sum+row.usage.totalTokens,0);
}
function exactTokens(rows){
  if(!rows.every(r=>r.usage?.estimatedCalls===0))return null;
  return nativeTokens(rows);
}
function executionCounts(rows){
  if(!rows.every(r=>Number.isSafeInteger(r.toolLedger?.total)
    &&r.toolLedger.total>=0))return null;
  return rows.reduce((sum,r)=>sum+r.toolLedger.total,0);
}
export function compareOfficialRuns(a,b){
  const left=checked(a),right=checked(b);
  demand(left.size===right.size
    &&[...left.keys()].every(id=>right.has(id)),
    'trial task sets must be identical: no cherry-picked score denominator');
  for(const [id,row] of left){
    const other=right.get(id);
    demand(row.repo===other.repo&&row.base_commit===other.base_commit,
      'base repo revision differs for '+id);
  }
  const taskIds=[...left.keys()].sort();
  const budgetKeys=['maxIterations','maxExecutorTokens','timeoutMinutes'];
  const sameBudgets=budgetKeys.every(k=>a.reproducibility?.[k]===
    b.reproducibility?.[k] &&Number.isFinite(a.reproducibility?.[k]));
  const sameModel=a.reproducibility?.provider?.model===
      b.reproducibility?.provider?.model
    &&a.reproducibility?.provider?.kind===
      b.reproducibility?.provider?.kind
    &&a.reproducibility?.provider?.baseUrl===
      b.reproducibility?.provider?.baseUrl;
  let leftOnly=0,rightOnly=0,both=0,neither=0;
  const paired=taskIds.map(id=>{
    const l=left.get(id),r=right.get(id);
    if(l.officialSolved && r.officialSolved)both++;
    else if(l.officialSolved)leftOnly++;
    else if(r.officialSolved)rightOnly++;
    else neither++;
    return {instance_id:id,leftSolved:l.officialSolved,rightSolved:r.officialSolved};
  });
  const leftRows=[...left.values()],rightRows=[...right.values()];
  const score=rows=>({
    solved:rows.filter(row=>row.officialSolved).length,
    resolvedRate:rows.filter(row=>row.officialSolved).length/rows.length,
    recordedTokens:nativeTokens(rows),
    independentlyCountedTokens:exactTokens(rows),
    toolCalls:executionCounts(rows),
    sha256:rows===leftRows?a.reproducibility?.binarySha256:
      b.reproducibility?.binarySha256,
  });
  return {
    dataset:a.dataset,attempted:taskIds.length,
    evidence:'independently_graded_same_issue_official_Docker',
    sameModel,sameBudgets,
    harnessComparisonControlled:sameModel&&sameBudgets,
    note:!sameModel||!sameBudgets
      ?'Raw official outcomes shown; model and/or budgets differ, so the delta cannot isolate harness quality.'
      :'Same model and recorded iteration/token/time budgets; upstream Docker verdicts determine each paired result.',
    left:score(leftRows),right:score(rightRows),
    paired:{bothSolved:both,leftOnlySolved:leftOnly,
      rightOnlySolved:rightOnly,neitherSolved:neither,
      netLeftAdvantage:leftOnly-rightOnly},
    taskOutcomes:paired
  };
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  try{
    demand(process.argv.length===4,
      'usage: node scripts/sub2-swebench-compare.mjs /absolute/left/report.json /absolute/right/report.json');
    console.log(JSON.stringify(compareOfficialRuns(
      JSON.parse(readFileSync(process.argv[2],'utf8')),
      JSON.parse(readFileSync(process.argv[3],'utf8'))),null,2));
  }catch(error){console.error(String(error.stack||error));process.exitCode=2;}
}
