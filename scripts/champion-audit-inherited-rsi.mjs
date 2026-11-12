#!/usr/bin/env node
// Re-audit the exact Sub3 native RSI experiment inherited by Champion.
// This audits a past accepted lineage; it does NOT claim a fresh Champion run.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { gunzipSync } from 'node:zlib';

const sha = (value) => createHash('sha256').update(value).digest('hex');
const git = (...args) => execFileSync('git', args, { encoding: 'utf8' }).trim();
const blob = (commit, path) => execFileSync('git', ['show', `${commit}:${path}`]);
const proof = JSON.parse(readFileSync('benchmark/sub3-rsi-completion-2026-11-12.json', 'utf8'));
const fail = (message) => { throw new Error(message); };

assert.equal(proof.kind, 'dominion.sub3.native_recursive_completion');
assert.equal(proof.status, 'completed');
assert.equal(proof.stopReason, 'capability_demonstrated');
assert.equal(proof.capabilityDemonstrated, true);
assert.equal(proof.acceptedGenerations, 3);
assert.equal(proof.maxAcceptedStreak, 3);
assert.equal(proof.modelCalls, 0);
assert.equal(proof.tokens, 0);
assert.equal(proof.accelerationProven, false);
assert.equal(proof.measurementScope, 'frozen_native_cases_only');
assert.equal(proof.historicalBugRestored, false);
assert.equal(proof.generations.length, 3);
assert.equal(proof.adoptedHeadCommit, proof.generations[2].acceptedCommit);

let previous = proof.sourceCommit;
const binaries = new Set();
const observed = [];
for (const [i, gen] of proof.generations.entries()) {
  const label = `generation ${i + 1}`;
  assert.equal(gen.attempt, i + 1, `${label}: wrong attempt`);
  assert.equal(gen.generatorCommit, previous, `${label}: broken generator succession`);
  assert.match(gen.acceptedCommit, /^[0-9a-f]{40}$/);
  assert.match(gen.generatorBinarySha256, /^[0-9a-f]{64}$/);
  assert.equal(gen.generatorProviderId, 'dominion-native');
  assert.equal(gen.candidateOrigin, 'native_inductive_source_mutation');
  assert.equal(gen.nativeMutation.sourceChanged, true);
  assert.equal(gen.metrics.eligible, true);
  assert.equal(gen.metrics.regressions, 0);
  assert.ok(gen.metrics.improvedHoldouts > 0, `${label}: no holdout gain`);
  assert.equal(gen.tokenUsage.calls, 0);
  assert.equal(gen.adoptedPatchMatchesReceipt, true);
  assert.equal(gen.gitParentMatchesGenerator, true);

  const parents = git('rev-list', '--parents', '-n', '1', gen.acceptedCommit).split(' ');
  assert.deepEqual(parents, [gen.acceptedCommit, previous], `${label}: actual git parent mismatch`);
  const changed = git('diff', '--name-only', previous, gen.acceptedCommit).split('\n');
  assert.ok(changed.includes(gen.nativeMutation.path), `${label}: expected source path was not edited`);
  assert.equal(sha(blob(previous, gen.nativeMutation.path)), gen.nativeMutation.beforeSha256,
    `${label}: original source does not match preserved hash`);
  assert.equal(sha(blob(gen.acceptedCommit, gen.nativeMutation.path)), gen.nativeMutation.afterSha256,
    `${label}: adopted source does not match preserved hash`);

  const evidenceFile = `benchmark/rsi-evidence/2026-11-12-sub3/generation-${i + 1}.evidence.txt.gz`;
  assert.equal(gen.evidenceFile, evidenceFile);
  assert.equal(sha(gunzipSync(readFileSync(evidenceFile))), gen.pairedEvidenceSha256,
    `${label}: compressed original paired evidence mismatch`);
  binaries.add(gen.generatorBinarySha256);
  observed.push({ generation: i + 1, domain: gen.domain, source: gen.nativeMutation.path,
    improvedHoldouts: gen.metrics.improvedHoldouts, metaGain: gen.metrics.metaHoldoutGains });
  previous = gen.acceptedCommit;
}

assert.equal(binaries.size, 3, 'reused generator binary');
assert.equal(observed[0].domain, 'memory');
assert.equal(observed[1].domain, 'improver');
assert.ok(observed[1].metaGain > 0, 'missing independently measured improver gain');
assert.equal(observed[2].domain, 'memory');
execFileSync('git', ['merge-base', '--is-ancestor', previous, 'HEAD']);

console.log('CHAMPION_INHERITED_RSI_LINEAGE_AUDIT=PASS');
console.log(JSON.stringify({ claim: 'Inherited Sub3 bounded native RSI provenance, not independent Champion rerun',
  acceptedGenerations: 3, distinctBinaries: binaries.size, observations: observed }, null, 2));
