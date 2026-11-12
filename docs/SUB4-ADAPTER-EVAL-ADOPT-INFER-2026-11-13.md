# Sub4: Independent model adaptation, adoption and actual descendant inference

## Judge gap addressed (2026-11-13)
Prior source modifications could train LoRA candidate weights but lacked independent promotion and a downstream inference binding. This branch now closes the full candidate → independent evaluation → eligible adoption → actual descendant model invocation → later source-outcome feedback loop.

## Implemented ownership boundaries
1. The owner alone can set allowLocalAdapterTraining=true and place training manifests under DOMINION_DATA/rsi-training-approved. The child may train only there, and only from a SHA-256 pinned, individually approved training-only JSONL and already installed HF base model. Training creates adapter_model.safetensors and an independently computed file hash. It is never treated as an improved model automatically.
2. The original campaign coordinator (not the editable descendant) invokes scripts/sub4_adapter_eval.py from its own source. It reads an external rsi-adapter-eval-private/manifest.json and cases.jsonl. Neither the private evaluation text nor expected completions are placed in child prompts or history records. Manifest independentEvaluationApproved=true, casesSha256 match, at least 8 unique pairs in at least 2 domains required. Teacher-forced paired completion loss uses identical cases for baseline and PEFT model.
3. A Rust paired comparison gate checks case counts, per-domain weighted aggregate consistency, per-domain nonregression, overall improvement and number of improved cases. Negative or inconclusive outcomes are recorded without promotion.
4. A passing adapter is promoted only inside its originating isolated RSI campaign (never to production). Each next child selection revalidates promotion source, hashes, campaign ID and actual adapter weight postimage.
5. The next recompiled worker launches scripts/sub4_adapter_server.py, loads local PEFT weights, checks readiness, passes a generation-bound loopback route ONLY to that descendant session (not the global provider registry), and runs the existing source-editing agent using those real weights. The server is stopped after the child returns and no global provider is registered.
6. The model-use receipt only claims actual source-writing inference when the real model.usage audit contains the exact temporary provider and session. A server merely starting or native mutation short-circuit cannot claim model use.
7. The next generation's independently verified source-code outcomes remain distinct from model-heldout loss. The meta-principle learner separately uses weight and source signals. Three independently measured source failures with the same promoted adapter can conservatively restore the prior provider rather than locking RSI to one model.

## Private evaluation sample format
The trusted owner separately creates DOMINION_DATA/rsi-adapter-eval-private/manifest.json with the two keys independentEvaluationApproved=true and casesSha256=SHA256(cases.jsonl). The private cases.jsonl contains at least 8 unique records, distributed over at least 2 domains. Every record has:
- split = evaluation
- approvedForIndependentEvaluation = true
- domain = memory, learning, tools, algorithm etc.
- prompt and completion strings

The private evaluation directory is not available to candidate workspace tools. Keep these evaluation answers independent of approved training data.

## Limitations
This is executable integration, not a completed LoRA training experiment. No actual private evaluation data, model weights, source compiler verification or independent benchmark gain is claimed. Teacher-forced completion loss is a proxy for model ability, not proof of improved general reasoning or downstream source editing. Repeated source-acceptance measurements on generations using adapted weights are required to establish real improvement feedback.

Dependency path: torch, transformers, peft (installed separately), and local Hugging Face model files. The frozen source-code A/B evaluator remains independent and unmodified.

Research and implementation reference: Hugging Face PEFT LoRA local loading and adapter deployment.
