# Sub4 outcome-conditioned open improvement principle learning

## Corrected architectural gap
The earlier invention <-> meta-rewrite loop merely counted two types of source edits. It did not learn whether the second-generation improver caused better downstream ability, discover a general principle from differences between accepted/rejected trials, or generalize beyond the RSI modules.

## Implemented program
- Model authors `improvementPrinciples` with unique id, mechanistic hypothesis, falsifier, predicted downstream change, **actual changed Rust method path**, target intelligence roles, transfer domains, and an arbitrary composition ancestry.
- Proposals are recorded by `rsi_system_plan`. Even an accepted proposal is only a **source-backed hypothesis**.
- The compiled child reconstructs every adopted principle and follows the actual adopted git-commit ancestry. Every later **independently measured** rollout that really selected that exact principle becomes a success, failure, holdout reward or regression observation. Models cannot label themselves as measured.
- The next child ranks these principles using posterior-smoothed success, downstream performance and unexplored transfer. It can test transfer to memory, model architecture, tools, learning or any newly invented role; active choices actually replace the source target and edit-policy prompt.
- When two separately adopted principles succeed and target different roles, their distinct evidence creates an executable **new-method synthesis assignment**. The worker must invent/write new Rust search code and a new principle record to validate its novel composition.
- Explicit owner instructions and adopted successor program priorities are preserved. A frozen independent oracle alone decides source adoption.

## Boundaries
The program does not evaluate new model weights, train on private conversations, or make the frozen holdouts available to the worker. No compile or performance result is claimed. The hypothesis-to-success conversion still requires independent later experiments; no association is called causal proof.

## Research direction
Meta 2026 HyperAgents, KRAFTON WHALE 2026, EvoTrainer 2026, ASCENT 2026, and JoMoHa 2026 motivate coupling learned meta policies, real task learning and model adaptation. This implementation differs by keeping explicit independently replayable source/lineage provenance and cross-domain prequential feedback.
