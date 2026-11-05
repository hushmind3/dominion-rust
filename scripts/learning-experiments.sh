#!/usr/bin/env bash
# Compares three conditions on held-out tasks with the same model and fixtures:
#   baseline   : fresh data, no prior learning
#   experience : prior correction/adaptation sessions, then held-out tasks
#   optimized  : experience plus the currently selected optimizer/policy
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${DOMINION_BIN:-$ROOT_DIR/target/release/dominion-rust}"
OUT="${1:-$ROOT_DIR/benchmark/LEARNING-COMPARE-$(date +%F).json}"
WORK=$(mktemp -d "${TMPDIR:-/tmp}/dominion-learning.XXXXXX")
[ -x "$BIN" ] || { echo "missing binary: $BIN" >&2; exit 2; }

start_engine() {
  local root="$1"
  local port
  port=$(python3 -c 'import socket;s=socket.socket();s.bind(("127.0.0.1",0));print(s.getsockname()[1]);s.close()')
  DOMINION_PORT="$port" DOMINION_DATA="$root/runtime" DOMINION_WEB="$root/web" \
    DOMINION_WORKSPACE="$root" DOMINION_BENCHMARK=1 \
    DOMINION_EXPERIMENT_SCHEDULER=0 DOMINION_BACKUP_INTERVAL_SECS=0 \
    "$BIN" >"$root/server.log" 2>&1 &
  ENGINE_PID=$!
  for _ in $(seq 1 120); do
    curl -sf "http://127.0.0.1:$port/api/health" >/dev/null && break
    sleep 0.1
  done
  export DOMINION_URL="http://127.0.0.1:$port"
}

stop_engine() {
  kill "${ENGINE_PID:-0}" 2>/dev/null
  wait "${ENGINE_PID:-0}" 2>/dev/null
}

run_condition() {
  local name="$1" root="$2" learn="$3" optimize="$4"
  mkdir -p "$root"
  start_engine "$root"
  if [ "$learn" = "1" ]; then
    DOMINION_URL="$DOMINION_URL" python3 "$ROOT_DIR/scripts/learning-experiments.py" \
      --only abc --out "$root/learning.json" >/dev/null
  fi
  if [ "$optimize" = "1" ]; then
    # Ask the engine to expose and select the best recorded optimizer, then
    # run candidate generation so the next tasks use it.
    curl -sf -X POST "$DOMINION_URL/api/benchmark/policy-search" \
      -H 'Content-Type: application/json' -d '{}' >/dev/null || true
  fi
  DOMINION_URL="$DOMINION_URL" python3 "$ROOT_DIR/scripts/learning-experiments.py" \
    --only f --out "$root/f.json" >/dev/null
  curl -s "$DOMINION_URL/api/optimizers" >"$root/optimizers.json"
  stop_engine
  echo "$root"
}

BASE_ROOT=$(run_condition baseline "$WORK/baseline" 0 0)
EXP_ROOT=$(run_condition experience "$WORK/experience" 1 0)
OPT_ROOT=$(run_condition optimized "$WORK/optimized" 1 1)

python3 - "$BASE_ROOT" "$EXP_ROOT" "$OPT_ROOT" "$OUT" <<'PY'
import json, sys, pathlib
base, exp, opt, out = sys.argv[1:5]
def load(root):
    return {
        "heldOut": json.load(open(pathlib.Path(root)/"f.json")),
        "optimizers": json.load(open(pathlib.Path(root)/"optimizers.json")),
    }
report = {
    "model": __import__("os").environ.get("MODEL", "qwen3:4b"),
    "conditions": {
        "baseline": load(base),
        "experience": load(exp),
        "optimized": load(opt),
    },
}
def summary(condition):
    tasks = condition["heldOut"].get("f", [])
    return {
        "success": sum(1 for task in tasks if task.get("success")),
        "tasks": len(tasks),
    }
report["summary"] = {name: summary(value) for name, value in report["conditions"].items()}
path = pathlib.Path(out)
path.parent.mkdir(parents=True, exist_ok=True)
path.write_text(json.dumps(report, ensure_ascii=False, indent=2)+"\n")
print(json.dumps(report["summary"], ensure_ascii=False, indent=2))
PY
