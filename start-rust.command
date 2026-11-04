#!/bin/zsh
# DOMINION — Rust backend launcher.
# Starts the already-built binary; builds only when sources are newer.
# Node is only used when the React bundle is missing (vite build).
set -e
cd "$(dirname "$0")"

PORT="${DOMINION_PORT:-4317}"
BIN="./target/release/dominion-rust"
source "$HOME/.cargo/env" 2>/dev/null || true

needs_build() {
  [ "${DOMINION_FORCE_BUILD:-0}" = "1" ] && return 0
  [ ! -x "$BIN" ] && return 0
  [ -n "$(find Cargo.toml Cargo.lock src -newer "$BIN" -print -quit 2>/dev/null)" ] && return 0
  return 1
}

if needs_build; then
  if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo를 찾을 수 없습니다. Rust 툴체인을 설치하세요: https://rustup.rs"
    exit 1
  fi
  echo "Building Rust server…"
  cargo build --release
else
  echo "Using prebuilt Rust server."
fi

if [ ! -f web/dist/index.html ] || [ "${DOMINION_REBUILD_WEB:-0}" = "1" ]; then
  if command -v npm >/dev/null 2>&1; then
    echo "Building React bundle…"
    (cd web && { [ -d node_modules ] || npm install --no-audit --no-fund; } && npm run build)
  else
    echo "web/dist 가 없고 npm도 없습니다. React 빌드가 필요합니다."
    exit 1
  fi
fi

echo "Starting Rust engine on http://127.0.0.1:$PORT"
DOMINION_PORT="$PORT" \
DOMINION_WEB="${DOMINION_WEB:-web/dist}" \
DOMINION_DATA="${DOMINION_DATA:-runtime}" \
exec "$BIN" --open
