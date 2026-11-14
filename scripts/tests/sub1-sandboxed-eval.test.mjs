import test from "node:test";
import assert from "node:assert/strict";
import {mkdtempSync,writeFileSync,mkdirSync,rmSync,symlinkSync} from "node:fs";
import {tmpdir} from "node:os";
import {join} from "node:path";
import {sandboxedModuleOracle} from "../sub1-sandboxed-eval.mjs";

function withFixture(source,fn){
 const root=mkdtempSync(join(tmpdir(),"dominion-oracle-"));
 const workspace=join(root,"workspace");
 mkdirSync(join(workspace,"src"),{recursive:true});
 writeFileSync(join(workspace,"src/price.mjs"),source);
 try{fn(workspace,root)}finally{rmSync(root,{recursive:true,force:true})}
}
const good=String.raw`
 export const currency="KRW";
 export function totalWithTax(price,rate){
   if(typeof price!=="number"||typeof rate!=="number"||!Number.isFinite(price)
     ||!Number.isFinite(rate)||price<0||rate<0)return null;
   return Math.round(price*(1+rate))
 }`;
test("real executable model patch passes only true behavioral test cases",()=>{
 withFixture(good,workspace=>{
  assert.equal(sandboxedModuleOracle("src/price.mjs",workspace),true);
 });
});
test("wrong implementation and fake textual success fail",()=>{
 withFixture('export const currency="KRW"; export const totalWithTax=()=>110;',workspace=>{
  assert.equal(sandboxedModuleOracle("src/price.mjs",workspace),false);
 });
});
test("model code cannot read outside target or invoke a subprocess",()=>{
 withFixture('import {readFileSync} from "node:fs"; readFileSync("/etc/passwd","utf8"); export const currency="KRW"; export const totalWithTax=()=>110;',workspace=>{
  assert.equal(sandboxedModuleOracle("src/price.mjs",workspace),false);
 });
 withFixture('import {execSync} from "node:child_process"; execSync("whoami"); export const currency="KRW"; export const totalWithTax=()=>110;',workspace=>{
  assert.equal(sandboxedModuleOracle("src/price.mjs",workspace),false);
 });
});
test("symlink to hidden oracle and parent escapes are rejected",()=>{
 withFixture(good,(workspace,root)=>{
  const hidden=join(root,"hidden.mjs");
  writeFileSync(hidden,good);
  symlinkSync(hidden,join(workspace,"src/escape.mjs"));
  assert.equal(sandboxedModuleOracle("src/escape.mjs",workspace),false);
  assert.equal(sandboxedModuleOracle("../hidden.mjs",workspace),false);
 });
});
