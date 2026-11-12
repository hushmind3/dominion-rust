# Sub1: local model-weight learning and verified descendant inheritance (2026-11-13)

## Correction for the standing -5 structural regression

The sub1 compiled whole-agent already has model *routing* and source-edit policy learning. That is not foundation-model weight learning. This change restores an optional, concrete PEFT/LoRA model-weight pipeline in addition to the independently evaluated source-level RSI.

## Live path, not narrative-only capabilities

1. The continuous RSI API records explicit `allowLocalAdapterTraining=true` as `rsiAllowLocalAdapterTraining`. The flag cannot be enabled by a model-generated method proposal or by a regular chat.
2. Only an opted-in actual RSI generation is shown `rsi_train_local_adapter`, even when the dynamic edit-program or learned rule would otherwise hide unrelated tools. Its optional use is explained in the model context. The rebuilt model worker offers `rsi_train_local_adapter`. Rust checks the running generation, permitted manifest basename, canonical approved training directory, canonical local base-model directory, SHA-256, bounded step/rank/context/learning rate. `scripts/sub1_adapter_train.py` validates each training example, excludes hidden-oracle fields, then uses PEFT to train and save an actual `adapter_model.safetensors` candidate. An existing output directory is never overwritten.
3. The candidate's provenance, base model, training data hash and unchanged artifact SHA are persisted as an *unverified* model candidate. A training receipt is not a quality result.
4. After the worker exits, the parent coordinator independently evaluates base weights vs the candidate over the same withheld sample set using `scripts/sub1_adapter_eval.py` from the parent source checkout. Its data never reaches the child. The trainer's original JSONL is hash checked again and its example hashes must be disjoint from all private examples, not merely different file bytes.
5. The paired gate requires at least 8 tests and 2 domains, positive aggregate loss gain, no individual-domain loss regression, and enough individually improved samples. Only a successful gated comparison stores a campaign-scoped promotion receipt.
6. On the next generation, the parent selects only a matching campaign's independently promoted adapter. The rebuilt child verifies the source script, exact weights and canonical model directory, launches an isolated loopback PEFT chat service, and supplies that actual provider only within its own session. No global model registration or production deployment occurs.
7. After the source-editing run, the worker obtains its model-specific call/token evidence with `Store::session_provider_usage`, a full session+provider SQL query. A ready sidecar or generic prompt does not prove the model was used. These receipts and the source candidate's independent evaluation are recorded separately in generation history. A no-progress rollback policy prevents an ineffective adapted model from monopolizing further generations.

## Scope and tests

- Rust: `src/engine/rsi_model_weights.rs`, `rsi_evolve.rs`, `jobs.rs`, `chat/provider.rs`, `chat/runtime.rs`, `tools.rs`, `db.rs`, API and module registration.
- Python: `scripts/sub1_adapter_train.py`, `sub1_adapter_eval.py`, `sub1_adapter_server.py` and `test_sub1_adapter_eval.py`.
- Included regressions: paired loss consistency and per-domain rollback, insufficient domains and duplicated holdouts, identical cross-split examples despite different file hashes, and 600 unrelated audit events not truncating real adapted calls.

### Experimental status

The pipeline and regression tests are committed but not executed as CI or a multi-generation real-model run in this change. Promotion, actual weight gain and recursively inherited improvement are only established by future independent execution evidence. The pre-existing source-level evaluator, neutral archive and cross-component negative-transfer learner remain separate and intact.
