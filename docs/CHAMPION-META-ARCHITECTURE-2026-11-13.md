# CHAMPION — Source-evolvable meta-architecture after the 18:00 structural judgment

**Branch:** `dominion-champion`  
**Judgment received:** 2026-11-13, 18:00 KST; structural score **48 / 100**, second behind sub4 (49).  
**Scope:** changes below were written and reviewed statically. **No build, tests, CI, model campaign, timing claim, or completion score** has been executed.

## 1. Design inversion: proof is not the planner

The prior champion design was a strong model-edited method lineage, but its orchestration was too close to a scripted A→B→A→B demonstration. That proof remains useful as a **read-only assessor**. It must not choose future experiments, pin the improvement objective, or define the evolving intelligence architecture.

The new independent actors are:

| Layer | Compiled responsibility | Editable? | Original authority |
| --- | --- | --- | --- |
| Intelligence | Models/routes, harness, memory, tools, procedures, learned algorithms | Source and approved runtime policy | Host/runtime execution receipts |
| Source experiment | `rsi/planner/descendant/controller.rs`, `rsi/execution/campaign.rs`, `rsi_worktree/capture.rs` | Descendant source policy and source code | Candidate + original detached worktree evidence |
| Meta experiment | `rsi/meta/experiment_policy.rs`, `rsi_method_generalization/{learning,ranking}.rs`, `rsi_system/meta_program.rs` | **Source-editable source and experiment-selection principles** | Original source-bound frozen evidence |
| Frozen evaluator/adoption | `rsi_worktree/comparison.rs`, `rsi/evaluation/adoption.rs`, `rsi/proof/campaign_verdict.rs` | **NOT** alterable during an experiment | Independent paired receipts, immutable profile hash, source hashes |

The 4-generation A/B method closure still belongs solely to the last row. The meta policy can choose *ordinary paired source evolution* rather than an additional method OFF arm. Non-matching/censored control episodes are never declared causally proven.

## 2. Typed experiment contract, not a JSON-only algorithm

`rsi/meta/experiment_policy.rs` defines independently serializable Rust types:

- `FrozenProtocol`: source commit, frozen profile SHA-256, case count, **amendmentAllowedInTrial=false**.
- `ExperimentProtocol`: `paired_source` | `matched_method_control`, with protocol eligibility verified against the actual compiled source operator.
- `ExperimentOption`: observed replayed trials, gains/losses, model-token cost and source-owned policy score.
- `SourceTarget` and `TargetOption`: source path, source SHA, dynamically discovered role/domain, independent adoption/failure, source-owned priority.
- `ExperimentPolicy`: version, generator commit, policy source, history length, alternatives, selected experiment, ranked **open-ended targets**, source self-redesign proposal and signed SHA-256 envelope.

`synthesize(history, frontier, algebra, request, commit)` is a **pure function**. It does not access `AppState`, SQLite, provider credentials or a mutable evaluator. This is a replaceable source algorithm, not a fixed rule for generations 1–4.

For legacy persistence, boundaries intentionally serialize/deserialize this typed contract to JSON. `decode` checks the exact frozen profile, source commit, version and digest before any consumer accepts it. This is an initial *typed seam*, not a claim that every older `Value` dependency has disappeared.

## 3. Actual invocation, not new orphan policy files

```
POST /api/rsi/start
  → jobs(rsi_evolve) [frozen cases, evaluation parameters, model owner]
  → campaign::run_loop: reconstruct source checkout, compile descendant commit
  → descendant/controller.rs: revalidate history → synthesize open target +
    experiment protocol from this compiled child's editable policy
  → campaign::run_loop: validate typed frozen contract; bind selected policy
    to generation job
  → worker.rs: rederive exact experiment choice + source-target ownership
    from original job, source frontier and revalidated evidence
  → experiment_policy::policy_controls_method
    → if matched method control: real compiled OFF job, frozen paired test
    → if paired source: no extra OFF job; still run real ON source evaluation
  → model/tool worktree edit → independent source pair evaluator → adoption
  → rsi_system/replay.rs: reconstruct experiment decision from original
    running compiled worker + original stored paired evidence
  → next compiled descendant: re-score protocols, roles and source policies
  → campaign_verdict: assess outcome independently (not a control policy)
```

The experiment designer is part of the compiled descendant: a successful model-written change to `src/engine/rsi/meta/experiment_policy.rs` alters subsequent descendant experiment choices. There is **no model-weight training** in this mechanism.

## 4. Continuous meta feedback and self-redesign

The policy reads the last 24 **revalidated** episodes, *including failed intervention designs* (not just rewarded source edits). A method-contrast run is counted only when its selected protocol is re-bound by `revalidated_history` to the **original descendant job** and immutable evaluator profile. Positive control outcomes also require original OFF/ON evidence, actual source/operator provenance and matched frozen pair results.

The source-evolvable priorities combine empirical acceptance/regression, uncertainty, exploration and observed token cost. No hard-coded improvement role enumeration: new role names derived from the actual source architecture become ordinary `SourceTarget` values, then compete for next source edits. Explicit owner-selected targets and a source-backed already-in-progress controlled method handoff still take precedence.

If independent experiments repeatedly fail, `redesignTrigger` recommends **the experiment-selection source itself** as a single isolated next edit target. The next model edits that source using ordinary audited tools and earns no privileged promotion: its code and the following compiled descendant still go through frozen independent pair evaluation.

This yields a cross-level self-improvement *capability*, not simply a proof transcript:
```
task evidence → improvement algorithm change → downstream task evidence
       ↑                                         ↓
   experiment-policy source ← meta experiment feedback
```

## 5. Structural reductions made in this round

- `rsi/planner/descendant.rs`: 681 → **264 lines**; actual source-compiled plan orchestration moved to `rsi/planner/descendant/controller.rs` (~440 lines). Public facade retained.
- `rsi_worktree.rs`: 759 → **459 lines**; detached model source capture/provenance/verification moved to `rsi_worktree/capture.rs` (~306 lines). Comparison and adoption remain separate.
- `rsi_system.rs`: 637 → **503 lines**; recursive feedback directive, executable meta-program compilation and inheritance moved to `rsi_system/meta_program.rs` (~139 lines).
- `rsi/meta/experiment_policy.rs`: pure policy, typed boundary, frozen evaluator fingerprint and source target ranking. The companion tests cover proof forgery, altered frozen profiles, method ON/OFF selection, failed-design self-redesign and unknown new source roles.

## 6. What is intentionally still incomplete

1. The large `rsi/execution/campaign.rs` coordinator (~1,281 lines) and remaining `AppState`/`Value` adapters have **not** been completely eliminated. More event-oriented phase handlers and typed receipts are needed.
2. Today's legal protocol choices are *native frozen paired* and *actual source-backed matched method OFF/ON*. A general arbitrary experiment operator runner is **not** built yet; a JSON name cannot execute a new experiment by itself.
3. A future evaluator can be proposed as a new *source candidate*, but the system does **not** yet support automatic, independently revalidated promotion of a new evaluator definition across campaigns. Current experiment evaluators remain frozen; no self-judged metric rewrite.
4. 4-generation proof remains restrictive and separate from the open-ended meta planner; it is not a requirement that every improvement cycle follows four stages.
5. Unit tests and integration/builds/CI have **not been run**. No live model completion or new structural judge grade is claimed.

## 7. 2026 references and independent adaptation

- [RSIHub (NeurIPS 2026 workshop)](https://github.com/simple-agent-lab/RSIHub) — declared mutation boundaries, fixed evaluator, composable operators and durable lineage. We borrowed the *separation principle*, not its engine/code.
- [GEAR, open general evolutionary architecture](https://rsi-gear.github.io/) — within-task vs across-task meta learning.
- [AIDE² — Recursive self-improvement of AI research agents](https://arxiv.org/abs/2609.26457) — search policies and memory mechanisms evolve through repeated agent-code improvement.
- [SpecForge (2026)](https://arxiv.org/abs/2610.15708) — frontiers, dependency-aware selection and diverse failure-driven exploration.

These are design context, not claims that DOMINION reproduced the reported performance.

## 8. Owner-requested CI, evidence fixes and typed observation (2026-11-13)

Actual GitHub Actions `CI` was executed. The first completed release test run had **803 of 806 passing and three failing**; this is not green. We preserved strict tests and corrected the sources of failure:

- `rsi_archive::select` no longer treats a JSON-quoted hash and the raw digest as different archive candidates. Live and persisted records with the same patch SHA now occupy one Pareto slot.
- The negative-transfer forged-evidence test now tampers with the **actually damaged** holdout case, rather than altering a previously passing case. The original independent receipt must reject the fraud.
- Extracted module `#[path]`, fixture `include_str!`, Rust scoped function visibility, method-family type comparison, dangling doc comments and a mutable-worker borrow boundary were fixed using actual CI compile output.
- New `rsi/execution/observation.rs` defines typed `GenerationObservation` of the original session and worker: model-only tool timeline, per-job and policy-revision program stages/rules, provider route, and tokens. The campaign no longer collects those records itself, shrinking `campaign.rs` from about 1,282 to about 1,222 lines. A source-only regression tests session, job and policy provenance.

The new head's CI final result must be obtained from GitHub Actions. No passing claim is made from the earlier partial 803/806 outcome. No training/fine-tuning/model-weight adaptation is added to RSI.

The dynamic architecture frontier now keeps `src/engine/rsi/meta/experiment_policy.rs` as an individually addressable experimental-design method even when another source is chosen as the representative file. Without this invariant, an experiment-designer failure could produce a redesign proposal with **no reachable code target**. The new entry uses the current code-indexed source SHA and the existing frozen adoption gate; adding a file name alone does not prove improvement.
