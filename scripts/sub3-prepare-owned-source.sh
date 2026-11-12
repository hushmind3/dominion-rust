#!/usr/bin/env bash
# Create a private, independently owned sub3 checkout for a real-model cycle.
# Never switch or update the runner's original checkout or any remote ref.
set -euo pipefail
if [[ $# -ne 3 ]]; then
  echo 'usage: sub3-prepare-owned-source.sh SOURCE_REPO TARGET_DIR PINNED_SHA' >&2
  exit 2
fi
source_repo=$(cd "$1" && pwd -P)
target=$2
revision=$3
if [[ ! "$revision" =~ ^[0-9a-f]{40}$ ]]; then
  echo 'A full 40-character pinned source commit is required' >&2
  exit 2
fi
if [[ -e "$target" ]]; then
  echo 'Refusing to overwrite an existing experiment checkout' >&2
  exit 2
fi
original=$(git -C "$source_repo" rev-parse --verify HEAD)
if [[ "$original" != "$revision" ]]; then
  echo 'The runner source changed from the pinned commit' >&2
  exit 2
fi
if [[ -n "$(git -C "$source_repo" status --porcelain --untracked-files=normal)" ]]; then
  echo 'Refusing to clone a dirty experiment source' >&2
  exit 2
fi
# Branch provenance must be anchored in the original checkout or an exact
# remote-tracking sub3 ref (actions/checkout can leave HEAD detached).
source_ref=$(git -C "$source_repo" symbolic-ref --quiet HEAD || true)
tracking_ref=$(git -C "$source_repo" rev-parse --verify 'refs/remotes/origin/dominion-sub3^{commit}' 2>/dev/null || true)
if [[ "$source_ref" != 'refs/heads/dominion-sub3' && "$tracking_ref" != "$revision" ]]; then
  echo 'Source revision is not the owned sub3 branch tip' >&2
  exit 2
fi
mkdir -p "$(dirname "$target")"
# A separate .git directory means the owned sub3 branch can be checked out
# even if the runner has dominion-sub3 active in its original worktree.
git clone --quiet --local --no-hardlinks --no-checkout "$source_repo" "$target"
git -C "$target" checkout --quiet -B dominion-sub3 "$revision"
# An explicit official remote preserves the engine's existing repo identity
# check. This sets metadata only and does not connect or push.
git -C "$target" remote set-url origin https://github.com/hushmind3/dominion-rust.git
if [[ "$(git -C "$target" symbolic-ref --quiet HEAD)" != 'refs/heads/dominion-sub3' ]] \
  || [[ "$(git -C "$target" rev-parse HEAD)" != "$revision" ]] \
  || [[ "$(git -C "$source_repo" rev-parse HEAD)" != "$original" ]] \
  || [[ -n "$(git -C "$target" status --porcelain --untracked-files=normal)" ]]; then
  echo 'The isolated source did not preserve ownership and commit identity' >&2
  exit 1
fi
printf 'SUB3_RUNTIME_SOURCE_BRANCH=dominion-sub3\nSUB3_RUNTIME_SOURCE_COMMIT=%s\n' "$revision"
