# Champion — one RSI completion verdict across runtime, API, and audit (2026-11-13)

## Why this change is necessary

The independent 2026-11-13 review found that the runner's three-adoption/source-evolution completion condition differed from the API's strict recursive-method closure. This could terminate a campaign as `capability_demonstrated` while its API presented `capabilityDemonstrated=false`. A source change alone is not a verified recursive METHOD improvement.

## Real execution route

| Responsibility | Live path and function |
|---|---|
| Campaign registration | `src/api/rsi.rs::start` → `jobs::enqueue` |
| Execution | `src/engine/rsi_evolve.rs` → `rsi/execution/source.rs::run` → `rsi/execution/campaign.rs::run_loop` |
| Descendant plan | `rsi/planner/descendant.rs::plan_generation_from_file` builds a plan with current source revisions and replayed history |
| Model, tools, and editing | `rsi/execution/worker.rs::worker` → real session, provider route, tool receipts, source modifications |
| Candidate and frozen comparison | `rsi_worktree.rs::finish` → `rsi_worktree/comparison.rs::compare_at` → original `rsi_pair` metrics |
| Adoption | `rsi/evaluation/adoption.rs::adopt` records accepted worktree commit, actual source hashes, and successor compiler revision |
| **Single completion gate** | `rsi/proof/campaign_verdict.rs::{evaluate,apply}` → original lineage, `generation_proof`, `model_driven_capability_proven`, and `rsi_recursive_closure::evidence` |
| Loop stop | `rsi/evaluation/adoption.rs::proof_goal_reached` reads `campaign_verdict["actualCompletion"]`; ordinary budgets/attempt limits still work |
| Durable recording | `rsi/execution/campaign.rs::run_loop` replays the gate after each persisted generation; `rsi/execution/source.rs::run` repeats it immediately before final campaign storage |
| API and independent review | `src/api/rsi.rs::{apply_capability,campaign_view,list,verify}` and `rsi.rs::capability_audit` consume the same gate, not an API-only definition |

## Proof invariant

`sourceEvolutionDemonstrated` and `modelDrivenCapabilityDemonstrated` remain **partial, independently replayed** achievements.

`capabilityDemonstrated`, `recursiveMethodClosureProven`, and `actualCompletion` all require the exact same `rsi_recursive_closure::evidence`:

1. **A (improved method)**: verified original paired A/B adoption of the actual method source; parent method SHA is recorded.
2. **B (descendant application)**: next independently compiled child actually dispatches the adopted method; the original `operatorExecution` and provider/tool receipts are revalidated; a **different** capability improves on frozen paired cases.
3. **A (method rewritten)**: next compiled descendant rewrites that same method from the specific B evaluation receipt; changed accepted source SHA and new independent meta holdout gain are both required.

The evidence report contains method path, method-family hash, parent and rewritten source hashes, original B evaluation receipt SHA, and all 3 source commits, candidate IDs and generation job IDs. The report is not created from model statements, UI counters or prefilled success flags.

The `rsi/proof/campaign_verdict.rs` contract computes visible and independently verified streaks separately and prevents accidental promotion of a three-adoption streak into RSI completion. It uses *inherited, pinned source lineage*, not only the current campaign's history array. `systemicHandoffProven` is a separate stronger projection and never promotes an incomplete closure.

**Stale-proof behavior:** If a saved campaign says `stopReason=capability_demonstrated` but the source-replayed strict proof is absent, the shared projection overrides the displayed and persisted reason with `recursive_method_closure_not_proven`, marks `needs_review`, and emits `closureClaimInvalidated=true`. The API cannot overwrite this status with a stale parent job status.

## Where the boundary remains

The observed A→B→A sequence is an evidence-backed *execution lineage*, not a matched counterfactual causal effect of the improved method compared with its predecessor. `causalAblationProven=false` remains explicit. Source performance improvement, independent candidate adoption, exact method use, and subsequent method revision are distinct claims. Real multi-generation provider validation is required to demonstrate an observed completion.

## Contract regression checks (authored, not executed)

- `src/engine/rsi.rs`: a forged 3-generation model-free source lineage can have `sourceEvolutionDemonstrated=true`, but must have `actualCompletion=false` and cannot retain a successful stop reason.
- `src/api/rsi.rs::capability_tests`: three `accepted=true` flags and a persisted `capabilityDemonstrated=true` cannot pass the API projection without original receipts.
- `rsi_recursive_closure/tests.rs`: a claimed 3-stage operator chain yields no source-bound closure evidence.
- `proof_goal_reached`: the runtime consumes exactly the authoritative verdict; no independent success rule remains.

No Rust compilation, tests, build, CI, benchmark or paid model campaign ran during this change.

## Research boundaries (2026)

- [AIDE², Sep 22 2026](https://arxiv.org/abs/2609.26457) distinguishes source evolution from an improved research agent's effectiveness on independently held-out tasks.
- [TRUST, Nov 7 2026](https://arxiv.org/abs/2611.10787) identifies false acceptance from greedy update gates and motivates preserving independent receipts before advancing an incumbent.
- These are design references only. This branch has not reproduced their results, nor implemented TRUST's statistical error-control procedure.

## Subsequent 2026-11-13 stricter gate

The A→B→A gate documented above has been superseded by an actual **A→B→A→B live OFF/ON method ablation** requirement in [the newer implementation and execution trace](CHAMPION-METHOD-ABLATION-2026-11-13.md). The canonical verdict is unchanged as the common runtime/API/audit entry point, but its complete proof now requires four generations and two independently evaluated OFF/ON descendants. Previously described three-generation observations are partial method-lineage evidence, never a present-day full-RSI completion.

## Structural supersession — 2026-11-13 18:00 judgment

The newer [source-evolvable meta-architecture](CHAMPION-META-ARCHITECTURE-2026-11-13.md) separates **proof of RSI** (this file's strict 4-step method lineage) from the **meta planner that selects open-ended targets and experiment protocols**. No fixed A/B lineage stage is required for each exploration episode. Current experiments keep a frozen evaluator; a new evaluator proposal cannot approve itself. Source modules were decomposed, but all changes are static and unexecuted.
