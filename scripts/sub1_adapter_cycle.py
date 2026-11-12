#!/usr/bin/env python3
"""Offline self-improving local language-model weights.

Separate phases: owner-approved training, base-vs-adapter BLIND prediction,
and inference service for Rust-accepted adapters only. This script receives
NO heldout labels. Rust is the independent evaluation/adoption authority.
"""
import argparse
import contextlib
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import sys
import time

os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"


def sha_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for b in iter(lambda: f.read(65536), b""):
            h.update(b)
    return h.hexdigest()


def examples(path, expected):
    if sha_file(path) != expected:
        raise ValueError("Owner-approved train data changed")
    rows, seen = [], set()
    for i, line in enumerate(path.read_text(encoding="utf-8").splitlines()):
        if i >= 4000:
            raise ValueError("Training sample budget")
        entry = json.loads(line)
        if entry.get("split") != "train" or entry.get("approvedForTraining") is not True:
            raise ValueError("Unapproved training row")
        if any(k in entry for k in ("holdout", "oracleExpected", "frozenCase", "rsiFrozenCases")):
            raise ValueError("Frozen evaluation fields are not training data")
        prompt, completion = entry.get("prompt"), entry.get("completion")
        if not isinstance(prompt, str) or not isinstance(completion, str):
            raise ValueError("Invalid training example")
        if not (1 <= len(prompt) <= 8192 and 1 <= len(completion) <= 8192):
            raise ValueError("Invalid training length")
        key = hashlib.sha256((prompt + "\0" + completion).encode()).hexdigest()
        if key not in seen:
            seen.add(key)
            rows.append((prompt, completion))
    if not rows:
        raise ValueError("No approved training examples")
    return rows


def local_model(base):
    import torch
    from transformers import AutoTokenizer, AutoModelForCausalLM
    tokenizer = AutoTokenizer.from_pretrained(
        str(base), local_files_only=True, trust_remote_code=False)
    if tokenizer.pad_token_id is None:
        tokenizer.pad_token = tokenizer.eos_token
    if tokenizer.pad_token_id is None:
        raise ValueError("Tokenizer lacks pad token")
    model = AutoModelForCausalLM.from_pretrained(
        str(base), local_files_only=True, trust_remote_code=False,
        torch_dtype="auto")
    model.eval()
    return tokenizer, model


def run_train(manifest, output, check):
    base = Path(manifest["baseModelDir"]).resolve(strict=True)
    train = Path(manifest["trainJsonl"]).resolve(strict=True)
    if manifest.get("trainingApproved") is not True:
        raise ValueError("Owner training approval missing")
    if not base.is_dir() or not train.is_file():
        raise ValueError("Missing local model or data")
    rows = examples(train, manifest["datasetSha256"])
    receipt = {"source": "sub1_actual_peft_training_v1",
               "datasetSha256": sha_file(train), "examples": len(rows),
               "qualityProven": False}
    if check:
        receipt["status"] = "checked"
    else:
        import torch
        from peft import LoraConfig, get_peft_model
        from transformers import TrainingArguments, Trainer
        tok, model = local_model(base)
        max_len = int(manifest["maxLength"])
        samples = []
        for prompt, completion in rows:
            prefix = tok(prompt, add_special_tokens=False)["input_ids"]
            result = tok(completion, add_special_tokens=False)["input_ids"]
            if tok.eos_token_id is not None:
                result.append(tok.eos_token_id)
            result = result[:max(1, max_len//2)]
            prefix = prefix[-max(0, max_len-len(result)):]
            if len(result):
                samples.append({"input_ids": prefix+result,
                                "attention_mask": [1]*(len(prefix)+len(result)),
                                "labels": [-100]*len(prefix)+result})
        if not samples:
            raise ValueError("No usable token samples")
        class Dataset:
            def __len__(self):
                return len(samples)
            def __getitem__(self, i):
                return samples[i]
        def collate(batch):
            size = max(len(x["input_ids"]) for x in batch)
            result = {}
            for name, pad in (("input_ids",tok.pad_token_id),
                              ("attention_mask",0),("labels",-100)):
                result[name] = torch.tensor([row[name]+[pad]*(size-len(row[name]))
                    for row in batch])
            return result
        rank = int(manifest["loraRank"])
        model = get_peft_model(model, LoraConfig(
            r=rank,lora_alpha=rank*2,lora_dropout=0.0,bias="none",
            task_type="CAUSAL_LM"))
        output.mkdir(parents=True, exist_ok=False)
        Trainer(model=model, args=TrainingArguments(
            output_dir=str(output), max_steps=int(manifest["maxSteps"]),
            learning_rate=float(manifest["learningRate"]),
            per_device_train_batch_size=1, save_strategy="no",
            logging_strategy="no", report_to=[], remove_unused_columns=False),
            train_dataset=Dataset(), data_collator=collate).train()
        model.save_pretrained(str(output), safe_serialization=True)
        tok.save_pretrained(str(output))
        artifact = output / "adapter_model.safetensors"
        if not artifact.is_file():
            raise ValueError("No actual adapter weights")
        receipt.update(status="trained", artifact=artifact.name,
                       adapterSha256=sha_file(artifact))
    print("DOMINION_ADAPTER_RESULT=" + json.dumps(receipt, sort_keys=True))


def generate(model, tokenizer, prompt, length):
    import torch
    encoded = tokenizer(prompt, return_tensors="pt", add_special_tokens=True)
    device = model.get_input_embeddings().weight.device
    tokens = {k: v.to(device) for k,v in encoded.items()}
    with torch.inference_mode():
        output = model.generate(**tokens, max_new_tokens=length,
            do_sample=False, pad_token_id=tokenizer.pad_token_id)
    new = output[0][encoded["input_ids"].shape[-1]:]
    return tokenizer.decode(new, skip_special_tokens=True).strip()


def load_adapter(base, adapter):
    from peft import PeftModel
    tok, model = local_model(base)
    model = PeftModel.from_pretrained(
        model, str(adapter), is_trainable=False)
    model.eval()
    return tok, model


def run_predict(base, adapter, prompts_path, output_limit):
    tok, model = load_adapter(base, adapter)
    prompts = [json.loads(line) for line in
        prompts_path.read_text(encoding="utf-8").splitlines() if line]
    if not (2 <= len(prompts) <= 64):
        raise ValueError("Independent prompt budget")
    predictions = []
    for row in prompts:
        if not isinstance(row.get("id"),str) or not isinstance(row.get("prompt"),str):
            raise ValueError("Invalid blind prompt")
        with model.disable_adapter():
            base_text = generate(model, tok, row["prompt"], output_limit)
        adapter_text = generate(model, tok, row["prompt"], output_limit)
        predictions.append({"id": row["id"], "baseline": base_text,
                            "candidate": adapter_text})
    # Only predictions leave the model process; hidden expected labels stay
    # inside the independent Rust assessor.
    print("DOMINION_EVAL_RESULT=" + json.dumps({
        "source":"separate_blind_base_vs_lora_inference_v1",
        "predictions": predictions}, ensure_ascii=False))


def run_serve(base, adapter, alias, port):
    tok, model = load_adapter(base, adapter)
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, fmt, *args):
            pass
        def respond(self, status, value):
            body=json.dumps(value, ensure_ascii=False).encode()
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        def do_GET(self):
            if self.path == "/v1/models":
                self.respond(200, {"object":"list","data":[
                    {"id":alias,"object":"model","owned_by":"dominion-lora"}]})
            elif self.path == "/health":
                self.respond(200, {"status":"ready","adapter":alias})
            else:
                self.respond(404, {"error":"Not found"})
        def do_POST(self):
            if self.path != "/v1/chat/completions":
                return self.respond(404, {"error":"Not found"})
            try:
                length = int(self.headers.get("Content-Length","0"))
                if length <= 0 or length > 262144:
                    raise ValueError("Invalid request size")
                request = json.loads(self.rfile.read(length))
                if request.get("model") != alias:
                    return self.respond(400, {"error":"Model mismatch"})
                messages = request.get("messages") or []
                if not isinstance(messages,list) or len(messages)>64:
                    raise ValueError("Invalid messages")
                clean=[]
                for msg in messages:
                    role=msg.get("role")
                    if role not in ("system","user","assistant"):
                        continue
                    content=msg.get("content")
                    if isinstance(content,str):
                        clean.append({"role":role,"content":content[:65536]})
                if not clean:
                    raise ValueError("No text messages")
                if hasattr(tok,"apply_chat_template") and tok.chat_template:
                    prompt=tok.apply_chat_template(clean,tokenize=False,
                        add_generation_prompt=True)
                else:
                    prompt="\n".join(m["role"]+": "+m["content"] for m in clean)+"\nassistant:"
                output_limit=min(1024,max(1,int(request.get("max_tokens",256))))
                output=generate(model,tok,prompt,output_limit)
                if request.get("stream") is True:
                    chunks=[
                        {"choices":[{"index":0,"delta":{"role":"assistant",
                            "content":output},"finish_reason":None}]},
                        {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}
                    ]
                    self.send_response(200)
                    self.send_header("Content-Type","text/event-stream")
                    self.send_header("Cache-Control","no-cache")
                    self.end_headers()
                    for chunk in chunks:
                        self.wfile.write(("data: "+json.dumps(chunk)+"\n\n").encode())
                        self.wfile.flush()
                    self.wfile.write(b"data: [DONE]\n\n")
                else:
                    self.respond(200, {
                        "id":"dominion-adapter","object":"chat.completion",
                        "model":alias,"choices":[{"index":0,
                            "message":{"role":"assistant","content":output},
                            "finish_reason":"stop"}],
                        "usage":{"prompt_tokens":len(tok.encode(prompt)),
                                 "completion_tokens":len(tok.encode(output))}})
            except Exception as error:
                self.respond(400, {"error": str(error)[:200]})
    ThreadingHTTPServer(("127.0.0.1",port),Handler).serve_forever()


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--mode",required=True,
        choices=["check","train","predict","serve"])
    parser.add_argument("--manifest")
    parser.add_argument("--output")
    parser.add_argument("--base")
    parser.add_argument("--adapter")
    parser.add_argument("--prompts")
    parser.add_argument("--alias",default="dominion-adapter")
    parser.add_argument("--port",type=int,default=4321)
    parser.add_argument("--max-new-tokens",type=int,default=64)
    opts=parser.parse_args()
    if opts.mode in ("check","train"):
        manifest=json.loads(Path(opts.manifest).read_text(encoding="utf-8"))
        run_train(manifest,Path(opts.output),opts.mode=="check")
    elif opts.mode=="predict":
        run_predict(Path(opts.base),Path(opts.adapter),Path(opts.prompts),
                    max(1,min(512,opts.max_new_tokens)))
    else:
        run_serve(Path(opts.base),Path(opts.adapter),opts.alias,opts.port)

if __name__=="__main__":
    try:
        main()
    except Exception as e:
        print("DOMINION_ADAPTER_ERROR="+str(e),file=sys.stderr)
        sys.exit(2)
