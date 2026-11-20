# DOMINION Desktop — Tauri 2

One Rust sidecar, one existing React/Vite UI, no duplicate agent loop.

Tauri starts the existing backend bound to 127.0.0.1:4317 with private
application data/workspaces, a random per-launch API access token, and an
independent random health challenge. A document-start script injects the
token only into the approved local origin. Webview JS receives no generic
shell permissions.

The desktop Cargo crate is separate from the core Rust workspace.
To package later, place a compiled target-suffixed Dominion backend under
src-tauri/binaries and stage web/dist through scripts/stage.mjs.
Neither CI, native compilation, bundling nor signing was performed.
