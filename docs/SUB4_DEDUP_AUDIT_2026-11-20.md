# Sub4 source-duplication audit and consolidation — 2026-11-20

## Scope and evidence

At the start of this pass, `dominion-sub4@ca872ff6` contained **206 tracked blobs**
in the Git tree, including **80 Rust sources under `src/`** and the React/Vite UI,
TypeScript SDK, Cloud Worker, tests, configuration, and documentation.
The native trace was checked from `src/api.rs` and `src/frontend_api.rs`
through `src/engine.rs`, `src/frontend_rooms.rs`, `src/group.rs`,
`src/provider.rs`, `src/connected_tools.rs`, `src/journal.rs`, and `src/store.rs`.
Focused route/source scans covered model discovery and downloads, frontend account
and model screens, connected tools, OAuth, channels, CI and SDK contracts.

A complete Git tree inventory is **not** a claim that every line, edge case, or
external credential flow was dynamically tested. Paid-provider integration and
live-model token benchmarks were not run for this refactor.

## Duplicate behavior removed

1. **One room owner admission.** `src/frontend_rooms.rs` formerly repeated
   message validation, display-name recipient lookup, and `say_once` across
   synchronous and background HTTP methods. Both now enter
   `admit_owner_message` before selecting their execution transport. A
   scoped 9-speaker regression covers rejected reasoning settings, invalid
   participants, successful targeted routing and exact model-call count.
2. **Stable fanout state.** Per-owner-turn agent configuration is read once
   instead of once per speaker, while provider credentials continue to resolve
   at dispatch. For rooms without auto-learning, scoped knowledge is projected
   once per owner turn; rooms that append auto-learning notes still refresh
   between speakers. Synchronous sends no longer make a redundant outbound
   channel-dispatch scan after `generate` already dispatched replies.
3. **One bounded public JSON reader.** Hugging Face metadata and models.dev /
   OpenRouter public catalog loading now use `src/frontend_public_http.rs`
   for pooled, redirect-disabled HTTP GETs, the existing per-request timeout,
   preflight Content-Length checks, streaming byte caps and JSON parsing.
   Existing error categories and distinct cache TTLs remain with the caller.
4. **One Hub repository/path validation base.** The model-download adapter
   reuses Hugging Face's validated repo ID and safe relative-path checks while
   keeping its additional per-component, file-size and control-character
   restrictions. Tests cover the stricter importer boundary.
5. **One provider catalog UI flow.** The two overlapping vendor discovery /
   connection tabs were consolidated into the existing searchable
   `DominionCloudAccount` vendor-connection flow. Hugging Face file search,
   installed/local models, free-model status, OAuth and BYOK remain separate
   because they have distinct functions. A source contract test rejects a
   second vendor catalog page and confirms that public lookup is still offered.

## Intentional adapters retained — not duplicate executors

- `/api/v1/*` typed SDK and `/api/*` main-UI compatibility routes are
  different wire contracts over the **same** Rust Engine, journal and policy.
  Removing either without replacing its clients would be a regression.
- Room dialogue orchestration (`group.rs`) and authorized tool-agent Runs
  (`engine.rs`) have distinct domains. Neither is a second tool execution
  scheduler.
- Native model activation, Hugging Face metadata, persistent downloads, and
  provider registration expose different state ownership. Their public reads
  have been shared only where equivalent.
- `read_program` and `run_program` have different approval/side-effect
  boundaries. Neither should bypass the existing Run policy/journal.
- `cloud/src/index.js` is a Cloudflare account/billing/inference Worker;
  `services/dominion-cloud/gateway-contract.ts` defines a contract, not a
  second active inference Worker. Deployment is separate from source CI.
- Legacy archive/import adapters preserve existing user records; they do not
  reactivate the old standalone Money implementation.

## Regression and follow-up boundaries

- `scripts/check-sub4-architecture.sh` now enforces exactly one owner-message
  admission, one `say_once` call site, one bounded public JSON reader,
  and absence of a resurrected Money/old engine module.
- Development CI runs Rust compile/unit coverage, browser-source contracts,
  SDK checks and Worker tests on changes. Explicitly dispatched release
  workflows own production bundling and real Chromium E2E; this pass does not
  claim a desktop release or live multi-provider benchmark.
- The large `src/group.rs`, `src/engine.rs`, `ModelExplorer.tsx` and Cloud
  Worker deserve future measured decomposition. **Size alone is not proof of
  duplicate scheduling or inefficient execution.** Split only responsibilities
  with regression invariants and no lost receipts.
