# Sub3 RSI: measured archive search and evaluation separation

## Research applied

The implementation was informed by these original GitHub repositories:

- [DGM](https://github.com/jennyzzt/dgm/blob/a565fd2d1dca504ef5104a7cc0f3bdc4ab9b4fd2/DGM_outer.py): keep executable stepping stones and balance parent quality against how many descendants have already been explored.
- [GEPA](https://github.com/gepa-ai/gepa/blob/ce5b0d6e3d6af6c6dbec6de2c54536bbb961d309/src/gepa/strategies/candidate_selector.py): retain candidates that excel on different evaluation cases rather than collapsing all evidence to the latest aggregate score.

This is an original Rust implementation of those search ideas, not a copy of either project's Python code. Model weights are unchanged.

## Executed search path

`rsi_evolve` now chooses a parent through `rsi_archive` before creating the next worker. The archive is derived from the campaign's durable history; it survives restart without maintaining a second mutable cache.

Each entry must match the current base commit, normalized frozen plan and coordinator oracle content hash. The server recomputes the complete evaluation from its paired receipt, verifies patch identity, and requires both builds and all regression guards to pass. A forged summary or externally supplied evaluation source cannot qualify an entry.

Non-dominated per-case quality/cost vectors form a Pareto frontier. Selection prefers the frontier parent with fewer distinct child attempts, with deterministic ties. Equal patches are deduplicated. A neutral parent is restored only in a disposable checkout; the accepted HEAD and the original A/B baseline stay unchanged. The child must make a new source change and independently qualify for adoption.

Native source generation now tries the remaining applicable operators after one operator has been independently evaluated, instead of returning no candidate at the first exhausted operator. Only replayable server-owned receipts reserve a source/operator pair. Explicitly opted-in automatic source search can run a native candidate before spending a model turn; fixed owner file edits and inherited patches retain their editing path. Native actions and model actions remain separately attributed.

The first current-source trial accepted a native memory change (long recall 130.194ms → 72.110ms) but rejected a merely 5% faster improver candidate under the unchanged 20% threshold. That experiment was stopped during the next model repair attempt to develop a stronger operator. The new operator synthesizes a single accumulator across all eligible domains, preserving verified gain/cost payoff, objective constraints, domain ties and the existing expression/counterexample interpreters. Differential tests compare it to the original feature-map selector across mixed histories and objectives. Native candidates identify their actual operator as generator and cannot credit an unused model editing policy.

## Evaluation changes

- The live workflow starts from the exact current commit. It no longer reintroduces a historical decomposition bug.
- The old four-example planning set is a development case. Its counterexamples can guide reflection.
- A distinct 16-input planning confirmation set is frozen into both variants. Its raw examples/answers are not returned as model feedback.
- The coordinator's fixture is excluded from source discovery and RSI file read/write tools. This is a tool boundary, not an OS security sandbox.
- Other regression cases, metric improvement thresholds, AB/BA ordering, build identity checks, explicit production application and the three-generation capability proof remain in force.
- An automatic proof campaign can request `stopAfterProof:true`. It finishes only after replaying the complete independent three-generation capability proof; a streak counter or caller-supplied capability flag cannot stop the run as a success. Ordinary campaigns retain their existing continuation behavior.

## Verification and evidence

Archive tests cover competing specialists, a dominated latest patch, exploration after a failed descendant, mutated source hashes, forged raw measurements, different bases/plans and stale oracle identities. Other regressions cover operator starvation, holdout feedback redaction and direct fixture access.

The current-source campaign `6ad36711-7016-4ede-8ad2-0e875899cf68` completed in 483,669ms with exactly three independently adopted native source generations. Long recall improved 129.059→70.789ms, the improver selector 104.378→11.888ms, and the rebuilt improver's memory descendant 74.099→50.225ms. All three comparisons reported zero regressions and unchanged accuracy. Each used 14 paired cases, with six AB/BA timing repetitions per variant. The server replayed the full capability proof and stopped with `capability_demonstrated`; `actualCompletion=true`.

The exact accepted lineage is `ae927f6 → 4384f1f → 1c226b6 → 71fef4e`. Independent audit also compared each accepted Git diff to its candidate patch SHA-256, each commit's parent to its generator, all three distinct generator binary hashes, and each on-disk evidence file to the saved paired receipt. The accepted commits were fast-forwarded into the development branch, preserving those exact identities. Existing production servers were not restarted.

See [the machine-readable evidence](../benchmark/sub3-rsi-completion-2026-11-12.json). Each generation links its compressed original compilation/evaluation log under `benchmark/rsi-evidence/2026-11-12-sub3/`; SHA-256 of the decompressed bytes equals that generation's `pairedEvidenceSha256`.

Final adopted source validation: `cargo test --release --all-targets` passed **831 tests** (16 benchmark + 713 engine/API + 3 audit + 99 integration), with zero failures. The compiled selector was also compared directly to the original feature-map policy across mixed histories and goals. Frontend typecheck/build passed, including the readable completion message.

This is a bounded native source optimization result: model calls/tokens are zero, model editor reward is zero, and `accelerationProven=false`. It is not evidence of LLM-authored mutations, open-ended general intelligence, or a causal increase in the probability of discovering future improvements. A passing test suite or a neutral search parent remains insufficient to establish those stronger claims.
