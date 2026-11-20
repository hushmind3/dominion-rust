# Sub4 Retired Subsystem and Dependency Audit — 2026-11-20

## Scope

Examined all **208 tracked blobs** on the Sub4 Git tree at the start of this audit: 81 Rust files, 52 React/TypeScript application files, 19 docs, and 56 project/config/CI/SDK/Cloud/test files. Every tracked blob was searched by content for the retired video, media, Money and RSI vocabulary, not only by filename. Direct native execution callers, recovery, policy, frontend route contracts, SDK, model tooling, package manifests and GitHub CI were then inspected in detail.

This is a tracked-source audit; it does **not** delete external caches, old Git commits, production services, databases, user workspaces, secrets or installed global apps. No paid-provider call or manual desktop build was performed.

## Actual executable remnants removed

- **RSI-era experimental executor:** deleted `compare_strategies` model tool, separate strategy comparison/admission/approval dispatch, two-candidate trial launch and report code, retired parent/child strategy-candidate restrictions, extra budget toggle and its environment variable. Generic `spawn_subagent`, native Run, repository worktree, provider selection, deterministic verification, receipts and ordinary approval remain.
- **Run restoration boundary:** preserved legacy Run JSON compatibility via serde's unknown-field behavior; archived experiment event families are explicitly blocked from restart/resume and their already-started child expense can settle without restarting an orphaned experiment parent. Durable records are not deleted or forged into successful jobs.
- **RSI-era frontend:** removed unused optimizer/experiment/algorithm route families and empty metrics displays. Memory shows real execution usage, recorded agent performance and independently verified recovery evidence from the native journal; no synthetic optimization dashboard.
- **Media UI leftovers:** removed legacy video-job labels, video-specific translation errors, stale E2E checks/deep links and dead wording. Google account labels represent the connected Workspace features rather than nonexistent built-in video production.
- **Money remnants:** none of the present Rust, React, Cloud or SDK executable files was a Money implementation. Previous cleanup removed the client and data collectors; these remain absent. Deleted dated docs that still advertised removed features.

## Abandoned installation and packaging

- Removed 7 package scripts referring to missing desktop/build or E2E targets, the unconfigured `@tauri-apps/cli` package, and the 12 associated platform package records from the lockfile.
- A normal `npm ci --prefix web` remains the frontend install path. No Tauri desktop app is claimed until an actual, tested packaging configuration exists.
- Subsequent cleanup removed the unmounted SQLite importer and archive, SDK wrappers, stale API routes and SQLite-only dependencies. Current encrypted backups, native Run history, credentials and stored user data are not deleted.
- Retained `/api` UI compatibility, `/api/v1` typed SDK, MCP, Google Workspace, GitHub, Cloudflare Worker billing and local model support: these still have real consumers and are not embedded video/Money/RSI runtimes.

## Deliberately retained, not a retired optimizer

- `src/strategy.rs` + `src/strategy/transfer.rs`: verified same-resource failure/recovery evidence, stored only with explicit opt-in; no training or automatic model/source changes.
- `src/repair_guidance.rs`, `src/knowledge.rs` and frontend memory: ordinary user-owned memory, feedback and reproducible repair hints.
- `src/sandbox.rs`: approval-gated native shell isolation for real agent work, not historical RSI scoring.
- `scripts/check-sub4-architecture.sh`: tests **must** mention prohibited retired names to prevent accidentally reintroducing them; these mentions are assertions, not working modules.
- The current Git history still includes older commits; the checked-out Sub4 branch's tracked source no longer contains their retired implementations.

## Verification

The development CI checks Rust targets/unit regressions, TypeScript, SDK contracts, UI tests, Cloud Worker tests and architecture guards. Production builds/Chromium are separately dispatched release checks. The specific CI run associated with the cleanup must be inspected before marking the result successful; this document is source provenance, not a substitute for a green run.

## 2026-11-21 — Follow-up dead-source removal (Sub4)

The latest all-target CI warning log identified unused runtime functions, unmounted JSON stubs and redundant internal projections. Deleted the dead HTTP `model_runtime`, `model_installed`, `channels` stubs, unused Engine `from_ports` wrapper (typed service/loop injection remains), unused App Server `consult` wrapper, unused cache-marker shim and dead RoomWorkBoard helper methods and top-level field. Projection and cache tests now inspect actual request/message representations rather than dead compatibility wrappers. Registered two channel permission/projection regression functions that were not being executed. Historic archived-experiment guards remain necessary to prevent replays. No active API endpoint, model provider, UI menu, user data or native Run implementation was removed.
