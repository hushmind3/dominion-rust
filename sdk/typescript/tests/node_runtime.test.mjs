import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {DominionRuntime} from '../node.mjs';
test('native Node host validates binary/workspace and port before dispatch',async()=>{
  const folder=await mkdtemp(join(tmpdir(),'dominion-sdk-native-'));
  try{
    await assert.rejects(()=>DominionRuntime.spawn({
      binary:join(folder,'missing'),workspace:folder,
      dataDir:join(folder,'state')
    }));
    await assert.rejects(()=>DominionRuntime.spawn({
      binary:process.execPath,workspace:folder,
      dataDir:join(folder,'state'),port:0
    }),RangeError);
  }finally{await rm(folder,{force:true,recursive:true})}
});
test('a non-Dominion program is not misreported as a native engine',async()=>{
  const folder=await mkdtemp(join(tmpdir(),'dominion-native-reject-'));
  try{
    await assert.rejects(()=>DominionRuntime.spawn({
      binary:process.execPath,workspace:folder,
      dataDir:join(folder,'state'),startupTimeoutMs:1100
    }),/exited|timeout|spawn/i);
  }finally{await rm(folder,{force:true,recursive:true})}
});
test('real Rust child process lifecycle is an optional native integration',async(t)=>{
  if(!process.env.DOMINION_RUST_BINARY){t.skip('No native binary supplied');return;}
  const folder=await mkdtemp(join(tmpdir(),'dominion-native-actual-'));
  try{
    const host=await DominionRuntime.spawn({
      binary:process.env.DOMINION_RUST_BINARY,workspace:folder,
      dataDir:join(folder,'data'),startupTimeoutMs:45000
    });
    const health=await host.client.transport.json('/api/health','GET');
    assert.equal(health.engine,'rust');
    await host.shutdown();
  }finally{await rm(folder,{force:true,recursive:true})}
});
