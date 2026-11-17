#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE="${DOMINION_WORKSPACE_DIR:-./workspace}"
TARGET="$WORKSPACE/bench-projects/two-file"
if [ -e "$TARGET" ]; then echo "Refusing to overwrite $TARGET" >&2; exit 2; fi
mkdir -p "$TARGET"
cp -R "$HERE/fixtures/two-file/." "$TARGET/"
(
  cd "$TARGET"
  git init -q
  git add .
  git -c user.name='Dominion Evaluation' -c user.email='eval@localhost' commit -qm 'baseline: two testable defects'
  git rev-parse HEAD
)
echo "Baseline prepared at $TARGET"
echo "Run: cargo run --bin dominion_coding_eval -- --manifest benchmarks/manifests/two-file.json --interactive"
