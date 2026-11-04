#!/bin/zsh
# DOMINION — Rust backend launcher.
# Builds the Rust server (and the React bundle when missing), starts it on
# 127.0.0.1:4317, then opens Chrome. Node is only used for `vite build`.
set -e
cd "$(dirname "$0")"

PORT="${DOMINION_PORT:-4317}"
source "$HOME/.cargo/env" 2>/dev/null || true

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo를 찾을 수 없습니다. Rust 툴체인을 설치하세요: https://rustup.rs"
  exit 1
fi

echo "Building Rust server…"
cargo build --release

if [ ! -f web/dist/index.html ]; then
  if command -v npm >/dev/null 2>&1; then
    echo "Building React bundle…"
    (cd web && { [ -d node_modules ] || npm install --no-audit --no-fund; } && npm run build)
  else
    echo "web/dist 가 없고 npm도 없습니다. React 빌드가 필요합니다."
    exit 1
  fi
fi

PIDS=$(lsof -nP -iTCP:"$PORT" -sTCP:LISTEN -t 2>/dev/null || true)
for pid in ${(f)PIDS}; do
  [ -n "$pid" ] && kill -9 "$pid" 2>/dev/null || true
done

echo "Starting Rust engine on http://127.0.0.1:$PORT"
DOMINION_PORT="$PORT" \
DOMINION_WEB="${DOMINION_WEB:-web/dist}" \
DOMINION_DATA="${DOMINION_DATA:-runtime}" \
nohup ./target/release/dominion-rust > /tmp/dominion-rust.log 2>&1 &

for _ in {1..40}; do
  sleep 0.25
  if curl -sf -m 2 "http://127.0.0.1:$PORT/api/health" >/dev/null 2>&1; then
    open -a "Google Chrome" "http://127.0.0.1:$PORT" 2>/dev/null || open "http://127.0.0.1:$PORT"
    echo "DOMINION (rust) is running: http://127.0.0.1:$PORT"
    exit 0
  fi
done

echo "서버가 시작되지 않았습니다. 로그: /tmp/dominion-rust.log"
tail -20 /tmp/dominion-rust.log
exit 1
