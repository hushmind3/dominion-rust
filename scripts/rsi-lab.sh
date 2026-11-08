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

run_suite() {
  local label="$1" dir="$2" ready=true ok=false
  if ! (cd "$dir" && cargo test --release --all-targets --no-run) >"$TEMP/$label.build.log" 2>&1; then ready=false; fi
  echo "$ready" >"$TEMP/$label.build"
  while IFS=$'\t' read -r case_id domain holdout filter; do
    [[ -n "$case_id" ]] || continue
    ok=false
    if [[ "$ready" == true ]]; then
      if (cd "$dir" && cargo test --release "$filter" -- --exact) >"$TEMP/$label.$case_id.log" 2>&1 \
        && grep -q 'running 1 test' "$TEMP/$label.$case_id.log"; then ok=true; fi
    fi
    echo "$ok" >"$TEMP/$label.$case_id.status"
  done <"$TEMP/cases.tsv"
}
run_suite baseline "$TEMP/baseline"
run_suite candidate "$TEMP/candidate"

mkdir -p "$(dirname "$REPORT")"
EVIDENCE="$REPORT.evidence.txt"
{
  echo "baseCommit=$BASE_SHA"
  cat "$TEMP/baseline.build.log" "$TEMP/candidate.build.log"
  while IFS=$'\t' read -r case_id domain holdout filter; do
    [[ -n "$case_id" ]] || continue
    for label in baseline candidate; do
      echo "$label/$case_id=$(cat "$TEMP/$label.$case_id.status")"
      if [[ -f "$TEMP/$label.$case_id.log" ]]; then cat "$TEMP/$label.$case_id.log"; fi
    done
  done <"$TEMP/cases.tsv"
} >"$EVIDENCE"
export RSI_EVIDENCE="$EVIDENCE"

node <<'NODE'
const fs=require('fs'),crypto=require('crypto'),path=require('path');
const read=p=>fs.readFileSync(p,'utf8');
const hash=b=>crypto.createHash('sha256').update(b).digest('hex');
const plan=JSON.parse(read(process.env.RSI_PLAN));
const rows=plan.cases.map(c=>({
 id:c.id,
 baselinePass:read(path.join(process.env.RSI_TEMP,'baseline.'+c.id+'.status')).trim()==='true',
 candidatePass:read(path.join(process.env.RSI_TEMP,'candidate.'+c.id+'.status')).trim()==='true'
}));
const report={
 baseCommit:process.env.RSI_BASE_SHA,
 patchSha256:hash(fs.readFileSync(process.env.RSI_PATCH)),
 oracleVersion:plan.oracleVersion,
 runner:'local-worktree-rsi-lab-v1',
 reportSha256:hash(fs.readFileSync(process.env.RSI_EVIDENCE)),
 baselineBuildOk:read(path.join(process.env.RSI_TEMP,'baseline.build')).trim()==='true',
 candidateBuildOk:read(path.join(process.env.RSI_TEMP,'candidate.build')).trim()==='true',
 cases:rows
};
fs.writeFileSync(process.env.RSI_REPORT,JSON.stringify(report,null,2)+'\n');
console.log(JSON.stringify({report:process.env.RSI_REPORT,evidence:process.env.RSI_EVIDENCE,
 gains:rows.filter(r=>!r.baselinePass&&r.candidatePass).length,
 regressions:rows.filter(r=>r.baselinePass&&!r.candidatePass).length},null,2));
NODE
