import test from 'node:test'
import assert from 'node:assert/strict'
import {classifyRefreshEvent,matchesRefreshTarget,matchesRoomRefresh,mergeRefreshScopes,emptyRefreshScope} from '../src/refreshEvents.ts'

test('chat messages only refresh the matching transcript and sessions list',()=>{
 const change=classifyRefreshEvent({type:'message',message:{id:'m',session_id:'chat-1'}})
 assert.ok(matchesRefreshTarget('/api/sessions/chat-1/messages',change.targets))
 assert.ok(matchesRefreshTarget('/api/sessions',change.targets))
 assert.equal(matchesRefreshTarget('/api/sessions/chat-2/messages',change.targets),false)
 assert.equal(matchesRefreshTarget('/api/providers',change.targets),false)
 assert.equal(matchesRefreshTarget('/api/agents',change.targets),false)
})
test('room events reach only the active room state, not foreign sessions',()=>{
 const change=classifyRefreshEvent({type:'record',kind:'job',record:{id:'job',input:{sessionId:'s1',roomId:'r1'}}})
 assert.ok(matchesRoomRefresh(change,'r1','s1'))
 assert.equal(matchesRoomRefresh(change,'r2','s2'),false)
 assert.equal(matchesRefreshTarget('/api/providers',change.targets),false)
})
test('room settings and provider edits do not trigger unrelated data loads',()=>{
 const room=classifyRefreshEvent({type:'record',kind:'room',record:{id:'r1'}})
 assert.ok(matchesRefreshTarget('/api/rooms',room.targets))
 assert.ok(matchesRefreshTarget('/api/rooms/r1/state',room.targets))
 assert.equal(matchesRoomRefresh(room,'r2','s2'),false)
 const provider=classifyRefreshEvent({type:'record',kind:'provider',record:{id:'p'}})
 assert.ok(matchesRefreshTarget('/api/providers',provider.targets))
 assert.equal(matchesRefreshTarget('/api/rooms',provider.targets),false)
})
test('approval and model events target only their consumers',()=>{
 const approval=classifyRefreshEvent({type:'approval',approval:{sessionId:'s2',id:'a'}})
 assert.ok(matchesRefreshTarget('/api/approvals',approval.targets))
 assert.equal(matchesRefreshTarget('/api/agents',approval.targets),false)
 const model=classifyRefreshEvent({type:'record',kind:'model_download',record:{id:'download'}})
 assert.ok(matchesRefreshTarget('/api/model-library/runtime',model.targets))
 assert.equal(matchesRefreshTarget('/api/providers',model.targets),false)
})
test('merge collapses rapid SSE bursts while unknown events do not cause full reloads',()=>{
 const a=classifyRefreshEvent({type:'message',message:{session_id:'same'}})
 const merged=mergeRefreshScopes(mergeRefreshScopes(emptyRefreshScope(),a),a)
 assert.deepEqual(merged.targets,a.targets)
 assert.deepEqual(merged.sessions,['same'])
 assert.deepEqual(classifyRefreshEvent({type:'audit'}).targets,['/api/audit/*','/api/audit'])
 assert.deepEqual(classifyRefreshEvent({type:'unrecognized'}).targets,[])
})

test('native Rust run changes refresh SDK and terminal chat without unrelated queries',()=>{
 const running=classifyRefreshEvent({type:'job_event',id:'r1',status:'running'})
 assert.ok(matchesRefreshTarget('/api/sdk/runs/r1',running.targets))
 assert.ok(matchesRefreshTarget('/api/jobs/r1',running.targets))
 assert.equal(matchesRefreshTarget('/api/providers',running.targets),false)
 assert.equal(matchesRefreshTarget('/api/sessions/s1/messages',running.targets),false)
 const done=classifyRefreshEvent({type:'job_event',id:'r1',status:'completed'})
 assert.ok(matchesRefreshTarget('/api/sessions/s1/messages',done.targets))
 const approval=classifyRefreshEvent({type:'job_event',id:'r1',status:'awaiting_approval'})
 assert.ok(matchesRefreshTarget('/api/approvals',approval.targets))
});
