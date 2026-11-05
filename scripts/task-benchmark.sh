#!/usr/bin/env bash
# Long-term task benchmark for DOMINION.
#
# It measures whether a procedure learned from an owner correction persists
# across fresh sessions and reaches another agent in the same partition.
#
# The task itself deliberately does not mention the audit rule. The rule is
# taught only in the correction turn. Baseline should miss it; post-learning
# rounds and a second agent should follow it.
set -uo pipefail

BASE="${DOMINION_BASE:-}"
PROVIDER_NAME="${BENCH_PROVIDER:-Ollama Qwen3 4B}"
ROUNDS="${BENCH_ROUNDS:-3}"
BENCH_ROOT="${BENCH_ROOT:-}"
WORKSPACE="$BENCH_ROOT/workspace"
BENCH_DIR="$WORKSPACE/bench"
TASK='bench/rule.txt 파일을 만들고 내용을 RULE-OK로 저장해.'
CORRECTION='아니, bench 작업을 할 때는 먼저 bench/audit.txt 파일을 만들고 내용을 정확히 before:rule.txt 로 저장해야 해. 그 다음 bench/rule.txt를 만들어. 다시 해.'

json_id() {
  python3 -c 'import sys,json;d=json.load(sys.stdin);print(d.get("id",""))'
}

provider_id() {
  curl -fsS "$BASE/api/providers" | python3 -c '
import sys,json
name=sys.argv[1]
for item in json.load(sys.stdin):
    if item.get("name")==name:
        print(item["id"]); break
' "$PROVIDER_NAME"
}

new_agent() {
  curl -fsS -X POST "$BASE/api/agents" -H 'Content-Type: application/json' \
    -d "{\"name\":\"$1\",\"providerId\":\"$PROVIDER_ID\",\"model\":\"$MODEL\",\"systemPrompt\":\"You are a precise file-operation agent.\"}" \
    | json_id
}

new_session() {
  curl -fsS -X POST "$BASE/api/sessions" -H 'Content-Type: application/json' \
    -d "{\"title\":\"$1\"}" | json_id
}

chat() {
  local session="$1" agent="$2" input="$3" learn="$4"
  curl -sS -N -m 300 -X POST "$BASE/api/chat" -H 'Content-Type: application/json' \
    -d "{\"sessionId\":\"$session\",\"agentId\":\"$agent\",\"providerId\":\"$PROVIDER_ID\",\"input\":\"$input\",\"toolsEnabled\":true,\"autoLearning\":$learn}" >/dev/null
}

reset_bench() {
  rm -rf "$BENCH_DIR"
  mkdir -p "$BENCH_DIR"
}

check() {
  local rule audit
  rule=$(tr -d '[:space:]' < "$BENCH_DIR/rule.txt" 2>/dev/null)
  audit=$(grep -c 'before:rule.txt' "$BENCH_DIR/audit.txt" 2>/dev/null || true)
  [ "$rule" = "RULE-OK" ] && [ "${audit:-0}" -ge 1 ]
}

wait_reflect() {
  local session="$1"
  for _ in $(seq 1 150); do
    local status
    status=$(curl -sS "$BASE/api/jobs" | python3 -c '
import sys,json
session=sys.argv[1]
for job in json.load(sys.stdin):
    if job.get("kind")=="reflect" and (job.get("input") or {}).get("sessionId")==session:
        print(job.get("status","")); break
else:
    print("none")
' "$session" 2>/dev/null)
    case "$status" in
      completed|failed) echo "reflect=$status"; return 0 ;;
    esac
    sleep 2
  done
  echo "reflect=timeout"
  return 1
}

if [ -z "$BASE" ]; then
  echo "DOMINION_BASE must point to an isolated benchmark engine." >&2
  exit 2
fi
if [ "$BASE" = "http://127.0.0.1:4317" ] && [ "${ALLOW_LIVE_BENCHMARK:-0}" != "1" ]; then
  echo "Refusing to modify the live 4317 service. Set ALLOW_LIVE_BENCHMARK=1 only if intentional." >&2
  exit 2
fi
if [ -z "$BENCH_ROOT" ]; then
  echo "BENCH_ROOT must be a disposable benchmark root (data/workspace/sentinel)." >&2
  exit 2
fi
SENTINEL="$BENCH_ROOT/.dominion-benchmark-sentinel"
if [ ! -f "$SENTINEL" ] || ! grep -qi 'benchmark' "$SENTINEL"; then
  echo "Missing or invalid benchmark sentinel: $SENTINEL" >&2
  exit 2
fi
ROOT_REAL="$(cd "$BENCH_ROOT" 2>/dev/null && pwd -P)" || {
  echo "BENCH_ROOT does not exist: $BENCH_ROOT" >&2
  exit 2
}
case "$ROOT_REAL/" in
  "$HOME/dominion-rust/"*)
    echo "Refusing to use the live repository as BENCH_ROOT." >&2
    exit 2
    ;;
esac
mkdir -p "$WORKSPACE"
PROVIDER_ID=$(provider_id)
if [ -z "$PROVIDER_ID" ]; then
  echo "provider not found: $PROVIDER_NAME" >&2
  exit 2
fi
MODEL=$(curl -fsS "$BASE/api/providers" | python3 -c '
import sys,json
name=sys.argv[1]
for item in json.load(sys.stdin):
    if item.get("name")==name:
        print(item.get("model","")); break
' "$PROVIDER_NAME")
echo "provider=$PROVIDER_NAME model=$MODEL"

# Scoped fixture approval only. Never enable global auto-approval.
curl -fsS -X POST "$BASE/api/policy" -H 'Content-Type: application/json' \
  -d '{"autoApproveAll":false,"autoApproveMcp":false,"autoApprove":["workspace_write","workspace_read","workspace_list","file_read","file_write"]}' >/dev/null

AGENT_A=$(new_agent "Bench Agent A")
echo "agentA=$AGENT_A"

reset_bench
BASELINE_SESSION=$(new_session "Bench Baseline")
echo "--- baseline ---"
chat "$BASELINE_SESSION" "$AGENT_A" "$TASK" false
if check; then BASELINE_PASS=1; else BASELINE_PASS=0; fi
echo "baseline_pass=$BASELINE_PASS"

echo "--- teach (same session) ---"
chat "$BASELINE_SESSION" "$AGENT_A" "$CORRECTION" true
wait_reflect "$BASELINE_SESSION"

POST_PASS=0
for round in $(seq 1 "$ROUNDS"); do
  reset_bench
  session=$(new_session "Bench Post $round")
  chat "$session" "$AGENT_A" "$TASK" false
  if check; then
    POST_PASS=$((POST_PASS + 1))
    echo "post$round=PASS"
  else
    echo "post$round=FAIL"
  fi
done

AGENT_B=$(new_agent "Bench Agent B")
reset_bench
CROSS_SESSION=$(new_session "Bench Cross Agent")
chat "$CROSS_SESSION" "$AGENT_B" "$TASK" false
if check; then CROSS_PASS=1; else CROSS_PASS=0; fi
echo "cross_pass=$CROSS_PASS"

echo "RESULT baseline=$BASELINE_PASS post=$POST_PASS/$ROUNDS cross=$CROSS_PASS"
echo "agentA=$AGENT_A agentB=$AGENT_B"
