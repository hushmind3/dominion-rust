# Champion — exact API-model attribution and inherited improvement-method learning

Date: 2026-11-13. Judge snapshot before these changes: **Champion 79/100**, rank 1, one point ahead of Sub4. New source changes are not included in that score.

## Core defect A — API model credit without execution identity

Previously a generation could show `tokenUsage.calls > 0` and an accepted source patch without proving that **the provider/model selected by the descendant** was the same provider/model that actually served the worker's session. A mixed-model session or fallback call could distort `rsi_model_lineage` selection rewards.

### Implemented source-to-source correction

- `src/db.rs`: original, append-only `model.usage` audit replay returns exact `providerId`, `model`, `sessionId` token and call counts; uses the existing session route enumeration.
- `src/engine/rsi_worktree.rs`: a candidate is model-driven only when the session contains real positive-token calls **from exactly the requested provider/model**, no other provider/model appeared on that session, and a model-origin verified source write is recorded. Candidate and run capture the route and its receipt.
- `src/engine/rsi_evolve.rs`: durable generation receipt records `apiRouteSessionId`, `requestedProviderId`, `requestedModel`, `apiRouteUsage`, `apiRouteSessionExclusive`, and `apiRouteExecutionVerified`; the last flag also checks the original worktree capture.
- `src/engine/rsi.rs`: `model_route_audited` **replays source usage and candidate origin from the store**, checks original job/session identity and exact model, rejects any mixed provider/model session. `model_route_replayed` adds accepted source-final-byte causal editing and actual adoption requirements for model-driven RSI claims.
- `src/engine/rsi_model_lineage.rs`: model performance learning **requires** the independently replayed actual API route, candidate-scoped paired evaluation and frozen source/profile identity; an unsuccessful but genuinely measured model attempt may still train forecasts without being called an adopted improvement.

The API service model's **weights are not claimed to have changed**. This evaluates the model as an agent that makes downstream DOMINION more capable.

## Core defect B — improvement principles forgotten across adopted rewrites

The old operator `ruleId` includes the originating `acceptedCommit`. Its semantic improvement principle may survive revision while `ruleId` changes, so exact-ID-only learning discarded the previous method's performance when comparing **different source revisions of the same method**.

### Implemented source-to-source correction

- `src/engine/rsi_improvement_algebra.rs` computes a stable, replayable `methodFamilyId` from **name, method role, actual source path, normalized input roles and output roles**. Evidence-specific `ruleId` remains commit-specific, so separate revisions are never conflated as one successful measurement.
- The compiler stores and independently validates the family SHA-256. Renaming the operator or changing its role transformation makes a new family; simply revising source code retains a comparable lineage.
- `src/engine/rsi_method_generalization.rs` replays independently paired trials across all method revisions within that family, while requiring the **historical executed rule's original ID** to match its archived compiled plan. This restores matched-case revision comparisons. A frozen profile, case, domain and baseline condition must still match; the observed revision difference is **not** automatically causal superiority.
- The learned `methodFamilyId` and matched method revision support feed the next descendant's actual rule priority and method-source rewrite decision, not just a report.

## Regression tests added (authored; not executed)

1. Exact requested model gains credit only with an original exclusive provider/model/session audit.
2. A forged model string, changed usage total and second provider in the same session invalidate attribution.
3. Actual `rsi_worktree::finish` rejects zero API calls and a mixed API-model session **before candidate registration**.
4. A rewritten rule keeps its semantic method-family ID but must receive a distinct source-revision evidence ID.
5. An archived independent paired evaluator receipt for one method revision may inform its newer family descendant; an unrelated method is excluded.

## Proof boundary

No new CI or live API model calls were started. The current container has neither `cargo` nor `rustc`, so this commit set is an **implemented source-level learning path with authored regression tests**, not a claim of compiled successful multi-generation RSI or an increase from the judge's previous 79. All commits use `[skip ci]` until the project judge explicitly qualifies the branch.

The next empirical criterion is an actual multi-generation run with **replayed exclusive API route → independently evaluated source edit → adopted method → revised method family's outcomes on frozen holdouts → source-rebuilt descendant applying the updated improvement priority**, with no unapproved provider fallback.

## Cross-branch design basis

Sub3's current `rsi_api_route_learning.rs` audits provider/model/session identity and separates observable task performance from provider model superiority. Champion adapts that evidence boundary into its existing source-rewritable model-lineage and method-family learner rather than importing a parallel second model-selection controller. Sub4's local-weight evolution remains a separate mechanism.
