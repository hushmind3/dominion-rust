# Dominion Sub4 — repository-wide architecture and documentation audit
**Date:** 2026-11-18 · **Baseline branch:** `dominion-sub4` · **Inspected HEAD:** `0ebb085152f3c57127ed6bbcd49c5ff5d3d00c95`

## Scope and method

The complete Git tree was inventoried: **197 tracked files**, **85 Rust source files under `src/`** and **54 TS/TSX/CSS sources under `web/src/`**. Primary execution, policy, journals, provider, MCP, groups, API/SDK, OAuth, stores, key frontend pages, configuration, CI, source-size scripts and selected tests were read directly from the Sub4 commit. A repository tree inventory is not the same as a line-by-line penetration audit; secondary UI assets, every test assertion and runtime dependencies were not exhaustively dynamically exercised.

Main/Sub2/Sub3 branch tips were located for comparison (`main` `8e6929f2`; `dominion-sub2` `902e5d06`; `dominion-sub3` `6d8ed875`). GitHub reports no common ancestor for a direct `main...dominion-sub4` commit comparison; use specific file-level comparisons, not an invented unified diff.

## Native execution trace inspected

`src/sdk.rs` / `src/api.rs` / `src/frontend_api.rs` → `src/engine.rs` → `src/provider.rs` or admitted `src/tools.rs` / `src/mcp.rs` / `src/connected_tools.rs` → `src/policy.rs` + `src/tool_schema.rs` + `src/hooks.rs` → `src/journal.rs` / `src/store.rs` → `src/verification.rs` / explicit receipts → SSE and React/TypeScript SDK.

The startup path `src/main.rs` opens `Dominion::open` and shares this Engine. Native delegation lives inside `src/engine/delegation.rs`; `read_program` and `run_program` reuse the existing execution journal rather than creating a second persistent agent runtime. Groups are separate domain state, **not** replacements for Run authorization or side-effect handling.

## Findings and current disposition

| Priority | Evidence | Finding | Disposition |
| --- | --- | --- | --- |
| P1 | `README.md` (54,841 bytes; 330 long lines) | Accumulated dated notes obscure current behavior, mix previously unverified tests with now-passing targeted CI and suggest deprecated subsystems | **Corrected:** replaced with current system guide and honest feature/verification boundaries |
| P1 | `web/package.json` | Desktop script targets `scripts/prepare-desktop.mjs` and `scripts/build-desktop.mjs` do not exist in the tracked Sub4 tree. No shipping desktop package is established | **Open:** explicitly documented; do not advertise working desktop distribution |
| P1 | `src/config.rs` / `.env.example` | Two operational model-call gates are supported in Rust: separate planning and independent model audit; the previous paid trial path has been retired | **Updated 2026-11-20:** two live flags remain in example config |
| P2 | `.github/workflows/check.yml` | `cargo fmt --all` reformats runner files but does not reject an unformatted committed source tree | **Corrected:** `cargo fmt --all -- --check` in manual release workflow |
| P2 | `src/engine.rs`, `src/context.rs`, `src/group.rs`, `src/provider.rs` | Context contraction and API caches have model/provider-dependent effectiveness. Existing exact source and deterministic mock tests must not be marketed as measured token or long-run recall improvement | **Corrected:** README distinguishes actual provider usage, character projections and historical benchmark results |
| P2 | `src/frontend_rooms.rs`, `src/frontend_api/agents.rs`, `src/frontend_extensions.rs`, `src/frontend_api/sdk_endpoints.rs` | Room-scoped tools/effort, per-agent reasoning, stdio MCP registration and arbitrary verifier commands return explicit unsupported errors | **Recorded:** real product limitations, not fabricated feature parity |
| P2 | `src/engine.rs` (69,926 bytes), `web/src/pages/Rooms.tsx` (59,977 bytes) | Large responsibility concentrations remain, although scheduling/delegation and many API/UI components are split already | **Open:** only split after profiling change ownership and regression invariants; file size alone is not a correctness failure |
| P3 | `docs/ARCHITECTURE.md` (historical audit note) | Some dated paragraphs still describe historical removed source files or an incomplete `/api/*` route adapter that was implemented later | **Resolved 2026-11-20:** stale feature-parity documents removed, active architecture updated |
| P3 | `src/legacy_import.rs`, `src/legacy_archive.rs`, `src/frontend_system.rs` | Legacy import and backup are separate explicit data operations; they do not imply live old-engine execution, atomic cross-store migration or transfer of credentials | **Verified boundary:** documented under compatibility and recovery |

## Targeted code evidence

- **Run safety:** `journal::begin` / `journal::finish`, exact tool call IDs, account mutation approval, uncertain remote outcomes, approval re-check and explicit reconciliation are represented in native modules. Cancellation and in-flight effects require the existing recovery tests; static inspection cannot establish all real-world external guarantees.
- **Prompt reuse:** `context::compact_strict` keeps short transcripts exact; optional references now append as suffixes. `provider::Reply` records gross/cached/write/output tokens; Group room receipts similarly record provider reported usage. The server does not expose a universal persistent Codex WebSocket chain in this branch. **Do not equate fewer serialized characters with provider-billed savings.**
- **MCP:** `src/mcp.rs` honors 2026-07-28 `tools/list` announced TTL and authorization context, with per-server singleflight on cold refresh. Tool discovery is permission-scoped; the schema/tool-call execution still uses the existing Run. It is limited to validated Streamable HTTP, not arbitrary stdio.
- **Frontend:** Source for `web/src/App.tsx`, Chat, Rooms, Models, Connections, Operations, key UI components and the corresponding `src/frontend_api/` and `src/frontend_rooms.rs` routes was inspected. UI existence alone does not validate every click and provider connector; browser E2E remains a release gate.

## Test and release evidence

- Targeted Rust workflow at **`0ebb0851`**: [successful run 44284747401](https://github.com/hushmind3/dominion-rust/actions/runs/44284747401) (all reported steps succeeded).
- Prior MCP singleflight change **`37d24f50`**: [successful targeted run 44283384198](https://github.com/hushmind3/dominion-rust/actions/runs/44283384198).
- **Not run for this documentation pass:** new paid-model benchmarks, Windows/macOS release workflow, browser E2E, full TypeScript suite, desktop packaging.
- Documentation and manual workflow changes do **not** prove test execution. Manual platform workflow now checks Rust formatting rather than quietly rewriting it.

## Next evidence-driven work

1. Repair or remove the dangling desktop packaging entrypoints only after the intended Tauri bundle and runtime layout are chosen; prove macOS/Windows artifact installation, not just npm script existence.
2. Measure 9-speaker and single-agent 100-turn recall plus gross/cached/noncached/write-token usage, context rebases, verification success and KRW model cost on identical model/provider budgets. Include canceled/failed outcomes.
3. Follow a full UI → SDK → native tool → approval → artifact → recovery path for GitHub and Google implementation; add browser E2E under explicit user-approved test scope.
4. Reduce the identified high-coupling Rust/React modules by moving a single well-defined responsibility per change, retaining precise regression tests; avoid duplicating the Run scheduler.

**Audit claims are source-grounded, not a full security certification or production-grade model benchmark.**
