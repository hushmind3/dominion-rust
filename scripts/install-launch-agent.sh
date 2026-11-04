#!/bin/zsh
# Installs a LaunchAgent so the Rust engine starts at login and restarts if it
# ever exits. Run once; after that the DOMINION.app icon only opens the UI.
set -e
REPO="$(cd "$(dirname "$0")/.." && pwd)"
PLIST="$HOME/Library/LaunchAgents/com.hushmind.dominion.rust.plist"
mkdir -p "$HOME/Library/LaunchAgents" "$HOME/Library/Logs"
cat > "$PLIST" <<PL
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>com.hushmind.dominion.rust</string>
  <key>ProgramArguments</key><array><string>$REPO/target/release/dominion-rust</string></array>
  <key>WorkingDirectory</key><string>$REPO</string>
  <key>EnvironmentVariables</key><dict>
    <key>DOMINION_HOME</key><string>$REPO</string>
    <key>DOMINION_PORT</key><string>4317</string>
    <key>DOMINION_DATA</key><string>$REPO/runtime</string>
    <key>DOMINION_WEB</key><string>$REPO/web/dist</string>
  </dict>
  <key>RunAtLoad</key><true/>
  <key>StandardOutPath</key><string>$HOME/Library/Logs/dominion-rust.log</string>
  <key>StandardErrorPath</key><string>$HOME/Library/Logs/dominion-rust.log</string>
</dict></plist>
PL
launchctl bootout "gui/$(id -u)/com.hushmind.dominion.rust" 2>/dev/null || true
launchctl bootstrap "gui/$(id -u)" "$PLIST"
echo "installed $PLIST"
