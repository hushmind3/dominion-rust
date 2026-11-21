#!/usr/bin/env node
// P3 real multi-host probe; safe discovery only unless explicitly opted in.
import {randomUUID} from 'node:crypto';

const execute=process.env.DOMINION_REMOTE_SMOKE_EXECUTE==='true';
const registry=JSON.parse(process.env.DOMINION_REMOTE_WORKERS_JSON||'[]');
if (!Array.isArray(registry)||registry.length<2){
  throw new Error('Configure at least 2 DOMINION_REMOTE_WORKERS_JSON nodes');
}
const ids=new Set(), urls=new Set();
for(const node of registry){
  const url=new URL(node.url);
  if(!node.id||ids.has(node.id)||urls.has(url.origin)||
    (url.protocol!=='https:' && !(url.protocol==='http:' &&
      ['localhost','127.0.0.1','[::1]'].includes(url.hostname)))||
    url.pathname!=='/'||url.search||url.hash||url.username||url.password||
    typeof node.token!=='string'||node.token.length<32){
    throw new Error('Remote node registry contains a duplicate or unsafe entry');
  }
  ids.add(node.id);urls.add(url.origin);
}
async function call(node,path,method='GET',body=null){
  const response=await fetch(new URL(path,node.url),{
    method,redirect:'error',signal:AbortSignal.timeout(15000),
    headers:{'x-dominion-worker-token':node.token,
      ...(body?{'content-type':'application/json'}:{})},
    body:body?JSON.stringify(body):undefined,
  });
  if(!response.ok)throw new Error(`${node.id} ${method} ${path}: HTTP ${response.status}`);
  const data=await response.json();
  return data;
}
const active=[];
for(const node of registry){
  const state=await call(node,'/api/v1/remote-worker/capabilities');
  if(state.schemaVersion!==1||state.readOnly!==true||
     !Number.isInteger(state.slots)||state.slots<1){
    throw new Error(`${node.id}: invalid native worker capability contract`);
  }
  active.push({node,slots:state.slots,busy:state.active,writer:state.writer===true});
}
console.log('Remote native worker endpoints checked:',active.map(a=>({
  id:a.node.id,slots:a.slots,busy:a.busy,writer:a.writer
})));
if(!execute){
  console.log('Discovery verified; no model calls dispatched (set DOMINION_REMOTE_SMOKE_EXECUTE=true to run).');
  process.exit(0);
}
const receipts=[];
for(const {node} of active){
  const id=randomUUID();
  const run=await call(node,'/api/v1/remote-worker/runs','POST',{
    runId:id,instruction:'Report a short observation from this node without modifying files',
    budgetKrw:100,maxSteps:1,writer:false
  });
  if(run.id!==id)throw new Error(`${node.id}: remote Run identity changed`);
  let terminal=run;
  for(let attempt=0;attempt<60 &&
      ['queued','running'].includes(terminal.status);attempt++){
    await new Promise(resolve=>setTimeout(resolve,1000));
    terminal=await call(node,`/api/v1/remote-worker/runs/${id}`);
  }
  if(terminal.id!==id||!['completed','failed','cancelled','paused','awaiting_approval']
    .includes(terminal.status))throw new Error(`${node.id}: unknown remote Run state`);
  if(!Number.isFinite(terminal.costKrw)||terminal.costKrw<0){
    throw new Error(`${node.id}: missing KRW receipt`);
  }
  receipts.push({node:node.id,runId:id,status:terminal.status,
    costKrw:terminal.costKrw});
}
if(new Set(receipts.map(v=>v.runId)).size!==receipts.length)
  throw new Error('Duplicate remote Run IDs');
console.log('Independent native remote receipts:',receipts);
console.log('Reported remote KRW total:',
  receipts.reduce((sum,v)=>sum+v.costKrw,0));
if(receipts.some(v=>v.status!=='completed')){
  process.exitCode=1; // preserve actual receipt instead of inventing success
}
