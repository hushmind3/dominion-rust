# Sub4 single-harness convergence — 2026-11-18

## Decision: improve ONE native Run, do not merge four runtime loops

Branch source review (all from `hushmind3/dominion-rust`, snapshots inspected on 2026-11-18):
- `main` `d8f6da21`: `src/engine/chat/runtime.rs` and `runtime/tool_catalog.rs` coordinate turns and use keyword-sensitive model-visible tool selection. It is the compatibility/UX baseline, not a second runtime to embed.
- `dominion-sub2` `0434216e`: `crates/dominion-agent-kernel/src/{lib,cycle,failure}.rs` and `src/engine/chat/runtime/{turn_body,tool_calls}.rs`. Strong model-independent effect uncertainty and bounded repair/review; own kernel must not be linked as another parallel state machine.
- `dominion-sub3` `3a1fb6d6`: `crates/dominion-agent-sdk/src/tool_discovery.rs` supplies permission-scoped progressive tool disclosure; `crates/harness/src/sdk.rs` and `crates/dominion-agent-sdk/src/runtime.rs` maintain separate library-level session/round contracts. Borrow the discovery *principle*, not the duplicate loops and checkpoints.
- `dominion-sub4` baseline `0621eae6`: `src/{engine,domain,journal,policy,tool_schema,mcp,tools}.rs` already own Run scheduling, schema admission, approvals, cancellation, retries, remote outcome uncertainty and durable recovery.

External implementations read:
- [Hermes tool search](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/user-guide/features/tool-search.md): dynamic MCP/plugin schemas behind search/describe/call, with catalog manifests and permission-scoped search. Discovery accuracy and extra cold-start turns are nontrivial tradeoffs.
- [OpenClaw ACPx](https://github.com/openclaw/openclaw/blob/main/extensions/acpx/README.md): ACP host retains gateway/session authority while an independently isolated external coding harness handles one delegated task.
- [OpenHands Agent SDK](https://github.com/OpenHands/software-agent-sdk): agent, execution and UI remain distinct contracts.

## Implemented in Sub4

`src/tool_discovery.rs` is a **pure catalog-projection and ranked-search algorithm**, *not* an executor. It adds:
1. When >8 currently permitted external tools are connected, advertise `tool_search` plus up to 3 intent-relevant original external schemas instead of all remote JSON parameter schemas. Small catalogs remain eagerly available. Relevant actions can start on the first turn without paid discovery, while all other tools remain discoverable.
2. Search **only the current operator-authorized Run catalog**, using weighted name/description/argument-name matches, smoothed corpus IDF and Korean/Chinese query aliases. Results include exact external names and required parameter keys, not action authority.
3. Record selected names only from successful, post-hook receipts, in the original durable Run journal (bounded 16; backward-compatible serde default). On the next model turn the selected original schemas become visible.
4. The model then invokes `mcp__...`/`github_...`/`google_...` directly. This uses the *existing* `tool_schema::admit`, policy, veto hooks, explicit approval, cancellation, write-ahead intent, outcome-unknown handling and recovery. No `tool_call` bridge dispatch or second scheduler can bypass those invariants.
5. Revocation, removed endpoints, disabled `tool_search`, new owner directives, and read-only child policy are checked before disclosure. The new discovery tool is side-effect free and replay-safe.

Existing `read_program` and `run_program` keep their bounded native-only semantics; no remote tool was smuggled into these paths. Built-in Skills remain lazy procedural references, not invisible executable permissions.

- Concurrent native Runs now coalesce a cold per-server `tools/list` request under a server-scoped asynchronous gate. Cache entries additionally carry the MCP configuration/authorization generation, so private tool metadata from an old token cannot reappear if a best-effort cache clear races with configuration replacement. Independent servers never wait behind a global HTTP listing lock.
- Added a real local HTTP fixture that launches 12 simultaneous callers, checks one request on first discovery and one on forced expiry, and simulates restoring an old private cache entry after bearer-token rotation. These are deterministic request-count regressions, **not** a claim of measured model-token or end-to-end latency improvement.

## Limits, measurements and next frontier

- `tool_search` adds a model round for the first cold deferred capability. This is a known tradeoff; compare task completion and full token cost in a fixed matrix before widening the threshold. The `schema_chars_saved` metric records external schema bytes omitted, *not* measured token reduction or quality gain.
- Our catalog retrieval currently still calls MCP `tools/list` on model turns, respecting server-announced TTL. This is network overhead outside the model tokens and can be optimized with protocol-owned freshness semantics.
- Externally delegated ACP execution is **not yet implemented**. A future adapter must provide session/cancellation/approval/budget/observed outcome contracts without linking another Run journal or assuming exactly-once remote effects.
- The ranking is deterministic and local. Its search quality, real model turns, and outcome gains remain to be measured using the same user goals, providers and budgets.

## Regressions

The `tool_discovery::tests` Rust suite covers hidden-schema call rejection before search, receipt-gated disclosure, original schema admission and argument validation, permission revocation, read-only isolation, removed endpoints, small eager catalogs, disabled search escape hatch, old Run serialization, bounded retention, and repeat searches.

`.github/workflows/sub4-dev-rust.yml` invokes `cargo test --lib tool_discovery::tests` on macOS development profile alongside existing engine/policy tests. Full release, frontend and paid-model benchmarks are separate.
