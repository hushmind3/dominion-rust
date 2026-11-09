#!/usr/bin/env python3
"""Bounded, isolated text/native Responses replay comparison.

Reads the local preview and committed participant frames; never writes to the
running engine. Credentials and prompt/output text stay in memory. Only usage,
hashes, item types and correctness booleans are written to the result file.
This is an experiment, not a cache warm-up or a product transport.
"""
import argparse
import hashlib
import json
import pathlib
import sqlite3
import time
import urllib.error
import urllib.request
import uuid


def digest(value):
    return hashlib.sha256(json.dumps(value, ensure_ascii=False, sort_keys=True).encode()).hexdigest()


def text_item(role, text):
    return {"role": role, "content": [{"type": "output_text" if role == "assistant" else "input_text", "text": text}]}


def call(url, headers, body):
    request = urllib.request.Request(url, data=json.dumps(body).encode(), headers=headers)
    started = time.monotonic()
    completed = None
    items = []
    with urllib.request.urlopen(request, timeout=90) as response:
        for line in response:
            if not line.startswith(b"data:"):
                continue
            raw = line[5:].strip()
            if raw == b"[DONE]":
                break
            event = json.loads(raw)
            if event.get("type") == "response.output_item.done":
                items.append(event["item"])
            if event.get("type") == "response.completed":
                completed = event["response"]
            if event.get("type") in ("response.failed", "error"):
                raise RuntimeError("Provider stream failed; response body withheld")
    if completed is None:
        raise RuntimeError("No completed response")
    output = completed.get("output") or items
    text = "".join(block.get("text", "") for item in output if item.get("type") == "message"
                   for block in item.get("content", []) if block.get("type") == "output_text")
    return completed, output, text, round(time.monotonic() - started, 3)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database", default="runtime/dominion.sqlite")
    parser.add_argument("--engine", default="http://127.0.0.1:4317")
    parser.add_argument("--agents", nargs="+", default=["dominion-ainz"])
    parser.add_argument("--rounds", type=int, default=4, choices=range(2, 7))
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    auth = json.loads((pathlib.Path.home() / ".codex/auth.json").read_text())["tokens"]
    headers = {"Authorization": "Bearer " + auth["access_token"],
               "chatgpt-account-id": auth["account_id"], "OpenAI-Beta": "responses=experimental",
               "originator": "codex_cli_rs", "User-Agent": "codex_cli_rs/0.0.0", "Content-Type": "application/json"}
    db = sqlite3.connect(pathlib.Path(args.database).resolve().as_uri() + "?mode=ro", uri=True)
    records = [json.loads(row[0]) for row in db.execute("select data from records where kind='participant_conversation'")]
    rows = []
    report = {"isolated": True, "productionWrites": 0, "model": "gpt-6-luna", "effort": "low",
              "learningCalls": 0, "learningLimitation": "Transport-only paired comparison; automatic learning is not exercised.",
              "rows": rows}
    output_path = pathlib.Path(args.output)
    def save():
        output_path.parent.mkdir(parents=True, exist_ok=True)
        output_path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    try:
        for agent in args.agents:
            preview = json.load(urllib.request.urlopen(args.engine + "/api/rooms/nazarick-circle/system-prompt?agentId=" + agent))
            participant = next(r for r in records if r.get("sessionId") == "room:nazarick-circle" and r.get("agentId") == agent)
            seed = []
            for frame in participant["workingView"]["frames"]:
                message = frame["message"]
                if message.get("content", "").strip():
                    role = "assistant" if message["role"] == "assistant" else "user"
                    seed.append(text_item(role, message["content"]))
            modes = {}
            for mode in ["text", "native"]:
                session = uuid.uuid4().hex
                modes[mode] = {"input": list(seed), "session": session, "key": "dominion_probe_" + session}
            marker = "확인어" + uuid.uuid4().hex[:8]
            for turn in range(1, args.rounds + 1):
                prompt = ("격리된 전송 실험이다. 확인어는 " + marker + ". 답은 확인어 하나만 써." if turn == 1
                          else "방금 정한 확인어 하나만 답해.")
                # Alternate order so a single mode does not always run first.
                for mode in (["text", "native"] if turn % 2 else ["native", "text"]):
                    state = modes[mode]
                    state["input"].append(text_item("user", prompt))
                    body = {"model": "gpt-6-luna", "instructions": preview["prompt"], "input": state["input"],
                            "stream": True, "store": False, "reasoning": {"effort": "low"},
                            "prompt_cache_key": state["key"]}
                    if mode == "native":
                        body["include"] = ["reasoning.encrypted_content"]
                    wire_headers = dict(headers, session_id=state["session"], **{"x-client-request-id": state["key"]})
                    completed, native, text, elapsed = call("https://chatgpt.com/backend-api/codex/responses", wire_headers, body)
                    usage = completed.get("usage", {})
                    detail = usage.get("input_tokens_details", {})
                    row = {"agent": agent, "turn": turn, "mode": mode, "seconds": elapsed,
                           "instructionsDigest": digest(preview["prompt"]), "inputDigest": digest(body["input"]),
                           "usage": usage, "correct": text.strip() == marker,
                           "outputTypes": [item.get("type") for item in native],
                           "encryptedReasoningPresent": any(bool(item.get("encrypted_content")) for item in native),
                           "servedModel": completed.get("model")}
                    rows.append(row)
                    print(json.dumps({"agent": agent, "turn": turn, "mode": mode, "input": usage.get("input_tokens"),
                                      "cached": detail.get("cached_tokens"), "correct": row["correct"]}), flush=True)
                    state["input"].extend(native if mode == "native" else [text_item("assistant", text)])
                    save()
    except urllib.error.HTTPError as error:
        report["error"] = {"httpStatus": error.code, "detail": "Provider body withheld"}
        save()
        raise SystemExit("Provider HTTP " + str(error.code))
    except Exception as error:
        report["error"] = {"type": type(error).__name__, "detail": "Exception details withheld"}
        save()
        raise SystemExit("Probe failed: " + type(error).__name__)
    save()


if __name__ == "__main__":
    main()
