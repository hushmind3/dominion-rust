// Deterministic architecture gate. No network calls, models or paid tokens.
import fs from 'node:fs';
import path from 'node:path';

const root=process.cwd();
const exists=file=>fs.existsSync(path.join(root,file));
const read=file=>fs.readFileSync(path.join(root,file),'utf8');
const list=dir=>fs.readdirSync(path.join(root,dir),{withFileTypes:true})
  .flatMap(item=>{
    const name=path.posix.join(dir,item.name);
    return item.isDirectory()?list(name):item.isFile()?[name]:[];
  });
const errors=[];
const check=(ok,message)=>{if(!ok)errors.push(message)};
const flat=fs.readdirSync(path.join(root,'src/engine'))
  .filter(name=>name.endsWith('.rs')&&exists('src/engine/'+name));
check(flat.length<=75,'flat src/engine Rust modules >75: '+flat.length);
for(const file of ['src/engine/rsi_evolve.rs','src/engine/chat/runtime.rs']){
  check(exists(file)&&fs.statSync(path.join(root,file)).size<=64*1024,
    'entry point became a monolith: '+file);
}
for(const area of ['planner','execution','evaluation','lineage','meta','learning']){
  check(list('src/engine/rsi/'+area).some(f=>f.endsWith('.rs')),
    'RSI responsibility directory missing: '+area);
}
// Deliberately forbid executable training paths, not innocent model-provider
// names or evidence of fixed-weight model selection.
const forbidden=/rsi_model_weights|rsi_train_local_adapter|rsiSelectedAdapter|rsiAdapterRoute|rsiAllowLocalAdapterTraining|modelAdapterIndependentEvaluation|modelWeightLearning|paired_model_reward|sub4_adapter_(?:train|eval|server)\.py|rsi-adapter-candidates/;
for(const file of [...list('src'),...list('scripts')].filter(f=>
    /\.(?:rs|py|mjs|js|ts|tsx)$/.test(f)&&f!=='scripts/rsi-structure-audit.mjs')){
  if(forbidden.test(read(file)))errors.push('parameter-training RSI path: '+file);
}
for(const file of ['src/engine/rsi_model_weights.rs','scripts/sub4_adapter_train.py',
    'scripts/sub4_adapter_eval.py','scripts/sub4_adapter_server.py']){
  check(!exists(file),'removed training path returned: '+file);
}
const modules=['src/engine/mod.rs','src/engine/rsi_evolve.rs',
  'src/engine/chat/runtime.rs',...list('src/engine/rsi').filter(f=>f.endsWith('.rs')),
  ...list('src/engine/chat/runtime').filter(f=>f.endsWith('.rs'))];
for(const file of modules){
  for(const match of read(file).matchAll(/(?:#\[path\s*=\s*|include_(?:str|bytes)!\s*\(\s*)"([^"]+)"/g)){
    const target=path.normalize(path.join(path.dirname(file),match[1]));
    check(exists(target),'unresolved Rust module or fixture: '+file+' -> '+target);
  }
}
if(errors.length){
  for(const e of errors)console.error('RSI_STRUCTURE_AUDIT: '+e);
  process.exitCode=1;
}else{
  console.log('RSI_STRUCTURE_AUDIT_OK '+JSON.stringify({
    engineRootFiles:flat.length,modulePaths:modules.length,fixedWeightOnly:true
  }));
}
