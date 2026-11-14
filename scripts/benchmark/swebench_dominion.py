#!/usr/bin/env python3
"""Run REAL Dominion autonomous-goal executions and export SWE-bench patches.

No simulated model, oracle, fabricated prediction or claimed resolved score.
Input: JSONL exported from princeton-nlp/SWE-bench_Verified (test split).
Requires a configured template of Dominion state with real provider settings.
Exact Docker-based grading is delegated to the official swebench harness.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from urllib.error import HTTPError, URLError
from urllib.request import Request, urlopen

TERMINAL = {"completed", "failed", "cancelled", "needs_review"}
DATASET = "princeton-nlp/SWE-bench_Verified"


def git(*args: str, cwd: Path | None = None, timeout: int = 240) -> str:
    done = subprocess.run(["git", *args], cwd=cwd, check=True, capture_output=True,
                          text=True, timeout=timeout)
    return done.stdout.strip()


def api(base: str, path: str, token: str | None = None,
        data: dict | None = None, timeout: int = 30) -> dict | list:
    headers = {"Accept": "application/json"}
    if token:
        headers["X-Dominion-Token"] = token
    if data is not None:
        headers["Content-Type"] = "application/json"
    payload = None if data is None else json.dumps(data).encode("utf-8")
    request = Request(base + path, headers=headers, data=payload,
                      method="POST" if data is not None else "GET")
    try:
        with urlopen(request, timeout=timeout) as response:
            result = json.loads(response.read(16 * 1024 * 1024))
        if not isinstance(result, (dict, list)):
            raise ValueError(f"{path}: unexpected response shape")
        return result
    except HTTPError as exc:
        detail = exc.read(2048).decode("utf-8", "replace")
        raise RuntimeError(f"HTTP {exc.code} {path}: {detail}") from exc


def checkout(row: dict, cache: Path, work: Path, offline: bool) -> str:
    repo = row["repo"]
    commit = row["base_commit"]
    if not isinstance(repo, str) or len(repo.split("/")) != 2:
        raise ValueError("Each official instance requires repo='owner/name'")
    if not isinstance(commit, str) or len(commit) != 40 or any(
        x not in "0123456789abcdefABCDEF" for x in commit
    ):
        raise ValueError(f"{repo}: invalid 40-character base_commit")
    mirror = cache / (repo.replace("/", "__") + ".git")
    mirror.parent.mkdir(parents=True, exist_ok=True)
    if not mirror.exists():
        if offline:
            raise FileNotFoundError(f"Offline mirror missing: {mirror}")
        git("clone", "--quiet", "--mirror", f"https://github.com/{repo}.git", str(mirror),
            timeout=1200)
    try:
        git("--git-dir", str(mirror), "cat-file", "-e", commit + "^{commit}")
    except subprocess.CalledProcessError:
        if offline:
            raise
        git("--git-dir", str(mirror), "fetch", "--quiet", "origin", commit, timeout=600)
    git("clone", "--quiet", "--shared", "--no-checkout", str(mirror), str(work))
    git("checkout", "--quiet", "--detach", commit, cwd=work)
    actual = git("rev-parse", "HEAD", cwd=work)
    if actual != commit:
        raise RuntimeError("Untrusted source: checkout commit differs from dataset")
    if git("status", "--porcelain", cwd=work):
        raise RuntimeError("Dataset checkout is not clean")
    return actual


def unused_local_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def wait_healthy(base: str, process: subprocess.Popen, token: str | None) -> None:
    deadline = time.monotonic() + 45
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"Dominion exited during startup (code {process.returncode})")
        try:
            api(base, "/api/health", token, timeout=1)
            return
        except (RuntimeError, URLError, OSError, TimeoutError, ValueError):
            time.sleep(0.25)
    raise TimeoutError("Dominion did not start in 45 seconds")


def launch(binary: Path, work: Path, data: Path, log: Path, token: str | None):
    port = unused_local_port()
    env = os.environ.copy()
    env.update({"DOMINION_PORT": str(port), "DOMINION_WORKSPACE": str(work),
                "DOMINION_DATA": str(data), "DOMINION_WEB": str(work)})
    # Token is inherited from a configured template or explicit environment,
    # never supplied in a URL or saved into public benchmark artifacts.
    if token is not None:
        env["DOMINION_ADMIN_TOKEN"] = token
    handle = log.open("wb")
    proc = subprocess.Popen([str(binary), "--no-browser"], cwd=work,
                            env=env, stdout=handle, stderr=subprocess.STDOUT)
    return proc, handle, f"http://127.0.0.1:{port}"


def stop(proc: subprocess.Popen, log_handle) -> None:
    if proc.poll() is None:
        proc.terminate()
        try:
            proc.wait(timeout=7)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait(timeout=5)
    log_handle.close()


def execution_metrics(base: str, token: str | None, goal: dict) -> dict:
    session = goal.get("sessionId")
    metrics = {"calls": None, "toolCalls": None, "tokens": None,
               "costQuoteKrw": None, "costQuoteKnownCalls": None,
               "replans": goal.get("replans"), "iterations": goal.get("iterations")}
    if not isinstance(session, str) or not session:
        return metrics
    try:
        use = api(base, f"/api/sessions/{session}/usage", token)
        metrics.update({"calls": use.get("calls"), "tokens": use.get("totalTokens"),
                        "costQuoteKrw": use.get("costQuoteKrw"),
                        "costQuoteKnownCalls": use.get("costQuoteKnownCalls")})
        executions = api(base, "/api/executions?limit=200&offset=0", token)
        if isinstance(executions, list):
            calls = 0
            for execution in executions:
                if execution.get("sessionId") != session:
                    continue
                ops = api(base, f"/api/executions/{execution['id']}/operations", token)
                if isinstance(ops, list):
                    calls += len([op for op in ops if op.get("status") == "completed"])
            metrics["toolCalls"] = calls
    except (RuntimeError, URLError, KeyError):
        pass  # Leave the metric unknown; never count a failed query as zero.
    return metrics


def validated_patch(payload: dict, expected_base: str) -> str:
    if payload.get("source") != "isolated_git_worktree_complete_patch_v2":
        raise ValueError("Expected exact isolated Git worktree evidence")
    if payload.get("complete") is not True or payload.get("baseCommit") != expected_base:
        raise ValueError("Patch origin or completeness mismatch")
    untracked = payload.get("untrackedFiles")
    if not isinstance(untracked,int) or untracked < 0 or untracked > 128:
        raise ValueError("Invalid count of included new files")
    patch = payload.get("modelPatch")
    if not isinstance(patch, str):
        raise ValueError("Exact modelPatch must be a string")
    raw = patch.encode("utf-8")
    if len(raw) != payload.get("bytes"):
        raise ValueError("Patch byte count differs from original")
    if hashlib.sha256(raw).hexdigest() != payload.get("sha256"):
        raise ValueError("Patch SHA does not match original source")
    return patch


def run_one(row: dict, args, root: Path) -> tuple[dict, dict]:
    ident = str(row["instance_id"])
    safe = ident.replace("/", "_")
    scratch = root / safe
    scratch.mkdir(parents=True, exist_ok=False)
    workspace = scratch / "repo"
    data = scratch / "state"
    shutil.copytree(args.state_template, data)
    original = checkout(row, args.cache, workspace, args.offline)
    token = args.admin_token
    proc, handle, base = launch(args.binary, workspace, data, scratch / "server.log", token)
    started = time.monotonic()
    status, goal, patch, error = "not_started", {}, "", None
    try:
        wait_healthy(base, proc, token)
        description = row.get("problem_statement", "")
        if not isinstance(description, str) or not description.strip():
            raise ValueError("Official problem_statement is empty")
        request = {"goal": description, "isolateWorktree": True,
                   "maxIterations": args.max_iterations, "maxReplans": args.max_replans,
                   "executor": {"providerId": args.provider_id, "model": args.model}}
        goal = api(base, "/api/goals", token, request)
        goal_id = goal["id"]
        deadline = time.monotonic() + args.timeout
        while time.monotonic() < deadline:
            goal = api(base, f"/api/goals/{goal_id}", token)
            status = str(goal.get("status", "unknown"))
            if status in TERMINAL:
                break
            if proc.poll() is not None:
                raise RuntimeError("Agent process exited before a terminal goal")
            time.sleep(args.poll_interval)
        else:
            api(base, f"/api/goals/{goal_id}/cancel", token, {})
            status = "timeout"
            goal = api(base, f"/api/goals/{goal_id}", token)
        if status in TERMINAL:
            try:
                result = api(base, f"/api/goals/{goal_id}/patch", token)
                patch = validated_patch(result, original)
            except (RuntimeError, KeyError, ValueError) as exc:
                error = "patch_export: " + str(exc)
        metrics = execution_metrics(base, token, goal)
    except Exception as exc:
        error = str(exc)
        metrics = {"calls": None, "toolCalls": None, "tokens": None,
                   "costQuoteKrw": None, "costQuoteKnownCalls": None,
                   "replans": goal.get("replans"), "iterations": goal.get("iterations")}
    finally:
        stop(proc, handle)
    prediction = {"instance_id": ident, "model_name_or_path": args.model_name,
                  "model_patch": patch}
    receipt = {"instanceId": ident, "baseCommit": original, "status": status,
               "goalId": goal.get("id"), "elapsedSeconds": round(time.monotonic() - started, 2),
               "patchBytes": len(patch.encode("utf-8")), "patchSha256":
                   hashlib.sha256(patch.encode("utf-8")).hexdigest() if patch else None,
               "error": error, "metrics": metrics,
               "resolved": None, "resolvedRequiresOfficialDockerGrader": True}
    return prediction, receipt


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tasks-jsonl", type=Path, required=True,
                        help="Official SWE-bench Verified rows exported as JSONL")
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--state-template", type=Path, required=True,
                        help="Configured Dominion state with provider secrets; copied per task")
    parser.add_argument("--provider-id", required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--model-name", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--cache", type=Path, default=Path(".dominion-benchmark/repos"))
    parser.add_argument("--offline", action="store_true")
    parser.add_argument("--max-instances", type=int, default=0)
    parser.add_argument("--timeout", type=int, default=1800)
    parser.add_argument("--max-iterations", type=int, default=16)
    parser.add_argument("--max-replans", type=int, default=6)
    parser.add_argument("--poll-interval", type=float, default=1.0)
    parser.add_argument("--admin-token", default=os.getenv("DOMINION_ADMIN_TOKEN"))
    parser.add_argument("--run-official", action="store_true",
                        help="Invoke the actual Docker SWE-bench Verified grader after generation")
    parser.add_argument("--max-workers", type=int, default=2)
    args = parser.parse_args()
    args.binary = args.binary.resolve()
    args.state_template = args.state_template.resolve()
    args.cache = args.cache.resolve()
    if not args.binary.is_file() or not args.state_template.is_dir():
        parser.error("A built Dominion executable and a configured state template are required")
    if args.timeout < 1 or args.max_iterations not in range(1, 65):
        parser.error("timeout and max-iterations are out of range")
    args.output.mkdir(parents=True, exist_ok=True)
    tasks = [json.loads(line) for line in args.tasks_jsonl.read_text().splitlines()
             if line.strip()]
    if args.max_instances > 0:
        tasks = tasks[:args.max_instances]
    if len({row["instance_id"] for row in tasks}) != len(tasks):
        parser.error("Duplicate instance IDs are forbidden")
    preds, receipts = [], []
    with tempfile.TemporaryDirectory(prefix="dominion-swebench-") as temp:
        for index, row in enumerate(tasks):
            print(f"[{index+1}/{len(tasks)}] {row['instance_id']}", flush=True)
            try:
                prediction, receipt = run_one(row, args, Path(temp))
            except Exception as exc:
                prediction = {"instance_id": row["instance_id"],
                    "model_name_or_path": args.model_name, "model_patch": ""}
                receipt = {"instanceId": row["instance_id"], "status": "setup_failed",
                    "error": str(exc), "resolved": None}
            preds.append(prediction)
            receipts.append(receipt)
            # Persist after EVERY task, including errors, before moving on.
            (args.output / "predictions.jsonl").write_text(
                "".join(json.dumps(r, ensure_ascii=False)+"\n" for r in preds))
            (args.output / "traces.jsonl").write_text(
                "".join(json.dumps(r, ensure_ascii=False)+"\n" for r in receipts))
    summary = {"dataset": DATASET, "model": args.model_name,
               "attemptedInstances": len(tasks),
               "nonemptyPatches": sum(bool(p["model_patch"]) for p in preds),
               "officialResolved": None, "officialEvaluated": False,
               "meanTokensObserved": None}
    observed = [r.get("metrics",{}).get("tokens") for r in receipts]
    known = [v for v in observed if isinstance(v,(int,float))]
    if known:
        summary["meanTokensObserved"] = sum(known)/len(known)
        summary["tokensObservedSamples"] = len(known)
    if args.run_official:
        run_id = "dominion_" + hashlib.sha256(
            (args.model_name + (args.output/"predictions.jsonl").read_text()).encode()
        ).hexdigest()[:12]
        # Distinct run id for every changed patch set prevents SWE-bench's
        # cached report from masquerading as a fresh evaluation.
        command = [sys.executable,"-m","swebench.harness.run_evaluation",
                   "--dataset_name", DATASET, "--split","test",
                   "--predictions_path",str(args.output/"predictions.jsonl"),
                   "--max_workers",str(args.max_workers),"--run_id",run_id]
        done = subprocess.run(command,cwd=args.output,check=False)
        summary["officialExitCode"] = done.returncode
        report = args.output/"logs"/"evaluation"/run_id/"results.json"
        if done.returncode == 0 and report.is_file():
            summary["officialEvaluated"] = True
            summary["officialReport"] = json.loads(report.read_text())
        else:
            summary["officialError"] = "Docker-based evaluator failed; no resolved score claimed"
    (args.output/"summary.json").write_text(
        json.dumps(summary,indent=2,ensure_ascii=False)+"\n")
    print(json.dumps(summary,ensure_ascii=False,indent=2))
    return 0 if not args.run_official or summary["officialEvaluated"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
