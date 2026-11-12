#!/usr/bin/env python3
"""Separate parent-side paired adapter oracle. Never serves cases to a child.

Measures token-level completion cross entropy over the same disjoint private
cases for base and adapter. A candidate has to improve held-out performance
before any RSI descendant can use its weights. Not a source-code oracle.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys

os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"

def hash_file(path):
    d = hashlib.sha256()
    with open(path, "rb") as fh:
        for buf in iter(lambda: fh.read(1048576), b""):
            d.update(buf)
    return d.hexdigest()

def cases(path, expected):
    if hash_file(path) != expected:
        raise ValueError("Frozen evaluation case file changed")
    data, identities = [], set()
    with open(path, encoding="utf-8") as fh:
        for i, line in enumerate(fh):
            if i >= 256:
                raise ValueError("Too many comparison cases")
            row = json.loads(line)
            if row.get("split") != "evaluation" or row.get("approvedForIndependentEvaluation") is not True:
                raise ValueError("Only privately approved evaluation cases are eligible")
            prompt, completion, domain = row.get("prompt"), row.get("completion"), row.get("domain")
            if not all(isinstance(x, str) and x for x in (prompt, completion, domain)):
                raise ValueError("Missing evaluation fields")
            if len(prompt) > 8192 or len(completion) > 8192:
                raise ValueError("Evaluation text exceeds budget")
            signature = hashlib.sha256((prompt+"\x00"+completion).encode()).hexdigest()
            if signature in identities:
                raise ValueError("Duplicated private test item")
            identities.add(signature)
            data.append((prompt, completion, domain))
    if len(data) < 8 or len({case[2] for case in data}) < 2:
        raise ValueError("Independent paired comparison needs >=8 tests and >=2 domains")
    return data

def verify_training_separation(path, expected, private_cases):
    """Refuse identical prompt+completion pairs across training and evaluation.

    Different file SHA-256 digests do not establish sample-level separation.
    Read only parent-controlled approved datasets; never emit private examples.
    """
    if hash_file(path) != expected:
        raise ValueError("Approved training material changed")
    training = set()
    with open(path, encoding="utf-8") as stream:
        for index, line in enumerate(stream):
            if index >= 4000:
                raise ValueError("Too many training items")
            row = json.loads(line)
            if row.get("split") != "train" or row.get("approvedForTraining") is not True:
                raise ValueError("Unapproved training material")
            prompt, completion = row.get("prompt"), row.get("completion")
            if not isinstance(prompt, str) or not isinstance(completion, str):
                raise ValueError("Unstructured training data")
            identity = hashlib.sha256((prompt+"\x00"+completion).encode()).hexdigest()
            training.add(identity)
    if not training:
        raise ValueError("No approved training material")
    for prompt, completion, _domain in private_cases:
        identity = hashlib.sha256((prompt+"\x00"+completion).encode()).hexdigest()
        if identity in training:
            raise ValueError("Training and frozen evaluation examples overlap")

def score(model, tokenizer, test_cases):
    import torch
    import torch.nn.functional as F
    model.eval()
    result = []
    with torch.inference_mode():
        for prompt, completion, domain in test_cases:
            p = tokenizer(prompt, add_special_tokens=False)["input_ids"]
            y = tokenizer(completion, add_special_tokens=False)["input_ids"]
            if tokenizer.eos_token_id is not None:
                y.append(tokenizer.eos_token_id)
            if not p or not y or len(p)+len(y) > 8192:
                raise ValueError("Private sample is outside model context")
            ids = torch.tensor([p+y], dtype=torch.long, device=model.device)
            out = model(input_ids=ids)
            logits = out.logits[:, len(p)-1:-1, :].float()
            labels = ids[:, len(p):]
            loss = F.cross_entropy(
                logits.reshape(-1, logits.size(-1)), labels.reshape(-1), reduction="mean")
            value = float(loss.item())
            if not (value >= 0 and value < 100):
                raise ValueError("Non-finite heldout loss")
            result.append((domain, value))
    return result

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--base",required=True)
    parser.add_argument("--adapter",required=True)
    parser.add_argument("--cases",required=True)
    parser.add_argument("--case-sha",required=True)
    parser.add_argument("--train",required=True)
    parser.add_argument("--train-sha",required=True)
    options=parser.parse_args()
    test_cases=cases(Path(options.cases),options.case_sha)
    verify_training_separation(Path(options.train),options.train_sha,test_cases)
    from transformers import AutoModelForCausalLM,AutoTokenizer
    from peft import PeftModel
    import torch
    base=str(Path(options.base).resolve(strict=True))
    adapter=str(Path(options.adapter).resolve(strict=True))
    tokenizer=AutoTokenizer.from_pretrained(base,local_files_only=True,trust_remote_code=False)
    model=AutoModelForCausalLM.from_pretrained(base,local_files_only=True,trust_remote_code=False)
    raw=score(model,tokenizer,test_cases)
    del model
    if torch.cuda.is_available():torch.cuda.empty_cache()
    model=AutoModelForCausalLM.from_pretrained(base,local_files_only=True,trust_remote_code=False)
    tuned=PeftModel.from_pretrained(model,adapter,is_trainable=False,local_files_only=True)
    adapted=score(tuned,tokenizer,test_cases)
    by_domain={}
    for (domain,b),(domain2,a) in zip(raw,adapted):
        if domain!=domain2:raise ValueError("Unpaired comparison")
        entry=by_domain.setdefault(domain,{"examples":0,"baselineLossSum":0.0,"candidateLossSum":0.0})
        entry["examples"]+=1
        entry["baselineLossSum"]+=b
        entry["candidateLossSum"]+=a
    for values in by_domain.values():
        n=values.pop("examples")
        values["examples"]=n
        values["baselineMeanLoss"]=values.pop("baselineLossSum")/n
        values["candidateMeanLoss"]=values.pop("candidateLossSum")/n
    baseline=sum(v for _,v in raw)/len(raw)
    improved=sum(a < b for (_,b),(_,a) in zip(raw,adapted))
    outcome={"source":"separate_parent_paired_lora_oracle_v1",
        "caseSha256":options.case_sha,"measuredExamples":len(raw),
        "distinctDomains":len(by_domain),"baselineMeanLoss":baseline,
        "candidateMeanLoss":sum(v for _,v in adapted)/len(adapted),
        "individuallyImproved":improved,
        "domains":by_domain,"privateExamplesReturned":False,
        "trainingEvaluationDisjointVerified":True}
    print("DOMINION_ADAPTER_EVAL="+json.dumps(outcome,sort_keys=True))

if __name__=="__main__":
    try: main()
    except Exception as err:
        print(f"Independent adapter evaluation failed: {err}",file=sys.stderr)
        sys.exit(2)
