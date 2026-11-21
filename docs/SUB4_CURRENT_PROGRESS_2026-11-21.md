# dominion-sub4 — current implementation and verification (2026-11-21)

**Authoritative task source:** the user's newest uploaded `서브.md`, heading `# dominion-sub4 구현 지시문`, together with the newest uploaded `공통지시문.md`. Historical repository documents (including the 2026-11-20 token audit) provide earlier measurements, **not active development instructions**. Only `dominion-sub4` was edited. No main branch, independent branches, Cloudflare deployment or production database was changed.

## P0: Run/Chat/Room/Team correctness

- Native Chat model-only and tool-capable Run share the original Engine scheduler, persistence, cancellation and receipt paths. Model-only requests persist `model_only_request_intent` before a potentially billable send and block automatic replay of an uncertain prior attempt. An effective-budget failure is never overwritten as completion; paused SSE closes with the original Run identity for recovery.
- `no_tools_required` covers multi-word Korean greetings, complete explanation questions and avoids the `우주가` / `주가` false-positive while preserving named files, search, prior work, attachments, approvals and external actions on the tool-capable path.
- The first-party model transport admits per-Run options only for explicitly supported combinations: Ollama Qwen3/DeepSeek-R1 thinking disable, Ollama gpt-oss low/medium/high, and official OpenAI GPT-5.6/GPT-6 low/medium/high. The UI queries `/api/chat/inference-support` and disables unsupported controls. Supported settings are frozen into the original Run and applied to the final wire request; local Ollama payload capture and unsupported-combination regressions are included. This is not a universal claim of provider support.
- Existing independent remote Worker authorization, tool/Writer distinction, bounded responses, separate charge receipts, exact parent/child Run identity and SHA-pinned Writer patch review are preserved; result flags now differentiate Inspector versus Writer accurately.

## P1: context, source-indexed Room recall and observed costs

- Chat role-preserving history and Room sealed owner evidence retain source indices. New unit cases cover 3/9 participants, 10/100 owner turns, deterministic restart, old owner quotes and non-promotion of archived peer statements.
- The original Room and provider implementation were **split without changing their outward contracts**: `src/product/group.rs` (891 lines), `group/context.rs` (290), Room test modules (632 and 448); `src/infrastructure/model_transport.rs` (834), `model_transport/contract.rs` (116), transport tests (573). The project-wide source budget of 900 lines per file passes.
- Fresh same-environment **deterministic mock** Chat HTTP/Chromium measurement from CI run [44893329607](https://github.com/hushmind3/dominion-rust/actions/runs/44893329607):

| Metric | `안녕` | `안녕하세요 반가워요` |
|---|---:|---:|
| Serialized message JSON characters | 502 | 510 |
| Tool schemas | 0 | 0 |
| Mock supplier-reported input tokens | 40 | 40 |
| Full HTTP response elapsed, ms | 19 | 18 |

These are fixture values in freshly created sessions, **not real-model TTFT**, not actual paid token billing, and not a temperature/hardware-controlled quality comparison. The earlier Workers AI three-pair and historical Qwen3.5 llama.cpp CPU cold/warm result remain in the dated audit only. The current branch **has not run** a newly controlled old/new real-model inference A/B, first-party paid cache-billing experiment, or test on the user's GPU. Do not import historical numbers as fresh measurements.

## P2: source-linked experience

- Existing Run workspace isolates file preview and page state by exact Run ID; uncertain model-only billing cannot advertise a safe Resume; tool approval and Writer SHA review remain distinct.
- Task Team creation now captures operator-selected per-Worker model routes with valid task IDs, exact model selection and immutable Run-bound routing. Team board shows actual saved Worker model, dependencies and receipt acceptance state. Existing single native execution loop is unchanged.
- Room UI identifies the failed speaker/model and links to the original owner message. Existing participant usage receipts and pending-speaker display remain intact.
- 360–375px Chromium scenarios cover reopening exact persisted Run evidence and the visible approval/denial buttons. Only the approval GET response in the test is simulated; actual buttons are not clicked and no external write is claimed.

## Verified tests and limits

[GitHub Actions: **44893329607**, success](https://github.com/hushmind3/dominion-rust/actions/runs/44893329607), on commit `193bcf38b65bb66b7171f381f50af08e6d0927e0`:
- `bash scripts/check-sub4-architecture.sh`: PASS.
- `node scripts/check-source-size.mjs`: 165 source files scanned, no over-900-line failure.
- `cargo check --locked --all-targets`: PASS.
- `cargo test --locked --workspace --lib`: 367 Dominion tests and independent core/algorithm package suites PASS.
- TypeScript, SDK/endpoint tests, production frontend bundle and UI tests: PASS.
- Isolated Rust backend + Vite + real Chromium: **14/14 browser integration specs PASS**, including `sub4-run-ux.spec.ts` and `sub4-chat-live.spec.ts`. Ports were isolated from production services.
- Existing Qwen3.5 real-GGUF CI probe: conditional and **skipped** on this run. A browser with a narrow viewport is not a physical Android/iOS device.

**Remaining measurement-only scope:** Fresh controlled real-model old/new A/B on identical model/hardware/scripts, physical-device test and independently metered paid-provider cache/TTFT receipts. These cannot be marked completed from fixture results. No production rollout was performed.


## 2026-11-21: industry-practice hardening after branch comparison

External branch assessment: `main` was provisionally rated 85/100, `dominion-sub4` 82/100. These are comparative reviewer scores, not official certifications and **not automatically recalculated** by this change. All implementation below was made **only** to `dominion-sub4`.

### Added durable, at-most-once agent inference gate

Previously the native model-only chat had a send-intent fence, but agent planning, ordinary model turns and optional verification did not have the same persisted charge boundary. Each stage now saves a `model_request_intent` before dispatching a potentially billed provider request, then records `model_request_settled` (exact `intent_seq`), supplier receipt tokens and cost in one atomic Run store update. On interrupted/disconnected requests the original Run stays paused and cannot auto-replay that unresolved upstream billing. **This is not provider-side exactly-once billing**, nor evidence that any absent receipt was free.

- `src/journal.rs`: `unsettled_model_request`; recovery retains unresolved source intent and emits `model_outcome_unknown`.
- `src/engine.rs`: `Engine::launch` rejects replay of a Run with an unresolved paid agent request.
- `src/harness/runner.rs`: applies the same intent/settlement contract to planning, agent turns and independent verifier.
- `src/engine/task_team_tests.rs`: checks exact receipt linkage and restart recovery.
- `src/engine/model_only_tests.rs`: loopback HTTP 503 received *after server accepts the request* tests that resume does not submit a second paid agent request.
- `src/product/frontend_api/chat.rs` and existing React Chat/Run inspector: preserve original Run ID in failure events, show paused uncertain provider request, provide source-linked Run inspection in the existing chat composer, and suppress unsafe Resume.
- Mobile regression specifically checks the existing recovery action can be clicked on 360px layout without overlaps; the first run exposed an overlapping header button, then the action was moved to the composer and the regression passed.

### MCP, CI and source quality

- Existing `2026-07-28` stateless Streamable HTTP MCP remains in place. The HTTP transport tests now require version headers, `Mcp-Method`, `Mcp-Name` only for named calls, request-local `_meta`, and no old `initialize` call or `Mcp-Session-Id`.
- CI enforces Clippy **correctness** lint group on Rust workspace/all targets. This is deliberately *not* represented as an all-Clippy-warnings-clean or `cargo fmt --check` pass.
- Every source-scoped push now builds the React production bundle and executes real Rust HTTP + Vite + Chromium E2E automatically following frontend/Rust checks, without deploying.
- Historical source-size gate remains enforced: executable modules 900 lines max.

### Verified result

**[GitHub Actions run 44897981841 — success](https://github.com/hushmind3/dominion-rust/actions/runs/44897981841)**, code commit `33f143cb023c7823853b389af90e2147ec2160cb`:
- `cargo check --locked --all-targets`: successful.
- `cargo clippy --locked --workspace --all-targets -- -D clippy::correctness`: successful.
- `cargo test --locked --workspace --lib`: 370 native Dominion tests passing, 3 ignored, plus separate package suites passing.
- Strict TypeScript, SDK contracts, product build, source-size and architecture checks: successful.
- **Real Chromium E2E: 15 of 15 passing**, including a 360px recovery/approval accessibility path. Mock-provider fixture is explicitly not real paid-model performance measurement.
- The existing optional Qwen3.5 cold/warm GGUF job was not run in this workflow.

Outstanding independent targets: rustfmt baseline migration and a possible enforced `cargo fmt --check` gate; real stdio/legacy MCP interoperability; physical Android/iOS browser testing; newly controlled actual paid-provider/cache and local GPU throughput receipts. No upstream `main` merge or production deployment was performed.
