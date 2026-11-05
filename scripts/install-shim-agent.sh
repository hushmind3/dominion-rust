#!/bin/zsh
# Runs the Rust OpenCode Go gateway on 127.0.0.1:4399. Node is not involved.
set -e
REPO="$(cd "$(dirname "$0")/.." && pwd)"
PLIST="$HOME/Library/LaunchAgents/com.hushmind.dominion.shim.plist"
mkdir -p "$HOME/Library/LaunchAgents" "$HOME/Library/Logs"
cat > "$PLIST" <<PL
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>com.hushmind.dominion.shim</string>
  <key>ProgramArguments</key><array>
    <string>$REPO/target/release/dominion-rust</string>
    <string>--shim</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>3</integer>
  <key>StandardOutPath</key><string>$HOME/Library/Logs/dominion-shim.log</string>
  <key>StandardErrorPath</key><string>$HOME/Library/Logs/dominion-shim.log</string>
</dict></plist>
PL
launchctl bootout "gui/$(id -u)/ai.dominion.shim" 2>/dev/null || true
launchctl bootout "gui/$(id -u)/com.hushmind.dominion.shim" 2>/dev/null || true
PIDS=$(lsof -nP -iTCP:4399 -sTCP:LISTEN -t 2>/dev/null || true)
for pid in ${(f)PIDS}; do [ -n "$pid" ] && kill -9 "$pid" 2>/dev/null || true; done
sleep 1
launchctl bootstrap "gui/$(id -u)" "$PLIST"
echo "installed $PLIST"
