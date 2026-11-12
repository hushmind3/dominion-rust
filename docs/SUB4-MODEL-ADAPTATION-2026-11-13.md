# Sub4: local parameter-learning channel

The evaluated agent is more than source-code RSI. This adds an optional real
LoRA weight update backend, separately from the source-code candidate pathway.

The compiled worker may call `rsi_train_local_adapter` in `check` or `train`
mode ONLY when its owner campaign input explicitly sets
`rsiAllowLocalAdapterTraining=true`. Both an approved manifest and
training-only JSONL must be stored in `DOMINION_DATA/rsi-training-approved`.
The base model must already exist in `DOMINION_DATA/models`. Training is
offline and invokes optional torch/transformers/peft.

Every training example must have `approvedForTraining=true` and `split=train`.
The file hash must match manifest `datasetSha256`. Frozen benchmark cases
and oracle keys are rejected. The trained LoRA artifact is checked against its
actual SHA-256 and logged as `rsi_adapter_candidate`, never as an improvement.

Manifest example:
```json
{
  "trainingApproved":true,
  "trainJsonl":"/full/path/to/approved/train.jsonl",
  "datasetSha256":"<64-hex SHA-256>",
  "baseModelDir":"/full/path/to/DOMINION_DATA/models/local-model",
  "maxSteps":20,"loraRank":8,"maxLength":1024,"learningRate":0.0001
}
```

Separate adapter-vs-baseline frozen paired evaluation and model runtime
selection remain necessary. This code introduces an *executable route* to
parameter updates; it does not claim a trained or improved deployed model.

Research context: WHALE 2026, ASCENT 2026, JoMoHa 2026, EvoTrainer 2026.
