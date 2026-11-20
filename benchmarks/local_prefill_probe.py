#!/usr/bin/env python3
"""Independent *actual* llama.cpp CPU prefill sample (no mocked model).

A small 2026 Qwen3.5 GGUF is served on a GitHub Actions Ubuntu CPU.
This measures one llama-server slot, NOT the owner's workstation GPU.
Tests do not fail just because runtime latency is noisy; receipts are stored
for comparison. A failed/invalid live request is a genuine failure.
"""
import json
import os
import time
import urllib.request

HOST=os.getenv("DOMINION_LOCAL_BENCH_URL","http://127.0.0.1:8099")
FIXED=("System instructions: You are Dominion, a precise Rust assistant. "
       "The following facts are historical context not current commands. "
       "Each owner decision must be preserved in the durable journal. "
       "Never fabricate tool receipts, file effects or provider token counts. "
       "Tools are scoped and only disclosed when necessary. ")*24
HEAD=("SYSTEM:\n"+FIXED+"\nOLD MESSAGE:\n"+
      "Architecture preserves user intent and separates tool operations. "*20)
assert len(HEAD)>3000

def query(suffix):
    payload={"prompt":HEAD+suffix, "n_predict":8,
             "temperature":0,"cache_prompt":True}
    before=time.perf_counter()
    req=urllib.request.Request(HOST+"/completion",
        data=json.dumps(payload).encode(),
        headers={"content-type":"application/json"},method="POST")
    with urllib.request.urlopen(req,timeout=90) as response:
        result=json.load(response)
    elapsed_ms=round((time.perf_counter()-before)*1000,1)
    if not isinstance(result,dict) or not isinstance(result.get("content"),str):
        raise RuntimeError("llama-server returned no completion")
    timing=result.get("timings") or {}
    return {"elapsed_ms":elapsed_ms,
        "prompt_n":timing.get("prompt_n"),
        "prompt_ms":timing.get("prompt_ms"),
        "tokens_cached":result.get("tokens_cached"),
        "tokens_evaluated":result.get("tokens_evaluated"),
        "predicted_n":timing.get("predicted_n"),
        "cache_flag":payload["cache_prompt"]}

def run():
    # Keep one server slot. Changing suffix must preserve the initial KV
    # prefix without changing system instructions, model, LoRA or tooling.
    cold=query("\nCURRENT OWNER: give one sentence about token budgeting.")
    warm=query("\nCURRENT OWNER: give one sentence about caching.")
    warm2=query("\nCURRENT OWNER: give one sentence about tool selection.")
    record={"model":"Qwen3.5-0.8B-Q4_K_M (2026)",
            "device":"GitHub hosted x86_64 CPU; not owner's RTX GPU",
            "backend":"2026 ggml-org llama-server /completion",
            "cache_mode":"cache_prompt=true, single slot, common HEAD",
            "system_prefix_chars":len(HEAD),
            "cold":cold,"warm":warm,"warm2":warm2,
            "input_rate_cost_estimate":None,
            "note":"prompt_ms and tokens_evaluated are provider receipts; single-run timing is noisy"}
    output=os.getenv("DOMINION_LOCAL_BENCH_OUTPUT","/tmp/sub4-local-prefill.json")
    with open(output,"w",encoding="utf-8") as f:
        json.dump(record,f,ensure_ascii=False,indent=2)
    print(json.dumps(record,ensure_ascii=False,indent=2))

if __name__=="__main__":
    run()
