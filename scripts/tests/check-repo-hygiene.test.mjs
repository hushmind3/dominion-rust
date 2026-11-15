import test from 'node:test'
import assert from 'node:assert/strict'
import {auditFiles,inheritedLargeSources} from '../check-repo-hygiene.mjs'

const core=[
  {path:'src/engine/goals/mod.rs',size:38,sha:'01',content:'pub mod execution;\n#[path = "execution.rs"] mod execution;'},
  {path:'src/engine/goals/execution.rs',size:16,sha:'02',content:'pub fn run() {}'},
  {path:'src/api/goals/mod.rs',size:24,sha:'03',content:'pub mod owner;'},
  {path:'src/api/goals/owner.rs',size:16,sha:'04',content:'pub fn act() {}'},
]

test('known clean baseline is allowed',()=>{
  assert.deepEqual(auditFiles(core).errors,[])
})
test('generated build, temp and binary files make CI fail',()=>{
  for(const path of ['target/release/app','web/dist/index.js','sdk/typescript/dist/index.js',
    'runtime/trace.log','src/engine/goals.rs','src/api/goals.rs','cache/data.sqlite-wal',
    'tools/foo.exe','data/part.gguf.part','download/model.dom inion-previous'.replace(' ','')]){
    const result=auditFiles([...core,{path,size:20,sha:'extra'}])
    assert.notEqual(result.errors.length,0,path)
  }
})
test('exact duplicate tracked files fail while empty keep markers are fine',()=>{
  const duplicate=[...core,{path:'src/demo/a.rs',size:99,sha:'same',content:'pub fn a() {}'},
    {path:'src/demo/b.rs',size:99,sha:'same',content:'pub fn a() {}'}]
  assert.match(auditFiles(duplicate).errors.join(' '),/duplicate tracked/)
  const markers=[...core,{path:'one/.gitkeep',size:0,sha:'empty'},
    {path:'two/.gitkeep',size:0,sha:'empty'}]
  assert.deepEqual(auditFiles(markers).errors,[])
})
test('dangling explicit rust module paths are rejected',()=>{
  const broken=core.map(x=>({...x}))
  broken[0].content='#[path = "deleted_legacy.rs"] mod legacy;'
  assert.match(auditFiles(broken).errors.join(' '),/dangling Rust/)
})
test('stale deleted goal source paths fail even if referenced by active RSI map',()=>{
  const broken=[...core,{path:'src/engine/rsi_system.rs',size:100,sha:'05',
    content:'const COMPONENTS: &[(&str,&[&str])] = &[\n("harness",&["src/engine/goals.rs"]),\n];'}]
  assert.match(auditFiles(broken).errors.join(' '),/goals\.rs|missing RSI indexed source/)
})
test('new monoliths and any further growth of inherited large source fail',()=>{
  assert.match(auditFiles([...core,{path:'src/new_monolith.rs',size:49*1024,sha:'06'}])
    .errors.join(' '),/size budget/)
  assert.match(auditFiles([...core,{path:'src/engine/models.rs',size:inheritedLargeSources['src/engine/models.rs']+1,sha:'06'}])
    .errors.join(' '),/size budget/)
})
test('append-only README and more than 25 dated snapshots are rejected',()=>{
  const records=[...core,{path:'README.md',size:21*1024,sha:'readme'}]
  for(let i=0;i<26;i++)records.push({path:'docs/history-'+i+'.md',size:40,sha:'doc'+i})
  const errors=auditFiles(records).errors.join(' ')
  assert.match(errors,/README changelog bloat/)
  assert.match(errors,/documentation snapshot/)
})
test('missing goal directory entry points fail',()=>{
  const records=core.filter(x=>x.path!=='src/api/goals/mod.rs')
  assert.match(auditFiles(records).errors.join(' '),/canonical directory-based/)
})
