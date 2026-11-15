# Native Rust SDK · Dominion Sub4

The Rust library is the primary interface, not a wrapper around a hosted agent
SDK. All execution decisions stay within the in-process Rust engine.

## Native usage

Add Dominion as a Rust dependency (for development, point the path to this
checkout). Use the same environment settings as the executable:

    use dominion::{Dominion, Settings, CreateRun};
    use std::time::Duration;

    # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    let app = Dominion::open(Settings::from_env()?).await?;
    let run = app.submit(CreateRun {
        instruction: "Create an HTML page and verify its output".into(),
        budget_krw: Some(1000.0),
        max_steps: Some(40),
    }).await?;
    let completed = app.wait(run.id, Duration::from_secs(120)).await?;
    println!("{:?}", completed.status);
    # Ok(())
    # }

Core operations:

- submit/submit_parallel: schedule durable Rust-owned agent workers.
- submit_from_repository: attach a Git source checkout to an isolated worktree.
- delegate/delegate_read_only: spawn an isolated child with persistent budget escrow. Read-only investigators cannot modify files, run shell or integrate Git patches. Parent completion is suspended until children finish and the unused reservation is settled.
- remember/search_memory: reusable non-weight knowledge, persisted per project or globally.
- list_skills/load_skill: on-demand project SKILL.md procedures; full content stays out of the default tool prompt.
- get/list/events/wait: inspect persisted progress and usage.
- instruct: inject user directives; the engine rejects stale model responses.
- approve/cancel/resume/reconcile: durable intervention and crash-side-effect repair.
- artifacts/read_artifact/read_subagent_file: workspace-confined results.
- create_group/group_turn, create_video/render_video, create_improvement/
  evaluate_improvement/adopt_improvement: the same product feature services.
- http_router: an optional API adapter; TypeScript Studio uses this, not an
  alternative model decision loop.

The Rust library API currently evolves with the implementation. The /api/v1
HTTP surface is versioned but not yet a frozen stable interoperability contract.

## Real project coding

A source repository must be beneath the configured DOMINION_WORKSPACE_DIR.
The detached worktree is created at workspace/<execution-uuid>. The initial
source commit and relative repository are saved in the durable Run record.
Exact file edits require one matching original snippet, avoiding risky blind
replacement. Read-only Git status/diff let the model inspect its edits.
Command execution requires shell_enabled and explicit user approval. Linux Bubblewrap or macOS Seatbelt isolation is enforced by default; DOMINION_SHELL_SANDBOX=off explicitly disables this boundary. An approved merge_subagent tool can apply a completed child's Git patch only after conflict checking in the isolated parent worktree.

## Important limits

Rust has tool-level durable write-ahead intent and a fail-closed OS sandbox path for approved shell and RSI commands, without comprehensive resource quotas or cross-platform parity. Subagents do not merge automatically; patch integration requires a separate approval. Model context projection remains a char-count heuristic, not an independently measured token reduction guarantee. Cache billing uses provider-reported read/write usage with configurable KRW rates; missing usage cannot be reconstructed.

No model-weight update, LoRA, or fine-tuning mechanism is linked into RSI.
There is no goals.rs or goals.api; objectives and mid-run directives remain
inside the existing Rust engine's execution loop.

## Validation status

Source and JS syntax were inspected during this refactor. Rust compilation,
provider E2E, desktop GUI navigation, OS sandbox evaluation and SWE-bench
comparison are still required before declaring frontier parity. Formal CI is
deferred until source cleanup and compatibility review.

## Optional TypeScript integration

The TypeScript package now exposes delegate requests with read_only, per-run SKILL.md discovery and load, and project-scoped memory search/store. All calls go through the same independent Rust HTTP adapter. The package does not provide a separate model loop or agent policy.
