# Sub4 — actual model-authored improvement evidence and cross-campaign comparability

Date: 2026-11-13  
Branch: `dominion-sub4`  
Status: source-level integration complete; Rust compilation, unit tests, CI and real-model continuous execution **were not run**.

## Why this change matters

The independent frozen source evaluator determines adoption, but learning *which generator actor and improvement policy to use next* also requires honest attribution. An individual candidate might be produced in a session that calls multiple models, and an adopted patch advances the generator's source commit. Per-campaign history alone therefore rarely has enough trials sharing the exact same source revision to compare model actors.

## Ownership and execution boundary

- `rsi_worktree::rsi_work_branch` allows only `dominion-sub4` plus the original legacy `dominion-sub`. Other developers' branches, including `dominion-sub3`, are not accepted by the standard isolated improvement path. Application also requires the source branch recorded on the original job to match.
- API, worktree and evolution integration fixtures have been moved onto the owned Sub4 checkout. The legacy no-branch-record case remains limited to the original legacy work checkout.

## What constitutes measured single-actor evidence

`rsi_model_ecology::executed` inspects actual `model.usage` audit events emitted by the model gateway for the generation's `sessionId`. It counts only positive-token `purpose=chat` calls; helper calls and other sessions do not count.

Before recording `actuallyUsedInGeneration=true`, the session must have `rsiRun=true` and `rsiGenerationJobId=job.id`; the selected provider and model must equal the worker's input route; at least one selected actor chat call must exist; **zero chat calls from another provider/model** may exist in the same generation. The receipt records selected and mixed call counts, the observed actors, session ownership and route consistency. `verified_attempt` rejects mixed or unowned evidence when learning actor efficacy.

## Cross-campaign source-controlled learning

A candidate actor is not declared superior from mixed source revisions. `rsi_model_ecology::historical_same_source_trials` recovers previous `rsi_generation` records **only** when the generator Git SHA, frozen independent evaluation profile, objective ID and domain are identical. It deduplicates by generation job ID and caps the window at 256 source-matched rows.

Every recovered row must match the original persisted `job` (including input route, model policy and compiled plan execution receipt), source candidate (candidate ID, origin and baseline commit), exact frozen case hash and independently **replayed paired receipt**. The original adoption is revalidated from the candidate and its original experiment adoption commit. A forged or now-invalid evaluation is not usable history. The comparable rows feed route `statistics` and `matched_cohort`; different-source outcomes remain exploratory hints rather than controlled model evidence.

This mechanism makes matched source comparisons possible across independently scheduled campaigns when advancing to an adopted source commit otherwise removes old-source trials from the active campaign. This is an **observational, source-controlled** comparison and does not prove that a specific model caused a whole-agent improvement.

## Inheritance through the genuine recursive loop

`rsi_archive` still persists each independently evaluated neutral stepping stone immediately, including the final budget-limited generation, and rechecks the source/patch/oracle before returning it. The recompiled descendant uses its own archive-parent selection and model-actor policy. The previous coordinator accepts the new choices only after source-bound and permission-bound validation. Subsequent source edits are independently evaluated and adopted under the original frozen A/B harness, and their outcomes are recorded for later descendants.

## 2026-11-13 judge correction: complete, bounded model audit

The judge identified that `rsi_model_ecology::executed` formerly called
`recent_audit(800)`, but the underlying `Store::recent_audit` silently
limits global audit queries to **500** rows. A long-running generation could
therefore have an early alternate model call hidden by later unrelated events.

The corrected production path does **not** raise the global limit:

1. `Store::model_usage_for_session_in_window(session,start,end)` performs a
   session-specific SQL query against the durable audit table using
   `event='model.usage'`, JSON `sessionId`, and an inclusive sealed end
   rowid. A session-specific partial SQLite index avoids a full global scan.
   SQLite/read/parse errors return `Err`; they cannot turn a mixed run into
   a credited single-model run.
2. The rebuilt `rsi_generation` worker persists `rsiAuditStartRowid`
   immediately before `jobs::run_prompt`, and `rsiAuditEndRowid`
   immediately after that turn returns. The completed interval is persisted
   on the original job and echoed in the model-actor attribution receipt.
3. `rsi_model_ecology::executed` counts actual token-bearing chat model
   calls across that entire interval. Any other model, or a chat event
   without a usable provider/model identity, disqualifies **single-actor**
   source-edit credit. Tool/helper calls do not count as model authorship.
4. When an earlier campaign contributes a matched-source trial,
   `independently_bound_comparison` recomputes its model-actor receipt
   against the original persisted job and the raw SQLite audit interval,
   as well as replaying the frozen independent source-code evaluation.
   A later model call in a reused session cannot change the sealed receipt.

Tests exercise >715 interleaved unrelated records beyond the old 500-row
window, competing/unknown model attribution, missing audit bounds, a
modified end cursor, and durable replay after a SQLite reopen. These are
**source-written tests, not tests claimed to have run**.

## Added evidence tests

- Branch-owning checkout is permitted; sibling branches and main are rejected.
- Real session ownership and exclusive chat-model usage are required for actor credit; helper calls do not earn credit; mixed-actor calls disqualify single-model attribution.
- A stored previous campaign's genuine paired evaluation and source-generating job can enter a same-source cohort; tampered job route and paired-evaluation evidence cannot.
- A fabricated historical generation without its original persisted job cannot enter any matched cohort.
- The existing Sub4 durable-archive tests cover final-generation persistence, interrupted worker attempts, tampered archive pointers and source-pinned replays.

**No CI/test execution or real multi-generation model outcome is claimed.**
