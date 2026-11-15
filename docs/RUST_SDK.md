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
- delegate: spawn a child with lineage and reserved budget.
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
Command execution requires shell_enabled and explicit user approval.

## Important limits

Rust has tool-level durable write-ahead intent, but no OS-isolated shell
sandbox. Subagents are independent and do not automatically merge their
repository edits into a parent. The model context projection is a char-based
request-budget heuristic, not a measured provider token reduction guarantee.
The cache bill reflects reported cached tokens and user-specified rates only.

No model-weight update, LoRA, or fine-tuning mechanism is linked into RSI.
There is no goals.rs or goals.api; objectives and mid-run directives remain
inside the existing Rust engine's execution loop.

## Validation status

Source and JS syntax were inspected during this refactor. Rust compilation,
provider E2E, desktop GUI navigation, OS sandbox evaluation and SWE-bench
comparison are still required before declaring frontier parity. Formal CI is
deferred until source cleanup and compatibility review.
