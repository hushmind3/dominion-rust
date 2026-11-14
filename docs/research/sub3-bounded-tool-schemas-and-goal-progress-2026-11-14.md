# Sub3 · Bounded tool schemas and evidence-based goal progress (2026-11-14)

## Why this change

Small-context providers previously received the first tool schemas that fitted the byte budget. Since native read/list tools precede edit/verify tools and MCP definitions are appended last, truncation could silently produce a *read-only* agent despite the user allowing edits and external services.

A separate autonomous-goal bug counted a newly phrased successful `code_search` or `workspace_read` observation as goal progress even when the goal required a file edit, install, run, or other action. The stall counter could reset on repeated browsing, delaying independent replanning.

## Real execution path

```text
POST /api/goals → jobs → goals::run
  → chat/runtime/turn.rs
    → native tools::definitions + mcp::definitions
    → tool_catalog::select_room_tools / select_tool_definitions
    → tool_catalog::trim_tool_definitions (capability-balanced, byte-bound)
    → provider::stream_completion → dispatched real tools
    → audit: tool.schema_budget { offeredBytes, selectedBytes, offeredCount, selectedCount, selectedTools }
  → verification::successful_evidence
  → goal_progress::advanced_with_policy(..., verification::looks_actionable(goal))
    → actionable task: closed deterministic gap OR new verified action evidence
    → research task: closed gap OR new successful observation
  → goals::run: stalled counter / replan / checkpoint / goal receipt
```

The schema trimmer operates **only on already authorized/selected tools**. It first reserves one available, budget-fitting tool for workspace observation, source mutation, command verification and search; it then reserves an available MCP tool and optional creation/delegation operations before filling remaining capacity in original order. It never increases permissions, reorders the resulting provider schema prefix, or exceeds the configured JSON-byte budget. An oversized individual tool schema is omitted instead of silently overrunning capacity.

The extra audit event uses exact *serialized JSON bytes*, which are not API-billed tokens. Source-of-truth provider token and cache metrics continue to be recorded separately. The bound is applied before specialized RSI stage menus, which intentionally restore only their permitted tools; no changes were made to the protected RSI evaluator/adoption boundary.

The goal policy reuses the same action-intent classifier as `verification::acceptance_gaps_in`. A newly successful search remains useful evidence for the reviewer, but does not itself reset the stalled counter for an actionable goal. Unknown or failed effects never qualify as progress. Explicitly closed acceptance checks still qualify.

## Regression tests

- `engine::chat::runtime::tool_catalog::schema_budget_tests`: read/edit/shell/search/MCP retention under a constrained budget, original-order stability, no overflow, original catalog unchanged when it fits, no introduction of filtered tools.
- `engine::goal_progress::tests`: successful action and acceptance-gap closure, read-only churn under actionable goals, research observations, failure/unknown responses.
- Test command on a Rust-equipped checkout: `cargo test --release schema_budget_tests` and `cargo test --release goal_progress::tests`. These tests have been committed; execution in the current environment was unavailable because `cargo`/`rustc` are not installed. GitHub Actions can provide an independent compilation/test result for the resulting branch commit.

## 2026 public technical basis

- [MCP Client Best Practices, 2026-07-28](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/docs/2026-07-28/develop/clients/client-best-practices.mdx): progressive tool discovery, stable cached schemas and bounded context. This patch is a conservative byte-budget capacity fix, **not** a claim of full lazy MCP schema loading.
- [GEPA-Curator, 2026-10-27](https://arxiv.org/abs/2610.37651): failure/experience curation distinguishes useful agent learning signals from noisy observations. This patch independently addresses noisy *goal-progress* observations; it does not claim equivalent benchmark performance.

## Remaining verification

Real provider cache hit rates, token bills, task pass rate, and tool-selection recall require actual model runs with matched tasks and a controlled token budget. No percentage saving or evaluation score is asserted by this source-only change.
