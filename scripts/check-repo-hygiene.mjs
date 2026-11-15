#!/usr/bin/env node
// Reproducible repository hygiene contract. This checks tracked files only:
// ordinary ignored build outputs are not part of a reproducible source tree.
import {execFileSync} from 'node:child_process'
import {statSync} from 'node:fs'
import {basename} from 'node:path'

const paths=execFileSync('git',['ls-files','-z'],{maxBuffer:16*1024*1024})
  .toString('utf8').split('\0').filter(Boolean)
const errors=[]
const generatedSegment=/\/(?:node_modules|target|dist|coverage|__pycache__|\.pytest_cache|\.cache|\.tmp|temp|tmp)\//
const transientSuffix=/(?:\.tmp|\.bak|\.orig|\.rej|\.swp|\.swo|\.sqlite-(?:wal|shm|journal)|\.part(?:\.[0-9a-f]{40})?|\.profraw|\.profdata)$/
const buildOutput=/\.(?:exe|dll|so|dylib|o|rlib|rmeta|class|pyc|pyo)$/
let trackedBytes=0
let docs=0
for (const path of paths) {
  const normalized='/'+path.replaceAll('\\','/')
  if (generatedSegment.test(normalized)||transientSuffix.test(path)||buildOutput.test(path)
      ||/^\/reports\//.test(normalized)||/^\/logs\//.test(normalized)) {
    errors.push(`generated/temporary file committed: ${path}`)
  }
  if (path.startsWith('docs/') && path.endsWith('.md')) docs++
  let size
  try {size=statSync(path).size} catch(error) {
    errors.push(`tracked file missing: ${path}: ${error.message}`)
    continue
  }
  trackedBytes+=size
  if(path === 'README.md' && size > 20*1024) {
    errors.push(`README has turned into an append-only changelog (${size} bytes); keep it below 20KiB`)
  }
  if(path === 'src/engine/goals.rs' && size > 12*1024) {
    errors.push(`goal entry module has regrown (${size} bytes); update src/engine/goals/* instead`)
  }
  if(path === 'src/api/goals.rs' && size > 8*1024) {
    errors.push(`goal HTTP facade has regrown (${size} bytes); update src/api/goals/* instead`)
  }
  if(size>1024*1024) errors.push(`tracked file >1MiB: ${path} (${size} bytes)`)
  if(path.startsWith('src/') && path.endsWith('.rs') && size>120*1024) {
    errors.push(`oversized Rust implementation: ${path} (${size} bytes); split module`)
  }
  if(path.startsWith('tests/') && path.endsWith('.rs') && size>400*1024) {
    errors.push(`oversized test file: ${path} (${size} bytes); split fixtures`)
  }
}
if(docs>25) errors.push(`documentation snapshots are accumulating (${docs}>25); consolidate old docs`)
if(errors.length) {
  console.error('DOMINION repository hygiene FAILED:\n'+errors.map(x=>' - '+x).join('\n'))
  process.exitCode=1
}else{
  console.log(`DOMINION repository hygiene OK: ${paths.length} tracked files; ${(
    trackedBytes/1024/1024).toFixed(2)} MiB; ${docs} docs. No tracked output debris.`)
}
