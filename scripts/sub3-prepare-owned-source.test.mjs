import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtempSync,mkdirSync,writeFileSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {execFileSync,spawnSync} from 'node:child_process';
const script=path.join(path.dirname(fileURLToPath(import.meta.url)),'sub3-prepare-owned-source.sh');
function git(cwd,...args){return execFileSync('git',args,{cwd,encoding:'utf8'}).trim();}
function fixture(){
  const dir=mkdtempSync(path.join(tmpdir(),'sub3-owner-contract-'));
  const src=path.join(dir,'source');mkdirSync(path.join(src,'src','engine'),{recursive:true});
  writeFileSync(path.join(src,'Cargo.toml'),'[package]\nname="fixture"\nversion="0.1.0"\n');
  writeFileSync(path.join(src,'src','engine','rsi.rs'),'// fixture');
  git(src,'init','-q');git(src,'checkout','-qb','main');
  git(src,'add','.');git(src,'-c','user.name=Test','-c','user.email=test@example.com','commit','-qm','baseline');
  return {dir,src,sha:git(src,'rev-parse','HEAD')};
}
function prepare(src,dst,sha){return spawnSync('bash',[script,src,dst,sha],{encoding:'utf8'});}
test('isolated source uses exactly the owned branch and pinned bytes, leaving main untouched',()=>{
  const {dir,src,sha}=fixture();try{
    const dst=path.join(dir,'isolated');
    const result=prepare(src,dst,sha);
    assert.equal(result.status,0,result.stderr);
    assert.equal(git(src,'symbolic-ref','HEAD'),'refs/heads/main');
    assert.equal(git(dst,'symbolic-ref','HEAD'),'refs/heads/dominion-sub3');
    assert.equal(git(src,'rev-parse','HEAD'),sha);
    assert.equal(git(dst,'rev-parse','HEAD'),sha);
    assert.equal(git(dst,'remote','get-url','origin'),'https://github.com/hushmind3/dominion-rust.git');
    assert.equal(git(dst,'status','--porcelain'),'');
    assert.match(result.stdout,/SUB3_RUNTIME_SOURCE_BRANCH=dominion-sub3/);
    assert.notEqual(prepare(src,dst,sha).status,0,'existing checkout must never be replaced');
  }finally{rmSync(dir,{recursive:true,force:true});}
});
test('rejects wrong source revision and dirty checkout without creating a clone',()=>{
  const {dir,src,sha}=fixture();try{
    const wrong=path.join(dir,'wrong');
    assert.notEqual(prepare(src,wrong,'0'.repeat(40)).status,0);
    assert.notEqual(prepare(src,wrong,'bad').status,0);
    writeFileSync(path.join(src,'local-edit.txt'),'uncommitted');
    assert.notEqual(prepare(src,wrong,sha).status,0);
  }finally{rmSync(dir,{recursive:true,force:true});}
});
