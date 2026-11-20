#!/usr/bin/env bash
# This is a source-architecture check, not a substitute for model/task benchmarks.
set -euo pipefail
required_count() {
  local file="$1" pattern="$2" expected="$3"
  local observed
  observed="$(grep -Ec "$pattern" "$file" || true)"
  if [[ "$observed" -ne "$expected" ]]; then
    echo "ERROR: $file expected $expected matches of [$pattern], observed $observed" >&2
    exit 1
  fi
}
# Only the strict provider-view projection is a supported production surface.
required_count src/context.rs '^[[:space:]]*pub fn compact_strict\(' 1
required_count src/context.rs '^[[:space:]]*pub fn compact\(' 0
# Native Run, its single real tool dispatch, and durable journal retain control.
required_count src/engine.rs '^[[:space:]]*async fn drive\(' 1
required_count src/engine.rs '^[[:space:]]*async fn perform_tool\(' 1
required_count src/journal.rs '^[[:space:]]*pub fn begin\(' 1
required_count src/journal.rs '^[[:space:]]*pub fn finish\(' 1
# Prevent split admission contracts and duplicate credential-free metadata fetchers.
required_count src/frontend_rooms.rs '^[[:space:]]*async fn admit_owner_message' 1
required_count src/frontend_rooms.rs 'engine[.]groups[.]say_once' 1
required_count src/frontend_public_http.rs '^[[:space:]]*pub[(]crate[)] async fn read_json' 1
required_count src/frontend_huggingface.rs 'reqwest::Client::builder' 0
required_count src/frontend_provider_public.rs 'reqwest::Client::builder' 0
# Do not resurrect an independent agent run loop or retired media/RSI executor.
for obsolete in \
  src/engine/chat/runtime.rs \
  src/rsi.rs \
  src/video.rs \
  src/video \
  src/engine/rsi.rs \
  src/money \
  web/src/pages/Money.tsx \
  crates/dominion-agent-sdk \
  crates/harness; do
  if [[ -e "$obsolete" ]]; then
    echo "ERROR: duplicate or retired execution implementation reintroduced: $obsolete" >&2
    exit 1
  fi
done
echo "PASS: one native Run/tool executor, one durable journal, one strict context projection"
