# Sub4 RSI — active architecture (2026-11-13)

This is an entry point to executable code, **not** an adoption receipt. The previous dated Sub4 micro-handoffs were consolidated here to avoid repeated, often stale descriptions of the same system.

## Recompiled descendants

`rsi_evolve.rs` creates detached worktree generations. `rsi_worktree.rs` verifies actual changed source and executes the frozen paired evaluator; `rsi_system.rs` independently replays the original receipts for accepted **and rejected** proposals. The descendant records its generator commit, selected model, source hashes, executed plan and evidence. `rsi_archive.rs` retains eligible source-verified alternatives across campaigns, not text-only successes.

## Mutable methods that can improve one another

- `rsi_meta_controller.rs` chooses and forecasts source interventions from independently measured, source-attributed credit.
- `rsi_prequential.rs` calibrates the forecast and, after repeated measured prediction error, arbitrates an *actual experiment* on its own estimator implementation. Planned, actually edited, independently adopted and rebuilt-successor events are separate.
- `rsi_recursive_rewrite.rs` carries accepted capability innovations into later method rewrites.
- `rsi_principle_learning.rs` revises executable improvement principles using later outcomes. `rsi_intervention_lab.rs` isolates ambiguous multi-module effects.
- `rsi_model_ecology.rs` selects among available fixed-weight model actors using independently observed task outcomes. A provider/model switch is different from training weights.

## Operating boundary

`scripts/sub4-live-rsi.mjs` emits real-model calls, original source writes and model-authored adoption evidence; `.github/workflows/sub4-system-live-rsi.yml` starts this resource-intensive test **only on explicit dispatch**. The last judge-confirmed score was 79. This document does not imply that current HEAD compiled, passed CI or completed a real-model successor chain.

Unit suites live under `tests/unit/`; integration checks remain in `tests/e2e.rs`. Source indexing excludes the tests and frozen oracle. The original historical reports remain recoverable from Git history.

## Fixed-weight-only RSI guarantee

The local LoRA/PEFT training and promotion module, training/evaluation/server scripts, associated tool registration, generation inference sidecar, model-adapter receipts and weight-based principle reward were removed. No running RSI path writes model parameters. `rsi_model_ecology` still experiments with **configured inference routes** to fixed pretrained models, using descendant source-change outcomes. The recursive loop evolves code, policy, memory, tools, and model-routing logic, not model weights.

## Physical module boundaries

`rsi_evolve.rs` is now a small API surface plus native selector template. Generation planning, model execution, research, empirical method trials, and adopted-source lineage logic live in `src/engine/rsi/`. Worktree evaluation, archived source evidence, fixed-weight model ecology, credit assignment, and method rewriting helpers are also grouped physically. The frozen evaluator remains outside the model's source-edit index.

## Method-source proof boundary (2026-11-13)

- `src/engine/rsi/proof/source_roles.rs` owns source-role classification; `rsi/proof/closure.rs` replays original candidate/job/paired receipts and calls that classifier, not model-authored method labels.
- A method A can include actual compiled planning, learning, meta-control, execution and lineage source (`rsi/{planner,learning,meta,execution,lineage}/`). This closes the blind spot for genuine modifications to the execution and inheritance methods.
- Method A/C must alter **only** improvement-method source; B must alter **only** a separate capability. Frozen evaluator (`rsi/evaluation/`) and independent proof (`rsi/proof/`) cannot earn method credit. Mixed application+method patches cannot satisfy a pure A step.
- The original `rsi_evolve.rs` source-scoring functions remain co-located with the native source-mutation selector. The source mutator in `rsi/execution/mutation.rs` hashes/rewrites that exact source layout; moving them without rewriting the mutator disconnects a real recursive edit path.
- Independent source-contract tests are authored in `tests/unit/engine_rsi_closure_source_tests.rs`. No tests, compilation, CI or live model campaign was executed in this pass.

2026 research context (not reproduced results): [SpecForge, Oct 11](https://arxiv.org/abs/2610.15708) on output coupling; [AIDE², Sep 22](https://arxiv.org/abs/2609.26457) on independently selected self-edits; [MetaSkill-Evolve, Jul 6](https://arxiv.org/abs/2607.05297) on task skills versus meta-improvement procedures.

## Immutable independent-verifier gate

The paired evaluator's own `rsi.rs`, `rsi/evaluation/{worktree,metrics}.rs`, `rsi/proof/`, the frozen oracle and test corpus are excluded at **both** the patch/changed-path gate (`rsi::safe_path`) and source-index discovery (`code_index::is_source`), using `rsi/proof/path_policy.rs`. Legitimate experimental work on `rsi/evaluation/intervention_lab.rs`, `rsi/evaluation/rule_tournament.rs`, actual execution, lineage and planner modules stays open. This blocks the trivial strategy of changing what counts as success instead of improving the successor. Bumping source-index parser version invalidates stale discovery records. The authored boundary tests were not executed.

The RSI API candidate/paired-receipt unit corpus was physically moved from `src/engine/rsi.rs` into `tests/unit/engine_rsi_api_tests.rs`. Its mock patch points to editable `src/engine/memory.rs`, not the newly protected evaluator module. The production API file therefore no longer embeds the large unit fixture. Tests remain authored, unexecuted.

## Matched source-ablation control gate (2026-11-13)

`rsi/proof/method_control.rs` reads the original `rsi_generation`, `rsi_candidate`,
`rsi_pair`, `job`, `session`, and `model.usage` records.  A descendant B must
outperform actually run independent controls from A's **pre-method** source
commit on the identical frozen case plan, unchanged baseline outcome, exact
task prompt, objective, model reasoning setting and audited fixed-weight
provider/model route. Control budget must be at least B's. All matched
controls are considered, not a hand-picked weak result; at least one
previously failing B holdout must pass under method-on and remain failing
under all matching method-off controls, with no reverse transitions.

The strict closure, the campaign stop decision and the API now consult this
same verifier; absence of an independent controlled model run means proof
remains false. `recursiveClosure.methodControl` supplies the pre-A source
commit and frozen task hash required to organize a valid repeat. Existing
campaigns are **not** relabeled. Control creation must use a separate,
actual, source-isolated generation through the ordinary RSI evaluator;
this change does not manufacture a control or execute a paid model call.
The contrast is nonrandomized and is not universal causal proof.

2026 evidence: [PAST-Bench](https://arxiv.org/abs/2608.04003) uses
matched experience on/off conditions; [AIDE²](https://arxiv.org/abs/2609.26457)
measures inherited source-level performance on hidden held-out tasks.

## Executable control reproduction

The existing `POST /api/rsi/start` accepts an explicitly approved
`controlForGenerationId`. Its `rsi_closure::control_anchor` replays the A
method and B capability adoption, retrieves B's original generation job,
and replaces user-provided model/task settings with B's real immutable
prompt, objective, frozen cases and actor. `rsi/execution/start.rs` creates
a separate detached worktree from the pre-A generator commit (not the live
branch head) and disables rsi_current lineage reuse and dirty snapshot
application. The standard compiled planner, model worker and frozen paired
evaluator execute one attempted control generation. `select()` always
chooses baseline for controls, preventing adoption and rsi_current changes.
`api::rsi::apply_campaign` rejects any control campaign. The independent
proof verifier requires the resulting original control job to point to the
exact treated B job and pre-A generator. No control costs were incurred here.

The SelfImprovement React page exposes an explicit **대조군 1회 실행**
action only after a replayed method/capability/method chain reports a
missing control. It calls the ordinary `POST /api/rsi/start` and swaps the
job display to that real isolated control campaign. The derived control
budget is at least the treatment's preflight budget (when bounded), and
the ordinary time cap applies. No silent background control requests run.

The normal 3-generation success response must set `proven:true` **only
after** the replayed method-off comparison passes; the verifier returns an
explicit pending control otherwise. Control campaigns retain their original
treatment campaign ID so the UI can return to the correct proof record.

The API control-request validation and task construction are now in
`src/api/rsi/control_request.rs`; HTTP handlers only route and enqueue.
Legacy API unit tests were moved to `tests/unit/api_rsi_tests.rs`.
No Rust build or tests were executed.

The matching comparator deliberately does **not** require the pre-A
policy and post-A policy to select the same domain or candidate target:
that selection is an outcome of the editable improvement method, not a
pre-treatment matching variable. The original task, source branch,
model-usage stratum, token budget and frozen case suite still match.
The API capability tests are separate from the Unix-only API integration
tests so the moved source retains its former test platform coverage.
