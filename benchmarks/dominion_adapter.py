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
import signal
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
    # macOS exposes temporary trees through both /var and /private/var.
    # Canonicalize both sides before the path containment checks, while
    # still rejecting links or path traversal outside either workspace.
    worktree = worktree.resolve()
    destination = destination.resolve()
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


def safe_required_paths(raw):
    values = json.loads(raw)
    if not isinstance(values,list) or len(values)>24:
        raise ValueError("required_changed_paths must be a bounded list")
    unique = set()
    for path in values:
        if (not isinstance(path,str) or not path or len(path)>512
                or "\\" in path or Path(path).is_absolute()
                or any(segment in (".","..","") for segment in path.split("/"))
                or path in unique):
            raise ValueError("invalid required_changed_paths entry")
        unique.add(path)
    return values


def observed_usage(run):
    events = run.get("events",[])
    if not isinstance(events,list):events=[]
    messages = run.get("messages",[])
    if not isinstance(messages,list):messages=[]
    return {
        "run_id":run.get("id"),
        "status":run.get("status","unknown"),
        "model_requests":sum(event.get("kind")=="model" for event in events
            if isinstance(event,dict)),
        "tool_started":sum(event.get("kind")=="tool_started" for event in events
            if isinstance(event,dict)),
        "tool_calls":sum(len(message.get("tool_calls",[]))
            for message in messages if isinstance(message,dict)
            and isinstance(message.get("tool_calls"),list)),
        "input_tokens":run.get("input_tokens"),
        "cached_input_tokens":run.get("cached_input_tokens"),
        "cache_write_tokens":run.get("cache_write_tokens"),
        "output_tokens":run.get("output_tokens"),
        "cost_krw":run.get("cost_krw",0)+run.get("delegated_spent_krw",0),
    }


def write_evidence(file,run,changed):
    # Do not leak model messages, provider credentials or tool arguments to a ZIP.
    events = run.get("events",[])
    timeline=[]
    for event in events if isinstance(events,list) else []:
        if not isinstance(event,dict):continue
        details=event.get("detail")
        safe={}
        if isinstance(details,dict):
            safe={key:details[key] for key in
                ("name","call_id","id","code","succeeded","observed")
                if key in details}
        timeline.append({"seq":event.get("seq"),"kind":event.get("kind"),
            "at_ms":event.get("at_ms"),"tool":safe})
    file.write_text(json.dumps({
        "run_id":run.get("id"),"base_commit":run.get("base_commit"),
        "repository":run.get("repository"),
        "changed_files_exported":changed,"usage":observed_usage(run),
        "events":timeline
    },ensure_ascii=False,indent=2),encoding="utf-8")


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
    required = safe_required_paths(
        os.getenv("DOMINION_BENCH_REQUIRED_CHANGED_PATHS","[]"))
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
        "max_steps":args.max_steps,
        "required_changed_paths":required,
    })
    run_id = run["id"]
    # Publish the real run identity before waiting so the parent runner can
    # cancel a still-running server job even if this CLI is killed at timeout.
    args.telemetry_file.write_text(json.dumps({
        "run_id":run_id,"status":"running","model_requests":None,
        "input_tokens":None,"output_tokens":None
    },indent=2),encoding="utf-8")
    started = time.monotonic()
    terminal = False
    original_sigterm = signal.getsignal(signal.SIGTERM)
    def stop_on_signal(_signal,_frame):
        raise SystemExit(143)
    signal.signal(signal.SIGTERM,stop_on_signal)
    try:
        while time.monotonic()-started < args.timeout:
            run = api("GET","/api/v1/runs/"+run_id)
            args.telemetry_file.write_text(
                json.dumps(observed_usage(run),indent=2),encoding="utf-8")
            if run["status"] in ("completed","failed","cancelled","awaiting_approval"):
                break
            if run["status"]=="paused" and not run.get("waiting_for_children"):
                break
            time.sleep(0.25)
        else:
            raise RuntimeError("Dominion run reached benchmark time limit")
        if run["status"] != "completed":
            raise RuntimeError("Dominion run did not complete: "+run["status"])
        isolated = Path(root).resolve()/run_id
        changed_paths = exported_paths(isolated)
        # A claimed-complete agent with unchanged required files is still a
        # failed benchmark, even if another file was produced somewhere else.
        unmet = [path for path in required
            if path not in changed_paths or not (isolated/path).is_file()]
        if unmet:
            raise RuntimeError("required artifact files not changed in the Run worktree: "+
                ", ".join(unmet))
        changed = apply_worktree_changes(isolated,workspace)
        terminal = True
        write_evidence(args.telemetry_file.with_suffix(".evidence.json"),run,changed_paths)
        telemetry=observed_usage(run)
        args.telemetry_file.write_text(json.dumps(telemetry,indent=2),encoding="utf-8")
        print(json.dumps({
            "run_id":run_id,"status":run["status"],
            "changed_files_exported":changed,"changed_paths":changed_paths,
            "events":len(run.get("events",[])),
            "measured_usage":telemetry
        }))
    finally:
        signal.signal(signal.SIGTERM,original_sigterm)
        if not terminal:
            try:
                api("POST","/api/v1/runs/"+run_id+"/cancel",{})
            except (OSError,RuntimeError):
                pass
            try:
                snapshot=api("GET","/api/v1/runs/"+run_id)
                args.telemetry_file.write_text(
                    json.dumps(observed_usage(snapshot),indent=2),encoding="utf-8")
                write_evidence(args.telemetry_file.with_suffix(".evidence.json"),
                    snapshot,[])
            except (OSError,RuntimeError):
                pass


if __name__ == "__main__":
    try:main()
    except (RuntimeError,ValueError,HTTPError,URLError) as error:
        print("DOMINION BENCHMARK ADAPTER ERROR: "+str(error),file=sys.stderr)
        sys.exit(2)
