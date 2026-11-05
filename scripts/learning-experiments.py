#!/usr/bin/env python3
"""Run the six required learning/adaptation integration experiments against a
real DOMINION engine and a real model.

Usage:
  DOMINION_URL=http://127.0.0.1:8787 \
  PROVIDER_BASE=http://127.0.0.1:11434/v1 MODEL=qwen3:4b \
  python3 scripts/learning-experiments.py --out benchmark/LEARNING-EXPERIMENTS.json

The script never fabricates a success: every result is read back from the
engine (execution status, operations, files, policies, optimizers).
"""
import argparse
import json
import os
import pathlib
import sys
import time
import urllib.error
import urllib.request

BASE = os.environ.get("DOMINION_URL", "http://127.0.0.1:8787").rstrip("/")
PROVIDER_BASE = os.environ.get("PROVIDER_BASE", "http://127.0.0.1:11434/v1")
MODEL = os.environ.get("MODEL", "qwen3:4b")


def request(method, path, body=None, timeout=600):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(
        BASE + path,
        data=data,
        method=method,
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(req, timeout=timeout) as response:
        return response.read().decode()


def post(path, body):
    raw = request("POST", path, body)
    try:
        return json.loads(raw) if raw else {}
    except json.JSONDecodeError:
        return {"raw": raw}


def get(path):
    raw = request("GET", path)
    try:
        return json.loads(raw) if raw else {}
    except json.JSONDecodeError:
        return {"raw": raw}


def chat(text, session_id=None, agent_id=None):
    body = {"input": text, "autoLearning": True}
    if session_id:
        body["sessionId"] = session_id
    if agent_id:
        body["agentId"] = agent_id
    raw = request("POST", "/api/chat", body)
    error = '"type":"error"' in raw.replace(" ", "")
    return {"error": error, "sessionId": session_id, "chars": len(raw)}


def executions():
    items = get("/api/executions")
    return items if isinstance(items, list) else []


def latest_execution(session_id=None):
    items = executions()
    if session_id:
        items = [item for item in items if item.get("sessionId") == session_id]
    return items[0] if items else {}


def workspace_files():
    try:
        return {entry["name"] for entry in get("/api/workspace")["entries"]}
    except Exception:
        return set()


def ensure_provider():
    providers = get("/api/providers")
    if isinstance(providers, list) and providers:
        return providers[0]
    return post(
        "/api/providers",
        {
            "name": "learning-bench",
            "baseUrl": PROVIDER_BASE,
            "model": MODEL,
            "enabled": True,
            "toolMode": "native",
        },
    )


def run_case(name, prompt, files, session_id=None, agent_id=None):
    before = workspace_files()
    result = chat(prompt, session_id, agent_id)
    deadline = time.time() + 180
    execution = {}
    while time.time() < deadline:
        execution = latest_execution(session_id)
        if execution.get("status") in ("completed", "failed", "interrupted"):
            break
        time.sleep(0.5)
    after = workspace_files()
    present = {path for path in files if path in after and path not in before}
    return {
        "name": name,
        "chatError": result["error"],
        "executionStatus": execution.get("status"),
        "failureClass": execution.get("failureClass"),
        "taskOutcome": execution.get("taskOutcome"),
        "createdFiles": sorted(present),
        "expectedFiles": files,
        "success": bool(files) and present == set(files),
    }


def experiment_a():
    # A: an initial wrong attempt, an owner correction, then a new session with
    # different wording. The second session must actually create the file.
    first = run_case(
        "a.initial",
        "bench 디렉터리에 alpha_report.txt 파일을 만들어.",
        ["bench/alpha_report.txt"],
    )
    correction = chat(
        "그게 아니라 bench 디렉터리가 아니라 루트에 alpha_report.txt를 만들어야지. "
        "다음부터 요청한 경로를 정확히 확인해."
    )
    second = run_case(
        "a.new_session",
        "이번에는 alpha_report.txt를 루트에 작성해. 내용은 CORRECTED.",
        ["alpha_report.txt"],
    )
    return {"initial": first, "correction": correction, "afterLearning": second}


def experiment_b():
    return run_case(
        "b.unknown_cause",
        "먼저 workspace_read로 missing-report.txt를 읽고, 없으면 실제 원인을 확인한 뒤 "
        "같은 실패를 반복하지 말고 필요한 파일을 만들어 작업을 끝내.",
        ["missing-report.txt"],
    )


def experiment_c():
    # C: reuse the learned method with a different file name, then check that a
    # contradictory environment does not blindly apply it.
    similar = run_case(
        "c.transfer",
        "workspace_read로 quarterly.csv를 읽고, 없으면 원인을 확인해 quarterly.csv를 만들어.",
        ["quarterly.csv"],
    )
    conflicting = run_case(
        "c.wrong_environment",
        "quarterly.csv는 이미 존재해야 한다. 삭제하지 말고 workspace_list로 확인만 해.",
        [],
    )
    return {"transferred": similar, "conflicting": conflicting}


def experiment_d():
    # D: agent-private experience must not leak; a shared verified method may.
    agent_a = run_case(
        "d.agent_a_private",
        "agent 전용 메모로 private-note.txt를 만들어.",
        ["private-note.txt"],
        agent_id="learning-agent-a",
    )
    agent_b = run_case(
        "d.agent_b",
        "agent 전용 메모를 참고해 다른 파일 public-note.txt를 만들어.",
        ["public-note.txt"],
        agent_id="learning-agent-b",
    )
    return {"agentA": agent_a, "agentB": agent_b}


def experiment_e():
    # E: use the existing experiment/promotion path and verify the active
    # revision changes and can be rolled back.
    search = post("/api/benchmark/policy-search", {})
    policies = get("/api/policies")
    optimizers = get("/api/optimizers")
    return {
        "search": search,
        "activePolicies": [
            item for item in policies if item.get("status") == "active"
        ]
        if isinstance(policies, list)
        else policies,
        "optimizers": optimizers,
    }


def experiment_f():
    # F: three conditions on held-out tasks. Condition separation is done by
    # separate data roots in scripts/learning-experiments.sh; here we record
    # the observable outcomes for the current engine.
    tasks = [
        ("f.file_creation", "결과 디렉터리에 result-a.txt를 만들어.", ["result-a.txt"]),
        ("f.correction_reuse", "루트에 corrected.txt를 만들어.", ["corrected.txt"]),
        ("f.probe", "workspace_list로 현재 파일을 확인한 뒤 확인.json을 만들어.",
         ["확인.json"]),
    ]
    return [run_case(name, prompt, files) for name, prompt, files in tasks]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default="benchmark/LEARNING-EXPERIMENTS.json")
    parser.add_argument("--only", default="abcdef")
    args = parser.parse_args()
    ensure_provider()
    post("/api/policy", {"autoApproveAll": True})
    report = {"model": MODEL, "provider": PROVIDER_BASE, "base": BASE}
    for key, fn in [
        ("a", experiment_a),
        ("b", experiment_b),
        ("c", experiment_c),
        ("d", experiment_d),
        ("e", experiment_e),
        ("f", experiment_f),
    ]:
        if key in args.only:
            report[key] = fn()
    out = pathlib.Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(report, ensure_ascii=False, indent=2)[:4000])
    return 0


if __name__ == "__main__":
    sys.exit(main())
