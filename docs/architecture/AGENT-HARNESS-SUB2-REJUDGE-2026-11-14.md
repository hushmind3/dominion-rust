# DOMINION Sub2 — 2026-11-14 rejudge follow-up: isolated goals and parallel investigation

Branch: `dominion-sub2`. The judge's latest **57/100** was a static score for the previous version, not a score for this follow-up. The previously confirmed GitHub CI on `ffcb6f6` completed 942 Rust tests, web typecheck and web build. **No build, test, CI, paid-provider session or benchmark has been run for the changes documented here.**

## Actual executable call graph

```text
Home / web/src/components/GoalConsole.tsx
  POST /api/goals { executor, goal, isolateWorktree, successCriteria, ... }
    → api/goals.rs::create_goal → jobs::enqueue("goal")
    → engine/goals.rs::run
       → (when opted in) goal_workspace.rs::prepare
          → validate clean source Git HEAD + top-level root
          → git worktree add --detach in canonical data_dir/goal-worktrees/<goalId>
          → persist gitWorktree {path, source, baseCommit} BEFORE any tool
       → create/reuse session {goalId}
       → project.rs::inspect(actual goal root)
       → code_navigation.rs::collect(actual goal root, indexed SHA-bound source)
       → verification.rs::baseline_checks_for_workspace(actual goal root)
       → plan → run_quiet_outcome → chat/runtime/turn.rs
          → dispatch.rs::dispatch_tool_call / deterministic_tool
             → goal_workspace.rs::workspace_for_session
             → tools::ToolContext.workspace = actual goal root
             → tools::execute / effect::seal (same root)
          → tools.rs::delegate_tasks → tools/batch.rs::delegate_many
             → up to 4 durable child jobs, wait or status polling
             → tools.rs::delegate(child delegatedReadOnly = true)
             → tool_catalog filter + native and MCP runtime denial for mutations
             → child goalId/root inherited across parentSessionId
       → verification.rs::acceptance_gaps_for_workspace(actual goal root)
       → reviewer only after deterministic checks → repair/replan
       → artifacts.rs::collect(actual root, successful file tool receipts)
       → receipts.rs::record (immutable, exact job attempt, workspace base)
       → episode result and original owner UI
  GET /api/goals/:id/diff
    → isolated: goal_workspace.rs::changes (status + tracked diff + untracked names)
    → ordinary: goals/diff.rs::capture (receipt-scoped read-only diff)
  POST /api/goals/:id/discard { confirmation: "DISCARD_GOAL_WORKTREE" }
    → check terminal goal and no active child jobs
    → git worktree remove --force (ONLY owner-requested isolated worktree)
    → persist discarded marker; later execution fails closed
  POST /api/goals/:id/resume
    → isolated path must still exist and match durable origin metadata
    → existing uncertainty acknowledgment and fresh job checkpoint
```

## Why isolation is actually connected

A detached worktree **is not** a filesystem or network sandbox. It separates Git checkout bytes from the source checkout but allows shell commands and approved external tools to access files outside it. Existing owner approvals remain required for effectful commands. No automatic commit, merge, branch promotion or source overwrite is added.

The goal's working root is resolved at all important layers: **native tool dispatch, effect receipt sealing, deterministic replacement paths, project manifest inspection, source indexing, deterministic file acceptance, and file-output SHA receipts**. Nested delegated agents resolve the same root from their parent session chain, with cycle and depth guards. A worktree removed by an owner action can never silently fall back to the shared source folder.

Goal worktrees require a **clean top-level Git checkout and a data directory outside that checkout**. A branchless detached checkout is used; changes remain reviewable as a diff but are never implicitly cherry-picked. Deleting the goal record requires disposing of the isolated workspace first. The file-output receipt remains conservative: arbitrary shell/Git external effects are not mislabeled successful file edits.

## Parallel investigation and token economics

`tools.rs::definitions` exposes `delegate_tasks` (1–4 tasks, optional await). `tools/batch.rs::delegate_many` validates every agent and prompt before starting any task and counts already active child research jobs. Jobs and sessions persist in SQLite, and the owner collects their outputs with the existing `task_result` path. Only the parent may apply changes.

A child marked `delegatedReadOnly` sees a reduced tool schema at `chat/runtime/turn.rs`, and the SAME allowlist is enforced at the actual native `tools::execute` gate and the external MCP dispatch gate. No shell, arbitrary file reads/writes, approvals, GitHub writes or further delegations are available to a read-only child. The parallel method may improve throughput on independent investigations but **is not proven to reduce total spend**; 4 simultaneous model calls can cost more.

## RSI lineage and fixed evaluation boundary

`engine/rsi_system.rs::{COMPONENTS,source_role}` now maps the executable `goal_workspace.rs` source to `agent_harness` and `tools/batch.rs` to `tools`. They are part of `source_inventory → blueprint → architecture_frontier` for actual descendant improvement planning, alongside `rsi_system/agent_feedback.rs::observe` using ordinary goal outcome receipts as hypotheses. Ordinary task outcomes cannot award independent frozen RSI scores; no model weights, LoRA or fine-tuning are connected.

## Scope, tests and remaining work

Regression tests (authored, **not executed**) include an actual local Git repository, detached worktree and nested child session, tracked change diff, explicit discard preserving original source, and refusal when the original checkout is dirty. The read-only tool-name gate is separately specified in `tools/batch.rs`.

Remaining: OS-level process sandboxing for untrusted commands; owner-controlled promotion/cherry-pick with conflict inspection; richer untracked-file diff; independently measured end-to-end agent success and token cost; automatic DAG write-set reconciliation; and frozen-paired evidence for any recursive improvement. No new score or Claude Code/Codex parity is asserted.

## 2026 engineering references

- OpenAI (2026-02-11), [Harness engineering](https://openai.com/index/harness-engineering/) — per-worktree testability and application legibility.
- OpenAI Developers (2026-08-19), [Codex as a platform](https://developers.openai.com/blog/codex-as-a-platform) — explicit approval and continuing tool execution.
- Anthropic (2026-02-05), [Building a C compiler with parallel Claudes](https://www.anthropic.com/engineering/building-c-compiler) — parallel task constraints and orchestration.
- Anthropic (2026-03-24), [Harness design for long-running application development](https://www.anthropic.com/research/harness-design-long-running-apps) — independent verification and long-horizon work.

### Video technical verification (post-render, no paid retries)

`engine/video.rs::run` now routes **all native, ComfyUI and LTX outputs** through `engine/video/quality.rs::inspect` after the output file exists and before reporting completion. An available local `ffprobe` is used to test video dimensions, aspect, playback duration and expected LTX audio. The structured `technicalQuality` result is stored both in `videoState` and the returned video job result. Invalid actual media returns a `needs_review:` error; no generation API is automatically retried. Missing `ffprobe` produces `status:not_verified`, never a fake pass.

This is **technical stream inspection, not subjective scene quality or shot continuity**. The new path and regression unit test have not been compiled or run. The old 942-test green CI on `ffcb6f6` does not cover this change.

## Owner-reviewed Git promotion — additional executable route

```text
GoalConsole → GET /api/goals/:id/diff
 → goal_workspace::changes → preview
 → bounded full Git --binary diff (max 128KiB) + SHA-256 + canApply
 → show ENTIRE patch and hash to owner
GoalConsole → POST /api/goals/:id/apply { expectedPatchSha256 }
 → api/goals::apply_workspace → goal_workspace::apply_reviewed
 → terminal goal, existing owner worktree, no concurrent writer jobs
 → source HEAD == goal baseCommit, source tracked/untracked clean
 → no untracked files in isolated worktree
 → exact patch hash equals owner's previously reviewed SHA-256
 → git apply --check → recheck original HEAD & status → git apply
 → hash original git diff against the same frozen base
 → persist gitApply { patchSha256, baseCommit, appliedAt, verified }
```

The patch is not clipped for review. Oversize or non-UTF8 diffs require external pull-request review rather than a potentially misleading source promotion. No git reset, cherry-pick, force, auto-commit or auto-merge occurs. A second attempt to apply the same goal is blocked by its durable `appliedAt` marker. A separate owner-confirmed discard action is still available after successful promotion.

Default `DOMINION_DATA` may be inside the source checkout. `goal_workspace::goal_path` now deterministically creates the worktree outside the checkout, using a source-path digest to avoid sharing a sibling directory between repositories.

Regression specifications cover clean-source application, wrong digest refusal, untracked-file refusal, idempotent second apply, nested data location, separate owner/child workspace, dirty source and explicit discard. Live model cost and actual end-to-end agent success rates are still not measured.
