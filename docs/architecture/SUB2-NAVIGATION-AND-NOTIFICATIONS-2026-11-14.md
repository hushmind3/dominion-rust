# Sub2 2026-11-14: navigation convergence, quiet RSI, memory corrections

Branch: `dominion-sub2`. Reconciliation source: `main` at `da78dd7e6712c80c2ab7b2738efff1abc5cc7b72`; issue [#8](https://github.com/hushmind3/dominion-rust/issues/8). This is a full-menu **source-path audit**, not a claim that all paths were exercised in a live browser.

## Product navigation: 14 visible menu entries, four groups

| Sidebar group | Visible entry | React entry | Existing API | Execution/data boundary | Verification |
|---|---|---|---|---|---|
| Workspace | Home | `Home.tsx` / `GoalConsole.tsx` | `/api/goals`, `/api/jobs`, `/api/status` | `engine/goals.rs`, `engine/jobs.rs`, `engine/goal_workspace.rs` | Static source |
| Workspace | Chat | `Chat.tsx` | `/api/chat`, `/api/sessions`, `/api/approvals` | `engine/chat/runtime/turn.rs` / native/MCP tool calls | Static source |
| Workspace | Group chat | `Rooms.tsx` | `/api/rooms`, `/api/rooms/:id/messages` | `engine/rooms.rs`, `engine/chat/room_context.rs` | Static source |
| Workspace | Models | `Models.tsx` | `/api/providers`, `/api/model-library/runtime` | `engine/models/*`, `engine/chat/provider/*` | Static source |
| Workspace | Agents | `Agents.tsx` | `/api/agents`, `/api/personas/import` | `engine/personas.rs`, chat runtime and delegated jobs | Static source |
| Workspace | Video | `Video.tsx` | `/api/video/jobs`, `/api/video/config` | `engine/video.rs`, `engine/video/{native,comfy,ltx,quality,upload}.rs` | Static source |
| Extensions/Connections | Connections | `Connections.tsx` + `Extensions.tsx` + `Capabilities.tsx` | `/api/channels`, `/api/mcp`, `/api/skills`, `/api/skill-catalog`, `/api/capabilities` | `engine/channel_adapter.rs`, `engine/mcp.rs`, `engine/skills.rs` | Source tab wiring; no live install run |
| Extensions/Connections | Memory | `Memory.tsx`, `memory/Growth.tsx`, `memory/LearningRecords.tsx` | `/api/memories`, `/api/memories/:id/edit`, `/api/memory/overview` | `api/collections.rs`, `engine/memory.rs`, chat retrieval | Source + new unit test (pending result) |
| Operations | Automations | `Operations.tsx` | `/api/automations`, `/api/jobs` | `engine/scheduler.rs`, `engine/jobs.rs` | Static source |
| Operations | Activity | `Operations.tsx` | `/api/executions`, `/api/audit`, `/api/browser/status` | `engine/run_state.rs`, `engine/browser.rs` | Static source |
| Operations | Self-improvement | `SelfImprovement.tsx` | `/api/rsi/start`, `/api/rsi/campaigns` | `engine/rsi/execution.rs`, `engine/rsi_worktree/*` | Source + quiet-poll patch; no browser click test |
| Operations | Settings | `Operations.tsx` | `/api/policy`, `/api/oauth/providers` | `engine/policy.rs`, `engine/oauth.rs` | Static source |
| Advanced | Workflows | `Workflows.tsx` | `/api/workflows`, `/api/jobs` | `engine/workflows.rs` | Static source |
| Advanced | Developer | `Developer.tsx` | `/api/dev/status`, `/api/dev/update`, `/api/dev/diff` | `engine/dev.rs` | Static source |

The menu is rendered from `WorkspaceShell.tsx::navigation`, which now exposes **one** Connections entry and a separate Memory entry. `App.tsx::locationState/navigate` maps old `#/extensions` and `#/capabilities` routes to `#/connections?section=extensions|capabilities`. `Connections.tsx` restores the corresponding subsection, and tab clicks persist a canonical hash. Original installation, OAuth and channel APIs remain; the frontend is reorganized, not replaced.

## Unsolicited self-improvement alarms

- There was **no browser `Notification` or `window.alert` emission** in the inspected `App.tsx` or `SelfImprovement.tsx` sub2 sources.
- Existing `SelfImprovement.tsx` background 1.5-second campaign polling previously set a prominent `error` on passive GET failure, while `Notice` also displayed stored campaign/install/provider errors on page render. Changed passive polling to 4 seconds with a silent network catch; only user-invoked operations set the prominent `error` notice.
- Campaign failures, installation errors and connection errors remain available under a collapsed **진행·연결 오류 기록 확인** block inside the RSI page. Neither campaign execution nor stored error/audit evidence is deleted.
- `App.tsx` SSE still refreshes normal job, goal, approval and audit information; required owner approvals and error logs remain accessible. No unsolicited RSI browser push or OS notification is added.
- This is a static source improvement. Browser notification scenarios need an interactive local browser test for run-time confirmation.

## Memory editing and provenance

- `Memory.tsx`: list/search/filter/create/delete retained. Each memory gains inline edit/save/cancel, limited to 1,600 characters.
- `api/collections.rs::action` now recognizes `POST /api/memories/:id/edit`, requires exactly `expectedText` and `text`, and applies `store.patch_if("memory", id, "text", expectedText, ...)` atomically.
- Scope, agent ID, source role, existing evidence quotation, source message IDs, lifecycle status and retrieval eligibility are **not** rewritten. `verification=owner_revision` marks the new text as owner corrected, not an exact quote from the original conversation. The previous text can be recovered only through earlier backups, not an exposed revision UI.
- One Rust regression test covers stale edits rejected with HTTP 409, modified text, preserved scope and evidence, and subsequent stale updates rejected. A passing Rust CI result is needed before claiming test execution success.

## Reviewability and verification limits

- CI has automatic GitHub workflows for web typecheck/build and `cargo test --release`. For current changes, front-end typecheck/build has passed on commit `d8aca686`; Rust tests at that commit were in progress at the time of inspection.
- The prior `79597225` run completed 831 Rust tests and failed two browser tests (`controls_a_local_page_end_to_end` and `agent_close_does_not_close_other_browser_sessions`). This does not establish complete engine test success. Browser test failures must be investigated separately from new menu behavior.
- No UI click-through, model request, external channel login, MCP install, video render, or long-term RSI campaign is claimed as performed for this change.

## Follow-up quality gates

Interactive checks: navigate all 14 sidebar entries, switch between the 3 Connections tabs, refresh deep links, install/uninstall an MCP item and a skill with approval, open Marketplace details, edit a memory and recover a concurrent conflict, open RSI with an old failed campaign and temporarily unavailable server without popup, verify true errors within the collapsed record, and confirm approvals/audit logs remain readable.
