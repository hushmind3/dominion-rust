# Sub4 — durable, independently validated, descendant-owned RSI archive

**Date:** 2026-11-13  
**Branch:** `dominion-sub4`  
**Verification status:** source-level implementation; CI, Rust tests and real paid-model continuous campaigns were **not** executed.

## Why the previous archive lost self-improvement continuity

The earlier `rsi_archive::select` searched only the current `rsi_campaign.history`. After a campaign finished or was restarted with empty history, it could no longer select its already evaluated neutral source patches or account for earlier descendant attempts. In addition, the archive choice was made in the *previous* resident coordinator, not the recompiled successor's own policy.

## Implemented execution cycle

1. **Archive verification and persistence** — `src/engine/rsi_archive.rs` imports main's persisted Pareto stepping-stone archive. Each entry is pinned by `(baseCommit, normalized frozen case plan, oracle SHA-256, patch SHA-256)`. Only independently replayed neutral candidates are saved as `rsi_archive` rows. Failed writes are not labelled persisted.
2. **Revalidation on retrieval** — archive rows must match an existing source candidate's patch, exact archive key, case count, paired receipt, complete frozen evaluation and oracle. Corrupted archive pointers cannot select a candidate or hide valid alternatives.
3. **Cross-campaign attempts and crash recovery** — the archive counts both `rsi_generation` results and previously launched `rsi_generation` jobs, deduplicated by job ID. Jobs are restricted to the exact original base commit, archive candidate and frozen oracle. An interrupted worker's launched attempt therefore does not disappear from the search budget.
4. **Learning from adopted descendants** — once each viable Pareto parent has been explored, selection incorporates adopted, independently replayed source changes made by child workers that actually edited the inherited neutral patch. The posterior-smoothed score is an exploratory proxy. It does **not** prove that the inherited patch caused the improvement.
5. **Evolution of the archive algorithm itself** — `rsi_evolve::worker_plan` now executes `rsi_archive::select` in the *newly compiled descendant binary*. The parent coordinator accepts that plan's archive choice without imposing its previous ranking policy. It **does** replay the unchanged independent verification gate before dispatch.
6. **Identical execution-time parent** — `rsi_evolve::worker` checks that the archive choice in the compiled plan matches the job input. `rsi_worktree::prepare` verifies the frozen candidate's receipt **before** creating a disposable Git worktree, then applies only the candidate whose ID the compiled plan selected.
7. **Real offspring measurements** — the normal independent baseline-versus-candidate oracle decides whether the new source is adopted. The next compiled child can learn from those outcomes alongside model routing, principle synthesis and program improvement. No new API model training or other weight modification is claimed.

## Scope of inheritance

The original source baseline must match exactly. Archive recovery intentionally **does not** transplant neutral patches across unrelated Git commits or different frozen evaluators. Independently adopted source revisions are inherited through the existing Git successor lineage, not by treating rejected archive patches as adopted abilities.

## Tests added/inherited

- Original source verifications: Pareto dominance and exploration, empty-history later-campaign resume, tampered paired receipt rejection, mismatched base/case/oracle rejection.
- Additional sub4 cases: store reopen with an unfinished child, archive pointer patch/identity tampering, source-verified offspring-priority guard, and compiled-descendant selection validation.

**No verification claim without actually running these tests.** No branch merge or main modification was made.

## Research inspiration

- Zhang et al., *Darwin Gödel Machine: Open-Ended Evolution of Self-Improving Agents* (2025): https://arxiv.org/abs/2505.22954
- Agrawal et al., *GEPA: Reflective Prompt Evolution Can Outperform Reinforcement Learning* (ICLR 2026): https://proceedings.iclr.cc/paper_files/paper/2026/hash/0e9e708b6f48e14fd0ac29e167413f76-Abstract-Conference.html
