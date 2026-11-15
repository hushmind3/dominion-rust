# Sub4 frontend alignment — 2026-11-15

## Source of truth
- Repository: `hushmind3/dominion-rust`
- Latest verified main HEAD when aligned: `879dc23dce309688691c200b001b180bead95a12`
- Main web tree SHA: `9e2be9b7376c1eefb6195e3ea7f9463b952456c0`
- The **entire** `web/` Git tree in `dominion-sub4` is copied from that exact latest main tree, including the original `App.tsx` routing, Chat/Models/Video/Rooms/RSI pages, Home, CSS, Vite configuration, package scripts and lockfile.
- This is **tree identity**, not a visually similar reconstruction or a partial cherry-pick. All added sub4-only files inside `web/` are removed, including `RustWorkspace.tsx`, experimental E2E assets and frontend-source marker. No UI feature is fabricated.

## Core and UI contract: currently different
- This restoration is intentionally **frontend-only**. The independent Rust engine, agent/RSI algorithms and the HTTP/TypeScript/Rust SDK in `src/` and `sdk/` remain untouched.
- The original main UI uses a broad `/api/*` service contract (for example `/api/sessions`, `/api/agents`, `/api/providers`, `/api/rooms`, `/api/rsi/*`, `/api/video/*`).
- Sub4 currently exposes a different `/api/v1/*` native Rust contract. The upstream Vite proxy defaults to port `4317`, while sub4's Rust server uses port `8787`. Main's package scripts also target its own binary's `--home` / `--open` flags, not sub4's bare server.
- **Do not represent this tree sync as full UI/backend functional parity.** Main frontend components expecting legacy endpoints cannot work against the current sub4 Rust API until those specific backend contracts are genuinely implemented. The source checkout, production bundle output and Rust engine have not been run during this commit.
- Future integration work must preserve exact main UI semantics and page hierarchy and **adapt the Rust backend** to real UI contracts, instead of replacing the frontend with simplified or dummy 'working' pages. Required APIs should be backed by durable Rust state/operations, not fake 200 responses or mocked examples.

## Removal of previous sub4 UI substitutions
The sub4 `RustWorkspace` integrated replacement, extra frontend-only file additions, invented home execution cards, changed owner profile editor, and modified memory page were removed from `web/` in favor of upstream main source. The underlying Rust memory/profile/agent/strategy implementations remain in `src/` and `sdk/`, available to integrate correctly later.

## Verification
- Git object comparison: main `web` tree and staged sub4 `web` tree both `9e2be9b7376c1eefb6195e3ea7f9463b952456c0` (exact file-level identity).
- No compilation, browser test, CI or main-runtime behavior was executed in this change, per the user's direction.
