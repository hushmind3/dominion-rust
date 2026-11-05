#!/usr/bin/env bash
# Runs one real-model (qwen3:4b via Ollama) adaptation trace against an
# unknown-cause tool failure, then restarts the engine on the same data and
# verifies the adaptation checkpoint survived.
#
# Usage: scripts/real-adaptation-trace.sh [output.json]
set -uo pipefail
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${DOMINION_BIN:-$ROOT_DIR/target/release/dominion-rust}"
OUT="${1:-$ROOT_DIR/benchmark/ADAPTATION-TRACE-$(date +%F).json}"
WORK=$(mktemp -d "${TMPDIR:-/tmp}/dominion-real-adapt.XXXXXX")
DATA="$WORK/runtime"
[ -x "$BIN" ] || { echo "missing binary: $BIN" >&2; exit 2; }

start_engine() {
  local port
  port=$(python3 -c 'import socket;s=socket.socket();s.bind(("127.0.0.1",0));print(s.getsockname()[1]);s.close()')
  DOMINION_PORT="$port" DOMINION_DATA="$DATA" DOMINION_WEB="$WORK/web" \
    DOMINION_WORKSPACE="$WORK" DOMINION_BENCHMARK=1 \
    DOMINION_EXPERIMENT_SCHEDULER=0 DOMINION_BACKUP_INTERVAL_SECS=0 \
    "$BIN" >"$WORK/server-$port.log" 2>&1 &
  ENGINE_PID=$!
  for _ in $(seq 1 120); do
    curl -sf "http://127.0.0.1:$port/api/health" >/dev/null && break
    sleep 0.1
  done
  PORT="$port"
}

stop_engine() {
  kill "${ENGINE_PID:-0}" 2>/dev/null
  wait "${ENGINE_PID:-0}" 2>/dev/null
}

start_engine
curl -sf -X POST "http://127.0.0.1:$PORT/api/providers" -H 'Content-Type: application/json' \
  -d '{"name":"ollama-qwen3","baseUrl":"http://127.0.0.1:11434/v1","model":"qwen3:4b","enabled":true,"toolMode":"native"}' >/dev/null
curl -sf -X POST "http://127.0.0.1:$PORT/api/policy" -H 'Content-Type: application/json' \
  -d '{"autoApproveAll":true}' >/dev/null
curl -s -N --max-time 900 -X POST "http://127.0.0.1:$PORT/api/chat" -H 'Content-Type: application/json' \
  -d '{"input":"먼저 workspace_read 도구로 missing-report.txt 파일을 읽어라. 파일이 없으면 실제 원인을 확인하고, 같은 실패를 반복하지 말고 필요한 파일을 만들어 작업을 완료해.","autoLearning":false}' \
  >"$WORK/chat.sse"
curl -s "http://127.0.0.1:$PORT/api/executions" >"$WORK/executions.json"
# Fetch operations per execution using the live port.
python3 - "$PORT" "$WORK/executions.json" >"$WORK/operations.json" <<'PY'
import json,sys,urllib.request
port,path=sys.argv[1],sys.argv[2]
executions=json.load(open(path))
out={}
for execution in executions:
    with urllib.request.urlopen(f"http://127.0.0.1:{port}/api/executions/{execution['id']}/operations") as response:
        out[execution["id"]]=json.load(response)
print(json.dumps(out))
PY
stop_engine

# Restart on the same data directory and confirm the adaptation state persisted.
start_engine
curl -s "http://127.0.0.1:$PORT/api/executions" >"$WORK/executions-after-restart.json"
stop_engine

mkdir -p "$(dirname "$OUT")"
python3 - "$WORK" "$OUT" <<'PY'
import json,sys,pathlib
work=pathlib.Path(sys.argv[1]); out=pathlib.Path(sys.argv[2])
executions=json.load(open(work/'executions.json'))
operations=json.load(open(work/'operations.json'))
after=json.load(open(work/'executions-after-restart.json'))
def adaptation_view(execution):
    return {
        "id": execution.get("id"),
        "status": execution.get("status"),
        "failureClass": execution.get("failureClass"),
        "adaptations": execution.get("adaptations") or [],
        "adaptationCheckpoint": execution.get("adaptationCheckpoint"),
        "lastAdaptationError": execution.get("lastAdaptationError"),
        "operations": operations.get(execution.get("id"), []),
    }
view=[adaptation_view(e) for e in executions]
after_view=[adaptation_view(e) for e in after]
model_adaptations=[a for e in view for a in e["adaptations"] if a.get("selected")]
refuted=[h for a in model_adaptations for h in (a.get("hypothesisResults") or []) if h.get("status")=="refuted"]
result={
    "generatedAt": __import__("datetime").datetime.now(__import__("datetime").timezone.utc).isoformat(),
    "model":"qwen3:4b",
    "provider":"ollama /v1/chat/completions",
    "task":"workspace_read missing-report.txt -> adaptation",
    "executions": view,
    "afterRestart": after_view,
    "assertions": {
        "hypothesesGenerated": any(a.get("hypotheses") for a in model_adaptations),
        "interventionSelected": bool(model_adaptations),
        "hypothesisRefuted": bool(refuted),
        "adaptationCheckpointPersisted": any(e.get("adaptationCheckpoint") for e in view),
        "adaptationCheckpointSurvivedRestart": any(e.get("adaptationCheckpoint") for e in after_view),
    },
}
out.write_text(json.dumps(result,ensure_ascii=False,indent=2)+"\n")
print(json.dumps(result["assertions"],ensure_ascii=False,indent=2))
for a in model_adaptations:
    print("selected:",a["selected"].get("tool"),a["selected"].get("id"))
    print("hypotheses:",[(h.get("id"),h.get("claim")) for h in (a.get("hypotheses") or [])])
    print("hypothesisResults:",a.get("hypothesisResults"))
    print("result:",a.get("result"))
print("trace:",out)
PY
