# Sub2 · 2026-11-14: 2026 open-source agent SDK research + executable Copilot SDK baseline

**Scope:** Rust product is still the existing Dominion engine. This change adds an *independent, executable* official GitHub Copilot SDK baseline using the exact same three real file-artifact tasks and byte-level oracle as `scripts/sub2-goal-eval.mjs`. It does **not** replace the production loop yet. No paid model call, live pass rate, invoice, or new judge score is claimed.

## Verified public SDKs (checked 2026-11-14)

| Available source | Relevant reusable capability | Integration/usage consideration |
| --- | --- | --- |
| [GitHub Copilot SDK](https://github.com/github/copilot-sdk), [GA 2026-06-02](https://github.blog/changelog/2026-06-02-copilot-sdk-is-now-generally-available/) | Production-tested coding-agent runtime, tool calls, planning, session state, Rust + Node support, multi-agent work; BYOK for [Ollama/OpenAI-compatible](https://github.com/github/copilot-sdk/blob/main/docs/auth/byok.md) | Open-source MIT SDK; usage may be charged. Requires runtime and model service. Rust SDK requires Rust 1.94+. Node SDK v1.0.33 pinned in separate optional benchmark package. |
| [Rig (Rust)](https://github.com/0xPlaygrounds/rig), [agent run protocol](https://github.com/0xPlaygrounds/rig/blob/main/crates/rig-agent/README.md) | Native provider-agnostic agent runtime; steppable, serializable `AgentRun`; tool registry; replayable effects | Compare `AgentRun` and `rig-cassette` to Dominion's handwritten planner/dispatcher + SQLite effects; preserve approvals & test fixtures, do not blindly replace. |
| [OpenCode SDK](https://opencode.ai/docs/build/sdk) | Embedded agent server with sessions, prompts, plugins | TS runtime/Bun packaging and workspace isolation must be evaluated before adopting into Rust app |
| [Claude Agent SDK](https://github.com/anthropics/claude-agent-sdk-typescript) | Codebase understanding, edits, commands, session recovery via Claude Code harness | Node/CLI coupling, provider/usage constraints, review of licensing and applicable keys |
| [OpenAI Agents SDK](https://openai.github.io/openai-agents-js/) | Multi-agent handoffs, tools, trace, guardrails, sessions | Different runtime/language; model billing and host tool ownership remain |

### Critical technical choice

Use an **independent SDK-run baseline first**, not a speculative full replacement. A SDK name and feature list are not performance evidence; a production integration must prove actual file changes, provenance, safe permissions, cost attribution, and recovery on the **same task manifest**. The existing user-facing `/goal` remains just a chat slash command and has no new menu.

## Actual files and call path

```text
scripts/sub2-goal-eval.mjs
    DEFAULT_CASES (3 immutable goal definitions)
    render / safeRelativePath / oracle
     |                                      |
     v                                      v
Dominion's native HTTP+Rust runner   Official Copilot SDK 1.0.33
native /api/goals + current receipt   scripts/copilot-sdk-baseline/runner.mjs
+ attributable model usage            CopilotClient.start()
                                     createSession(provider=BYOK Ollama)
                                     onPermissionRequest -> only read/write
                                     inside each disposable case dir
                                     sendAndWait(task) -> model/tool events
                                     assistant.usage + tool.execution_*
                                     bytes checked by the SAME oracle
                                     both runs write JSON into results/
```

SDK benchmark is opt-in and requires the same `.dominion-benchmark-sentinel` and existing `workspace/` and `data/` folders as the native benchmark. The SDK runner does *not* require a Dominion API server. It creates unique per-case paths, checks no symlink/escape, blocks shell/network/MCP, and denies managed-policy autoapprovals. It refuses non-loopback or Copilot-account inference without `DOMINION_EVAL_ALLOW_PAID=1`. Local Ollama via BYOK requires only the SDK runtime plus the local model endpoint. Node module loads **only after** the safety checks.

The SDK runner counts per-model-call `assistant.usage` events (input/output/cache token fields), executed tool calls, independent artifact oracle, and a soft token limit and deadline; it does not infer a bill from the Copilot premium request multiplier. Missing model usage => unverified, even when a target file exists. Failed attempts stay in aggregate cost per verified completion.

### Run once manually (no fake successes in CI)

```bash
# Create an expendable environment OUTSIDE your source tree
ROOT="$(mktemp -d /tmp/dominion-agent-compare-XXXXXX)"
mkdir -p "$ROOT/workspace" "$ROOT/data"
printf 'benchmark\n' > "$ROOT/.dominion-benchmark-sentinel"

# SDK is installable and open-source; models can still cost money.
npm install --prefix scripts/copilot-sdk-baseline
DOMINION_EVAL_ENABLE=1 \
DOMINION_EVAL_ROOT="$ROOT" \
DOMINION_SDK_MODEL="qwen3:8b" \
DOMINION_SDK_BASE_URL="http://127.0.0.1:11434/v1" \
node scripts/copilot-sdk-baseline/runner.mjs

# Native Dominion comparison: run a SEPARATE benchmark-mode instance,
# with root/data and root/workspace, then:
DOMINION_EVAL_ENABLE=1 \
DOMINION_EVAL_ROOT="$ROOT" \
DOMINION_EVAL_BASE="http://127.0.0.1:4317" \
DOMINION_EVAL_PROVIDER_ID="<your-local-provider-id>" \
DOMINION_EVAL_MODEL="qwen3:8b" \
node scripts/sub2-goal-eval.mjs
```

**Do not count a missing run as 0/3 success nor the benchmark installation as SDK-powered production.** Two runs compare only when case IDs, instructions and model deployment are matched. Target SDK works with a local OpenAI-compatible server only if that model can actually complete the SDK tool schema; model-level failures are observed, not silently rerouted.

### Tests and limitations

- CI: `node --check scripts/copilot-sdk-baseline/runner.mjs`; `node --test scripts/copilot-sdk-baseline/runner.test.mjs` checks zero-cost authorization, symlink/path traversal, refusal of shell/network/MCP, usage absence and failed-attempt accounting.
- SDK real invocation: **not run** without an authorized local model or explicit paid budget.
- A permission callback is not a process/OS sandbox. All work is confined to an explicit disposable benchmark root and no SDK shell tool is exposed. For running untrusted code, future product adoption needs OS-level sandboxing and a human-approval bridge.
- No fake claims of user task completion, pricing, SDK superiority, memory improvement, full autonomous software engineering, or production-grade isolation.
- The separately scoped Node SDK package is **not part of the production React UI**, avoiding a second agent implementation in the main build without measurable evidence.

## Integration gate

Only after both SDK and native runs produce comparable `pass@1`, bounded token/cost receipts, recovery and file-proof traces should Dominion choose whether to replace its internal agent loop with SDK `AgentRun`, use a Copilot SDK runtime adapter as an optional backend, or retain native Rust orchestration. `Rig` offers a Rust-native alternative with pure steppable loop + cassette; its semantics should be compared directly with the existing `jobs.rs` and `verification.rs`.

The higher score previously assigned from CI/static source connections is **not** evidence that the implementation is already competitive with Codex/Claude Code. We explicitly separate verified static functionality, deterministic mock tests, and measured real-model delivery.
