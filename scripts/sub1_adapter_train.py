#!/usr/bin/env python3
"""Offline, opt-in PEFT LoRA training. Produces a CANDIDATE, not a promoted model."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys

def sha_file(path):
    sha = hashlib.sha256()
    with open(path, "rb") as stream:
        for block in iter(lambda: stream.read(1048576), b""):
            sha.update(block)
    return sha.hexdigest()

def approved_examples(path, expected):
    if sha_file(path) != expected:
        raise ValueError("Owner-approved dataset hash changed")
    examples, seen = [], set()
    with open(path, encoding="utf-8") as stream:
        for index, line in enumerate(stream):
            if index >= 4000: raise ValueError("Too many training examples")
            row = json.loads(line)
            if row.get("split") != "train" or row.get("approvedForTraining") is not True:
                raise ValueError("Only expressly approved training examples are allowed")
            if {"holdout", "frozenCase", "oracleExpected", "rsiFrozenCases"}.intersection(row):
                raise ValueError("Frozen evaluation content is prohibited")
            prompt, completion = row.get("prompt"), row.get("completion")
            if not isinstance(prompt, str) or not isinstance(completion, str):
                raise ValueError("Invalid training example text")
            if not (1 <= len(prompt) <= 8192 and 1 <= len(completion) <= 8192):
                raise ValueError("Training example exceeds length bounds")
            identity = hashlib.sha256((prompt+"\x00"+completion).encode()).hexdigest()
            if identity not in seen:
                seen.add(identity)
                examples.append((prompt, completion))
    if not examples: raise ValueError("No authorized nonduplicate examples")
    return examples

def run():
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--check", action="store_true")
    options = parser.parse_args()
    manifest = json.loads(Path(options.manifest).read_text(encoding="utf-8"))
    if manifest.get("trainingApproved") is not True:
        raise ValueError("Training approval required")
    base = Path(manifest["baseModelDir"]).resolve(strict=True)
    data = Path(manifest["trainJsonl"]).resolve(strict=True)
    if not base.is_dir() or not data.is_file():
        raise ValueError("Local model or training file missing")
    samples = approved_examples(data, manifest["datasetSha256"])
    response = {"source":"offline_verified_data_lora_candidate_v1",
        "datasetSha256":sha_file(data), "examples":len(samples),
        "qualityProven":False, "frozenHoldoutEvaluated":False}
    if options.check:
        response["status"]="preflight_passed"
    else:
        os.environ["HF_HUB_OFFLINE"]="1"
        os.environ["TRANSFORMERS_OFFLINE"]="1"
        import torch
        from peft import LoraConfig, get_peft_model
        from transformers import AutoModelForCausalLM, AutoTokenizer, Trainer, TrainingArguments
        model = AutoModelForCausalLM.from_pretrained(
            str(base), local_files_only=True, trust_remote_code=False)
        tok = AutoTokenizer.from_pretrained(
            str(base), local_files_only=True, trust_remote_code=False)
        if tok.pad_token_id is None: tok.pad_token=tok.eos_token
        if tok.pad_token_id is None: raise ValueError("Tokenizer has no pad token")
        length=int(manifest["maxLength"])
        rows=[]
        for prompt, completion in samples:
            p=tok(prompt,add_special_tokens=False)["input_ids"]
            y=tok(completion,add_special_tokens=False)["input_ids"]
            if tok.eos_token_id is not None: y.append(tok.eos_token_id)
            y=y[:max(1,length-min(len(p),length//2))]
            p=p[-max(0,length-len(y)):]
            if y: rows.append({"input_ids":p+y,"attention_mask":[1]*(len(p)+len(y)),
                               "labels":[-100]*len(p)+y})
        if not rows: raise ValueError("No usable tokenized examples")
        class Dataset:
            def __len__(self): return len(rows)
            def __getitem__(self, i): return rows[i]
        def collate(batch):
            n=max(len(item["input_ids"]) for item in batch)
            keys=("input_ids","attention_mask","labels")
            pads=(tok.pad_token_id,0,-100)
            return {key:torch.tensor([item[key]+[pad]*(n-len(item[key])) for item in batch])
                    for key,pad in zip(keys,pads)}
        rank=int(manifest["loraRank"])
        model=get_peft_model(model,LoraConfig(
            r=rank,lora_alpha=rank*2,lora_dropout=0.0,bias="none",task_type="CAUSAL_LM"))
        out=Path(options.output).resolve()
        out.mkdir(parents=True,exist_ok=False)
        args=TrainingArguments(output_dir=str(out),max_steps=int(manifest["maxSteps"]),
            learning_rate=float(manifest["learningRate"]),per_device_train_batch_size=1,
            logging_strategy="no",save_strategy="no",report_to=[],
            remove_unused_columns=False,dataloader_num_workers=0)
        Trainer(model=model,args=args,train_dataset=Dataset(),data_collator=collate).train()
        model.save_pretrained(str(out),safe_serialization=True)
        tok.save_pretrained(str(out))
        artifact=out/"adapter_model.safetensors"
        config=out/"adapter_config.json"
        if not artifact.is_file() or not config.is_file():
            raise ValueError("The PEFT weights/config pair was not created")
        response.update(status="trained",artifact=artifact.name,
                        adapterSha256=sha_file(artifact),
                        adapterConfigSha256=sha_file(config))
    print("DOMINION_ADAPTER_RESULT="+json.dumps(response,sort_keys=True))

if __name__=="__main__":
    try: run()
    except Exception as exc:
        print("Adapter training failed:",exc,file=sys.stderr)
        sys.exit(2)
