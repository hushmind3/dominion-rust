#!/usr/bin/env python3
"""Bridge matched benchmark fixtures to a running real Dominion Rust HTTP API.

This adapter never auto-approves shell/MCP operations. It exports the isolated
Git worktree's changes into the verifier workspace only after run completion.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen


def api(method, path, payload=None):
    url = os.getenv("DOMINION_BENCH_API_URL", "http://127.0.0.1:8787").rstrip("/") + path
    headers = {"Accept": "application/json"}
    if payload is not None:headers["Content-Type"] = "application/json"
    token = os.getenv("DOMINION_ACCESS_TOKEN", "")
    if token:headers["Authorization"] = "Bearer " + token
    body = json.dumps(payload).encode("utf-8") if payload is not None else None
    try:
        with urlopen(Request(url,data=body,headers=headers,method=method),timeout=20) as response:
            return json.load(response)
    except HTTPError as error:
        detail = error.read().decode("utf-8","replace")[:1200]
        raise RuntimeError(f"{method} {path} rejected ({error.code}): {detail}") from error


def git(cwd, *args):
    command = ["git", "-C", str(cwd), *args]
    proc = subprocess.run(command,capture_output=True,text=True,check=False,timeout=30)
    if proc.returncode:
        raise RuntimeError("git command failed: "+proc.stderr[-400:])
    return proc.stdout


def ensure_fixture_git(workspace):
    if (workspace/".git").exists():
        git(workspace, "rev-parse", "HEAD")
        return
    git(workspace, "init", "-q")
    git(workspace, "add", ".")
    git(workspace, "-c", "user.name=DominionBenchmark",
        "-c", "user.email=bench@example.invalid",
        "commit", "-qm", "baseline fixture")


def exported_paths(worktree):
    tracked = git(worktree,"diff","--name-only","--no-ext-diff","HEAD","--").splitlines()
    new = git(worktree,"ls-files","--others","--exclude-standard").splitlines()
    return sorted(set(tracked + new))


def apply_worktree_changes(worktree, destination):
    count = 0
    for name in exported_paths(worktree):
        if not name or name.startswith(".git") or ".." in Path(name).parts:
            raise RuntimeError("invalid agent-produced changed path")
        from_file = (worktree/name).resolve()
        to_file = (destination/name).resolve()
        if worktree not in from_file.parents or destination not in to_file.parents:
            raise RuntimeError("agent output escapes benchmark workspace")
        if from_file.is_file():
            to_file.parent.mkdir(parents=True,exist_ok=True)
            shutil.copy2(from_file,to_file)
        elif not from_file.exists() and to_file.is_file():
            to_file.unlink()
        else:
            raise RuntimeError("unrecognized modified output path")
        count += 1
    return count


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace",type=Path,required=True)
    parser.add_argument("--prompt-file",type=Path,required=True)
    parser.add_argument("--model",required=True)
    parser.add_argument("--budget-krw",type=float,required=True)
    parser.add_argument("--max-steps",type=int,required=True)
    parser.add_argument("--telemetry-file",type=Path,required=True)
    parser.add_argument("--timeout",type=int,default=270)
    args = parser.parse_args()
    workspace = args.workspace.resolve()
    prompt = args.prompt_file.resolve().read_text(encoding="utf-8")
    root = os.getenv("DOMINION_WORKSPACE_DIR")
    if not root:
        raise RuntimeError("DOMINION_WORKSPACE_DIR must equal benchmark workspace parent")
    if Path(root).resolve() != workspace.parent:
        raise RuntimeError("Rust server workspace root must be the same benchmark workspace parent")
    if args.max_steps < 1 or args.budget_krw < 0:
        raise ValueError("invalid budget or step limit")
    config = api("GET","/api/v1/config")
    if config.get("model") != args.model or not config.get("configured"):
        raise RuntimeError("real Rust provider must be configured to same benchmark model")
    ensure_fixture_git(workspace)
    run = api("POST","/api/v1/runs/from-repository",{
        "instruction":prompt,
        "repository":workspace.name,
        "budget_krw":args.budget_krw,
        "max_steps":args.max_steps
    })
    run_id = run["id"]
    started = time.monotonic()
    while time.monotonic()-started < args.timeout:
        run = api("GET","/api/v1/runs/"+run_id)
        if run["status"] in ("completed","failed","cancelled","awaiting_approval"):
            break
        if run["status"]=="paused" and not run.get("waiting_for_children"):
            break
        time.sleep(0.25)
    else:
        api("POST","/api/v1/runs/"+run_id+"/cancel",{})
        raise RuntimeError("Dominion run reached benchmark time limit")
    isolated = Path(root).resolve()/run_id
    if run["status"] != "completed":
        raise RuntimeError("Dominion run did not complete: "+run["status"])
    changed = apply_worktree_changes(isolated,workspace)
    details = run.get("messages",[])
    events = run.get("events",[])
    telemetry = {
        "tool_calls":sum(len(m.get("tool_calls",[]))
            for m in details if isinstance(m,dict) and isinstance(m.get("tool_calls"),list)),
        "input_tokens":run.get("input_tokens",0),
        "cached_input_tokens":run.get("cached_input_tokens",0),
        "output_tokens":run.get("output_tokens",0),
        "cost_krw":run.get("cost_krw",0)+run.get("delegated_spent_krw",0)
    }
    args.telemetry_file.write_text(json.dumps(telemetry,indent=2),encoding="utf-8")
    print(json.dumps({
        "run_id":run_id,"status":run["status"],
        "changed_files_exported":changed,"events":len(events),
        "measured_usage":telemetry
    }))


if __name__ == "__main__":
    try:main()
    except (RuntimeError,ValueError,HTTPError,URLError) as error:
        print("DOMINION BENCHMARK ADAPTER ERROR: "+str(error),file=sys.stderr)
        sys.exit(2)
