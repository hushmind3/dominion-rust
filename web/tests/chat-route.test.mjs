import test from 'node:test'
import assert from 'node:assert/strict'
import {chatRoute} from '../src/utils/chatRoute.ts'

const providers=[
 {id:'provider-a',model:'default-a'},
 {id:'provider-b',model:'default-b'},
]

test('stored conversation route wins when an agent default is edited later',()=>{
 const session={agentId:'agent-1',providerId:'provider-a',model:'pinned-model'}
 const changedAgent={providerId:'provider-b',model:'new-agent-model'}
 assert.deepEqual(chatRoute(session,'agent-1',changedAgent,undefined,providers),
  {providerId:'provider-a',model:'pinned-model'})
})

test('a missing or disabled stored provider is not silently replaced by another',()=>{
 const session={agentId:'agent-1',providerId:'missing-provider',model:'stored-model'}
 assert.deepEqual(chatRoute(session,'agent-1',{providerId:'provider-a',model:'default-a'},undefined,providers),
  {providerId:'missing-provider',model:'stored-model'})
})

test('explicit provider override selects the provider default, never a stale session model',()=>{
 const session={agentId:'agent-1',providerId:'provider-a',model:'pinned-model'}
 assert.deepEqual(chatRoute(session,'agent-1',{providerId:'provider-a',model:'agent-model'},
  {providerId:'provider-b',model:''},providers),{providerId:'provider-b',model:'default-b'})
 assert.deepEqual(chatRoute(session,'agent-1',{providerId:'provider-a',model:'agent-model'},
  {providerId:'provider-b',model:'owner-picked'},providers),{providerId:'provider-b',model:'owner-picked'})
})

test('new chat uses its selected agent defaults',()=>{
 assert.deepEqual(chatRoute(undefined,'agent-1',
  {providerId:'provider-b',model:'agent-default'},undefined,providers),
  {providerId:'provider-b',model:'agent-default'})
})
