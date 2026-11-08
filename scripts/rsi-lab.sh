#!/usr/bin/env bash
# Manual paired code experiment. Run untrusted patches in a disposable VM.
set -euo pipefail
if [[ $# != 4 ]]; then
  echo "Usage: bash scripts/rsi-lab.sh BASE_SHA candidate.patch cases.json report.json" >&2
  exit 2
fi
BASE_SHA="$1"
PATCH="$(realpath "$2")"
PLAN="$(realpath "$3")"
REPORT="$(node -p "require('path').resolve(process.argv[1])" "$4")"
ROOT="$(git rev-parse --show-toplevel)"
TEMP="$(mktemp -d /tmp/dominion-rsi.XXXXXX)"
CREATED_BASE=false
CREATED_CANDIDATE=false
cleanup() {
  if [[ "$CREATED_CANDIDATE" == true ]]; then git -C "$ROOT" worktree remove --force "$TEMP/candidate" >/dev/null 2>&1 || true; fi
  if [[ "$CREATED_BASE" == true ]]; then git -C "$ROOT" worktree remove --force "$TEMP/baseline" >/dev/null 2>&1 || true; fi
  rm -rf "$TEMP"
}
trap cleanup EXIT
if [[ ! "$BASE_SHA" =~ ^[0-9a-fA-F]{40}$ ]]; then echo "Expected full Git commit SHA" >&2; exit 2; fi
git -C "$ROOT" cat-file -e "$BASE_SHA^{commit}"
export RSI_TEMP="$TEMP" RSI_PLAN="$PLAN" RSI_PATCH="$PATCH" RSI_BASE_SHA="$BASE_SHA" RSI_REPORT="$REPORT"

# Each case is an exact test name. No arbitrary shell commands in the manifest.
node <<'NODE'
const fs=require('fs'),path=require('path'),s=JSON.parse(fs.readFileSync(process.env.RSI_PLAN,'utf8'));
if(!Array.isArray(s.cases)||s.cases.length<2||s.cases.length>128)throw Error('2..128 cases required');
if(!/^[A-Za-z0-9_.-]{1,120}$/.test(s.oracleVersion||''))throw Error('oracleVersion required');
const seen=new Set();
for(const c of s.cases){
 if(!/^[A-Za-z0-9_.-]{1,120}$/.test(c.id||'')||
    !/^[A-Za-z0-9_.-]{1,80}$/.test(c.domain||'')||
    !/^[A-Za-z0-9_:]{1,240}$/.test(c.filter||'')||seen.has(c.id))
   throw Error('Invalid or duplicate case id/domain/exact filter');
 seen.add(c.id);
}
fs.writeFileSync(path.join(process.env.RSI_TEMP,'cases.tsv'),
 s.cases.map(c=>[c.id,c.domain,c.holdout===true?'true':'false',c.filter].join('\t')).join('\n')+'\n');
NODE

git -C "$ROOT" worktree add --quiet --detach "$TEMP/baseline" "$BASE_SHA"
CREATED_BASE=true
git -C "$ROOT" worktree add --quiet --detach "$TEMP/candidate" "$BASE_SHA"
CREATED_CANDIDATE=true

# Optional test-only fixture patch, frozen and applied to BOTH variants.
FIXTURE="$(node -e "const s=require(process.argv[1]);process.stdout.write(s.fixturePatch||'')" "$PLAN")"
if [[ -n "$FIXTURE" ]]; then
  FIXTURE="$(cd "$(dirname "$PLAN")" && node -p "require('path').resolve(process.argv[1])" "$FIXTURE")"
  for dir in "$TEMP/baseline" "$TEMP/candidate"; do
    git -C "$dir" apply --check "$FIXTURE"
    git -C "$dir" apply "$FIXTURE"
  done
fi
git -C "$TEMP/candidate" apply --check "$PATCH"
git -C "$TEMP/candidate" apply "$PATCH"

# The outer runner is never taken from a candidate worktree.
node "$(dirname "${BASH_SOURCE[0]}")/rsi-measure.mjs"
