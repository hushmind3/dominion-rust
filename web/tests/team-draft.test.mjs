import test from 'node:test'
import assert from 'node:assert/strict'
import {analyzeTeamDraft} from '../src/utils/teamDraft.ts'

const task=(id,depends='',budget='1000')=>({
 id,depends,budget,instruction:'Implement and verify',workspace:'project',steps:'12'
})
const check=(nodes,total='3000')=>analyzeTeamDraft(nodes,total)

test('dependency levels preserve real DAG ordering without claiming parallel execution',()=>{
 const graph=check([task('research'),task('implement','research'),task('verify','implement')])
 assert.equal(graph.issue,null)
 assert.deepEqual(graph.waves,[['research'],['implement'],['verify']])
 assert.equal(graph.usedBudget,3000)
 const branching=check([task('review','a,b'),task('b'),task('a')])
 assert.deepEqual(branching.waves,[['b','a'],['review']])
})

test('unique IDs and exact dependencies are rejected before any HTTP write',()=>{
 assert.equal(check([task('same'),task(' same ')]).issue.code,'duplicate_id')
 assert.equal(check([task('a b')]).issue.code,'invalid_id')
 assert.equal(check([task('a','a')]).issue.code,'self_dependency')
 assert.deepEqual(check([task('a','missing')]).issue,
  {code:'unknown_dependency',taskId:'a',dependency:'missing'})
 assert.equal(check([task('a','b,b'),task('b')]).issue.code,'duplicate_dependency')
 assert.equal(check([task('a','b'),task('b','a')]).issue.code,'cycle')
})

test('worker instruction includes the owner goal and wire framing before the Rust 20KB limit',()=>{
 const almostFull='a'.repeat(19_990)
 assert.equal(check([{...task('a'),instruction:almostFull}]).issue.code,'invalid_instruction')
 assert.equal(analyzeTeamDraft([task('a')],'3000','Goal '.repeat(5000)).issue.code,'invalid_instruction')
})

test('backend budget, workspace, instruction and step constraints are preflighted',()=>{
 assert.equal(check([task('a','', '4000')]).issue.code,'budget_exceeded')
 assert.equal(check([task('a')],'1000001').issue.code,'invalid_total_budget')
 assert.equal(check([task('a')],'').issue.code,'invalid_total_budget')
 assert.equal(check([{...task('a'),budget:''}]).issue.code,'invalid_budget')
 assert.equal(check([{...task('a'),steps:'1.5'}]).issue.code,'invalid_steps')
 assert.equal(check([{...task('a'),instruction:'   '}]).issue.code,'invalid_instruction')
 assert.equal(check([{...task('a'),workspace:'../secret'}]).issue.code,'invalid_workspace')
 assert.equal(check([{...task('a'),workspace:''}]).issue,null)
})
