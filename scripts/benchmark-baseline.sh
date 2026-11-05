#!/usr/bin/env bash
# Records the exact starting point for a benchmark/reproducibility session.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT" || exit 2
BRANCH="$(git rev-parse --abbrev-ref HEAD)"
HEAD="$(git rev-parse HEAD)"
REMOTE="$(git ls-remote origin "refs/heads/$BRANCH" 2>/dev/null | awk '{print $1}')"
TRACKED="$(git diff --binary | shasum -a 256 | awk '{print $1}')"
STATUS="$(git status --porcelain)"
UNTRACKED="$(git ls-files --others --exclude-standard | while IFS= read -r file; do
  [ -f "$file" ] || continue
  printf '%s\t%s\n' "$file" "$(shasum -a 256 "$file" | awk '{print $1}')"
done)"
mkdir -p benchmark
UNTRACKED="$UNTRACKED" STATUS="$STATUS" BRANCH="$BRANCH" HEAD="$HEAD" REMOTE="$REMOTE" TRACKED="$TRACKED" \
python3 - <<'PY'
import hashlib, json, os
untracked = []
for line in os.environ.get("UNTRACKED", "").splitlines():
    if "\t" in line:
        path, digest = line.split("\t", 1)
        untracked.append({"path": path, "sha256": digest})
manifest = {
    "generatedAt": __import__("datetime").datetime.now(
        __import__("datetime").timezone.utc
    ).isoformat(),
    "branch": os.environ["BRANCH"],
    "head": os.environ["HEAD"],
    "remoteHead": os.environ["REMOTE"],
    "trackedDiffSha256": os.environ["TRACKED"],
    "statusSha256": hashlib.sha256(os.environ["STATUS"].encode()).hexdigest(),
    "status": os.environ["STATUS"].splitlines(),
    "untracked": untracked,
    "note": "S0 baseline manifest. It records the starting point; it is not an experiment result.",
}
with open("benchmark/BASELINE-2026-11-05.json", "w") as handle:
    json.dump(manifest, handle, ensure_ascii=False, indent=2)
    handle.write("\n")
print(json.dumps(manifest, ensure_ascii=False, indent=2)[:1200])
PY
