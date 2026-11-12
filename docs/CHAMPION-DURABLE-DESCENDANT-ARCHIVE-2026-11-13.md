# Champion: durable descendant-owned RSI stepping-stone archive (2026-11-13)

Branch: `dominion-champion`. Status: source integration only; compilation, CI and live-model evidence not yet measured for this commit.

## Structural change

The archive selection logic comes from the independently developed `dominion-sub4` source revision on 2026-11-13, integrated with Champion's own existing source-planning and worker paths.

1. The **rebuilt descendant binary** now chooses the archive search parent for a whole-system campaign. Previously the resident coordinator had selected an archive candidate before invoking the descendant, then its selection was overwritten later. A descendant-editable program is now the sole ranking owner for this mode.
2. The resident coordinator **independently replays the chosen candidate's original paired receipt**, frozen case plan, source commit and immutable oracle. It repeats this validation in the actual generation worker, checking that plan and inherited candidate ID are identical.
3. After each completed generation, **the archive persists any eligible neutral source patch before publishing that generation receipt**. The last attempt survives a campaign restart even if no subsequent selection occurs. A failed store write is reported as `neutralArchivePersisted=false`, not silent success.
4. Archive records are pinned to candidate ID, patch hash, exact base commit, frozen cases and oracle; stored pointer tampering is rejected on replay.
5. Explorer penalties count **unfinished durable generation jobs** as well as completed generation records, deduplicated by job ID. Source-bound independent accepted descendants are an observational search-priority signal, never a quality/adoption claim for a neutral parent.
6. The descendant owns the archive ranking code and can rewrite it in its next source revision; the immutable paired evaluator, adoption gate and production promotion boundary remain outside that ranking authority.

## Scope

This fixes continuity and authority in the source-evolution protocol, not proof of general intelligence improvement. The historical 3-generation native proof belongs to the older exact source and cannot validate this commit. Changes do not introduce model fine-tuning or unapproved model calls.

## Follow-up verification

Compile `src/engine/rsi_archive.rs` and `src/engine/rsi_evolve.rs` together, run their Rust unit tests and the whole-system controller/worker contract tests; then perform a bounded real-model campaign. The source-level tests include durable restart, invalid archive pointer, unfinished child and final-generation persist scenarios.

Research context: [Darwin Gödel Machine](https://arxiv.org/abs/2505.22954), [Mendel Gödel Machine](https://arxiv.org/abs/2608.07645), [HyperAgents](https://arxiv.org/abs/2603.19461), [VERSE](https://arxiv.org/abs/2610.02616).
