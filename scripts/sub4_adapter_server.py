#!/usr/bin/env python3
"""A promoted LoRA is the actual model in a descendant's generation loop.

OpenAI-compatible text/SSE server used only for a single isolated RSI worker.
Tool-call text protocol remains in Dominion's Rust model runtime.
"""
import argparse
import hashlib
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
import json
import os
from pathlib import Path
import sys

os.environ["HF_HUB_OFFLINE"]="1"
os.environ["TRANSFORMERS_OFFLINE"]="1"

def digest(path):
    h=hashlib.sha256()
    with open(path,"rb") as f:
        for chunk in iter(lambda:f.read(1048576),b""):
            h.update(chunk)
    return h.hexdigest()

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--base",required=True)
    parser.add_argument("--adapter",required=True)
    parser.add_argument("--sha",required=True)
    parser.add_argument("--port",type=int,required=True)
    cli=parser.parse_args()
    if not 0<cli.port<65536:raise ValueError("Invalid local port")
    adapter=Path(cli.adapter).resolve(strict=True)
    weights=adapter/"adapter_model.safetensors"
    if not weights.is_file() or digest(weights)!=cli.sha:
        raise ValueError("Promoted adapter was changed")
    from transformers import AutoModelForCausalLM,AutoTokenizer
    from peft import PeftModel
    import torch
    base=str(Path(cli.base).resolve(strict=True))
    tok=AutoTokenizer.from_pretrained(base,local_files_only=True,trust_remote_code=False)
    model=AutoModelForCausalLM.from_pretrained(base,local_files_only=True,trust_remote_code=False)
    adapted=PeftModel.from_pretrained(model,str(adapter),is_trainable=False,local_files_only=True)
    adapted.eval()
    class Handler(BaseHTTPRequestHandler):
        def log_message(self,*args):pass
        def do_GET(self):
            if self.path!="/health":
                self.send_error(404);return
            response=b'{"ready":true,"adapterVerified":true}'
            self.send_response(200);self.send_header("Content-Type","application/json")
            self.send_header("Content-Length",str(len(response)))
            self.end_headers();self.wfile.write(response)
        def do_POST(self):
            if self.path!="/chat/completions":
                self.send_error(404);return
            try:
                size=int(self.headers.get("Content-Length","0"))
                if not 0<size<=512000:raise ValueError("Invalid request size")
                body=json.loads(self.rfile.read(size))
                messages=body.get("messages",[])
                if not isinstance(messages,list) or not messages:
                    raise ValueError("Missing messages")
                if len(messages)>80:raise ValueError("Too much history")
                normalized=[]
                for item in messages:
                    role=item.get("role","user")
                    content=item.get("content","")
                    if not isinstance(content,str):
                        content=json.dumps(content,ensure_ascii=False)
                    if role not in ("system","assistant","user","tool"):
                        role="user"
                    normalized.append({"role": "user" if role=="tool" else role,
                                       "content":content[:30000]})
                if getattr(tok,"chat_template",None):
                    prompt=tok.apply_chat_template(normalized,tokenize=False,
                                                  add_generation_prompt=True)
                else:
                    prompt="\n".join(m["role"]+": "+m["content"] for m in normalized)+"\nassistant:"
                inputs=tok(prompt,return_tensors="pt",truncation=True,max_length=8192)
                inputs={k:v.to(adapted.device) for k,v in inputs.items()}
                max_tokens=max(16,min(int(body.get("max_tokens",1024)),2048))
                with torch.inference_mode():
                    outputs=adapted.generate(**inputs,max_new_tokens=max_tokens,
                        do_sample=False,pad_token_id=tok.eos_token_id)
                tokens=outputs[0,inputs["input_ids"].shape[-1]:]
                generated=tok.decode(tokens,skip_special_tokens=True)
                usage={"prompt_tokens":int(inputs["input_ids"].shape[-1]),
                       "completion_tokens":int(tokens.shape[-1])}
                usage["total_tokens"]=usage["prompt_tokens"]+usage["completion_tokens"]
                parts=[
                    {"choices":[{"index":0,"delta":{"role":"assistant","content":generated},
                                 "finish_reason":None}]},
                    {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":usage}
                ]
                if body.get("stream",True):
                    payload=("".join("data: "+json.dumps(p,ensure_ascii=False)+"\n\n" for p in parts)
                             +"data: [DONE]\n\n").encode()
                    self.send_response(200)
                    self.send_header("Content-Type","text/event-stream")
                else:
                    payload=json.dumps({"choices":[{"message":{"role":"assistant",
                        "content":generated},"finish_reason":"stop"}],"usage":usage}).encode()
                    self.send_response(200)
                    self.send_header("Content-Type","application/json")
                self.send_header("Content-Length",str(len(payload)))
                self.end_headers();self.wfile.write(payload)
            except Exception as e:
                payload=json.dumps({"error":str(e)}).encode()
                self.send_response(500);self.send_header("Content-Type","application/json")
                self.send_header("Content-Length",str(len(payload)))
                self.end_headers();self.wfile.write(payload)
    ThreadingHTTPServer(("127.0.0.1",cli.port),Handler).serve_forever()

if __name__=="__main__":
    try:main()
    except Exception as e:
        print("Promoted adapter sidecar failed:",str(e),file=sys.stderr)
        sys.exit(2)
