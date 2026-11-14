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
import re
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


def instance_scratch_name(instance_id: str) -> str:
    """Deterministic directory without trusting a dataset-supplied path."""
    if not isinstance(instance_id, str) or not instance_id or len(instance_id) > 1024:
        raise ValueError("Invalid benchmark instance ID")
    return "case-" + hashlib.sha256(instance_id.encode("utf-8")).hexdigest()[:24]


def checkout(row: dict, cache: Path, work: Path, offline: bool) -> str:
    repo = row["repo"]
    commit = row["base_commit"]
    if (not isinstance(repo, str) or len(repo.split("/")) != 2
            or any(re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,99}", part) is None
                   for part in repo.split("/"))):
        raise ValueError("Each official instance requires a safe repo='owner/name'")
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
    metrics = {"calls": None, "toolCalls": None, "successfulToolCalls": None,
               "failedToolCalls": None, "uncertainToolCalls": None,
               "tokens": None, "costQuoteKrw": None,
               "costQuoteKnownCalls": None,
               "replans": goal.get("replans"), "iterations": goal.get("iterations")}
    if not isinstance(session, str) or not session:
        return metrics
    try:
        use = api(base, f"/api/sessions/{session}/usage", token)
        metrics.update({"calls": use.get("calls"), "tokens": use.get("totalTokens"),
                        "costQuoteKrw": use.get("costQuoteKrw"),
                        "costQuoteKnownCalls": use.get("costQuoteKnownCalls")})
        # Count *attempts*, not only successes. A failed tool invocation is
        # still a tool call and must not disappear from cost/recovery metrics.
        # Paginate because lengthy agent tasks may generate >200 executions.
        operations = []
        offset = 0
        while True:
            executions = api(base, f"/api/executions?limit=1000&offset={offset}", token)
            if not isinstance(executions, list):
                raise ValueError("Execution listing is not an array")
            for execution in executions:
                if execution.get("sessionId") != session:
                    continue
                ops = api(base, f"/api/executions/{execution['id']}/operations", token)
                if not isinstance(ops, list):
                    raise ValueError("Operation listing is not an array")
                operations.extend(ops)
            if len(executions) < 1000:
                break
            offset += 1000
        metrics.update(tool_operation_metrics(operations))
    except (RuntimeError, URLError, KeyError, TypeError, ValueError):
        pass  # Leave the metric unknown; never count a failed query as zero.
    return metrics


def tool_operation_metrics(operations: list[dict]) -> dict:
    """Receipt-only counts; no inference from model text or UI progress."""
    statuses = [op.get("status") for op in operations if isinstance(op, dict)]
    return {
        "toolCalls": len(statuses),
        "successfulToolCalls": sum(status == "completed" for status in statuses),
        "failedToolCalls": sum(status in ("failed", "error") for status in statuses),
        "uncertainToolCalls": sum(status in ("interrupted", "running", "pending")
                                  for status in statuses),
    }


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
    scratch = root / instance_scratch_name(ident)
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
                   "requireTests": True,
                   "maxIterations": args.max_iterations, "maxReplans": args.max_replans,
                   "executor": {"providerId": args.provider_id, "model": args.model}}
        if args.token_budget is not None:
            request["tokenBudget"] = args.token_budget
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
        metrics = {"calls": None, "toolCalls": None, "successfulToolCalls": None,
                   "failedToolCalls": None, "uncertainToolCalls": None,
                   "tokens": None, "costQuoteKrw": None,
                   "costQuoteKnownCalls": None,
                   "replans": goal.get("replans"), "iterations": goal.get("iterations")}
    finally:
        stop(proc, handle)
    prediction = {"instance_id": ident, "model_name_or_path": args.model_name,
                  "model_patch": patch}
    metrics["completedWithPriorToolFailures"] = (
        status == "completed" and
        isinstance(metrics.get("failedToolCalls"), int) and
        metrics["failedToolCalls"] > 0
    ) if metrics.get("failedToolCalls") is not None else None
    metrics["configuredGoalTokenBudget"] = args.token_budget
    if isinstance(metrics.get("tokens"),(int,float)) and args.token_budget:
        metrics["tokenBudgetOverrunObserved"] = max(
            0,metrics["tokens"]-args.token_budget)
    receipt = {"instanceId": ident, "baseCommit": original, "status": status,
               "goalId": goal.get("id"), "elapsedSeconds": round(time.monotonic() - started, 2),
               "patchBytes": len(patch.encode("utf-8")), "patchSha256":
                   hashlib.sha256(patch.encode("utf-8")).hexdigest() if patch else None,
               "error": error, "metrics": metrics,
               "resolved": None, "resolvedRequiresOfficialDockerGrader": True}
    return prediction, receipt


def official_evaluator_command(
    output: Path, run_id: str, max_workers: int, instance_ids: list[str]
) -> list[str]:
    """The authoritative grader must evaluate exactly this task cohort."""
    if not instance_ids or len(set(instance_ids)) != len(instance_ids):
        raise ValueError("Official evaluator needs a nonempty unique cohort")
    return [
        sys.executable, "-m", "swebench.harness.run_evaluation",
        "--dataset_name", DATASET, "--split", "test",
        "--predictions_path", str(output / "predictions.jsonl"),
        "--max_workers", str(max_workers), "--run_id", run_id,
        "--instance_ids", *instance_ids,
    ]


def official_score(report: dict, expected_instances: set[str]) -> dict:
    """Only score the exact submitted instance IDs from the official judge."""
    if not isinstance(report, dict) or report.get("schema_version") != 2:
        raise ValueError("Unsupported official judge report schema")
    resolved = report.get("resolved_ids")
    submitted = report.get("submitted_ids")
    if not isinstance(resolved, list) or not isinstance(submitted, list):
        raise ValueError("Official judge did not supply per-instance result evidence")
    if len(set(submitted)) != len(submitted) or set(submitted) != expected_instances:
        raise ValueError("Official judge evaluated a different instance cohort")
    if len(set(resolved)) != len(resolved) or not set(resolved).issubset(expected_instances):
        raise ValueError("Official judge resolved IDs differ from submitted tasks")
    if report.get("resolved_instances") != len(resolved):
        raise ValueError("Official report count disagrees with resolved evidence")
    return {
        "officialResolved": len(resolved),
        "officialDenominator": len(expected_instances),
        "officialResolvedRate": round(len(resolved)/len(expected_instances), 6)
            if expected_instances else None,
        "officialResolvedIds": sorted(resolved),
        "officialReportSchema": 2,
    }


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
    parser.add_argument("--token-budget", type=int, default=100000,
                        help="Per-instance aggregate token ceiling in the native Rust goal")
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
    # The official evaluator runs with cwd=args.output; relative prediction
    # paths would otherwise be interpreted a second time beneath that cwd.
    args.output = args.output.resolve()
    if not args.binary.is_file() or not args.state_template.is_dir():
        parser.error("A built Dominion executable and a configured state template are required")
    if args.token_budget is not None and not 1000 <= args.token_budget <= 2_000_000:
        parser.error("token-budget must be between 1000 and 2000000")
    if args.timeout < 1 or args.max_iterations not in range(1, 65):
        parser.error("timeout and max-iterations are out of range")
    if args.max_workers < 1 or args.poll_interval <= 0 or args.max_replans < 0:
        parser.error("max-workers, poll-interval and max-replans must be valid")
    args.output.mkdir(parents=True, exist_ok=True)
    tasks_source = args.tasks_jsonl.resolve()
    tasks_source_hash = hashlib.sha256(tasks_source.read_bytes()).hexdigest()
    tasks = [json.loads(line) for line in tasks_source.read_text().splitlines()
             if line.strip()]
    if args.max_instances > 0:
        tasks = tasks[:args.max_instances]
    if not tasks:
        parser.error("No official instances selected; refusing an empty benchmark")
    if len({row["instance_id"] for row in tasks}) != len(tasks):
        parser.error("Duplicate instance IDs are forbidden")
    # Compare products only under a pinned binary, exact task cohort and
    # matching limits; never store credentials or provider secrets in the run.
    manifest = {
        "datasetClaim": DATASET,
        "datasetRowsSha256": tasks_source_hash,
        "instanceIds": [row["instance_id"] for row in tasks],
        "binarySha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
        "modelName": args.model_name, "model": args.model,
        "providerId": args.provider_id, "tokenBudget": args.token_budget,
        "maxIterations": args.max_iterations, "maxReplans": args.max_replans,
        "timeoutSeconds": args.timeout, "maxWorkers": args.max_workers,
        "officialDockerRequested": bool(args.run_official),
        "note": "This manifest identifies input bytes, not an independently confirmed official dataset origin",
    }
    (args.output / "run-manifest.json").write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False) + "\n")
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
               "binarySha256": manifest["binarySha256"],
               "datasetRowsSha256": tasks_source_hash,
               "attemptedInstances": len(tasks),
               "nonemptyPatches": sum(bool(p["model_patch"]) for p in preds),
               "officialResolved": None, "officialEvaluated": False,
               "meanTokensObserved": None}
    observed = [r.get("metrics",{}).get("tokens") for r in receipts]
    known = [v for v in observed if isinstance(v,(int,float))]
    if known:
        summary["meanTokensObserved"] = sum(known)/len(known)
        summary["tokensObservedSamples"] = len(known)
    witnessed = [r for r in receipts
                 if isinstance(r.get("metrics", {}).get("failedToolCalls"), int)]
    failure_cases = [r for r in witnessed if r["metrics"]["failedToolCalls"] > 0]
    summary["observedFailureCases"] = len(failure_cases)
    summary["observedRecoveryCases"] = sum(
        r["status"] == "completed" for r in failure_cases)
    summary["observedFailureRecoveryRate"] = (
        summary["observedRecoveryCases"] / len(failure_cases)
        if failure_cases else None)
    summary["toolCallSamples"] = len(witnessed)
    summary["totalToolCallsObserved"] = sum(
        r["metrics"]["toolCalls"] for r in witnessed)
    if args.run_official:
        run_id = "dominion_" + hashlib.sha256((
            args.model_name + manifest["binarySha256"] + tasks_source_hash +
            (args.output / "predictions.jsonl").read_text()
        ).encode()).hexdigest()[:16]
        # SWE-bench caches by run_id and instance_id. Pin both the patch set
        # AND the agent binary/task cohort, not just the displayed model name.
        command = official_evaluator_command(
            args.output, run_id, args.max_workers,
            [row["instance_id"] for row in tasks])
        summary["officialRunId"] = run_id
        done = subprocess.run(command,cwd=args.output,check=False)
        summary["officialExitCode"] = done.returncode
        # The official evaluator has shipped both report layouts. Keep the
        # run-specific name in both candidates; never read an unrelated or
        # previous run's report as a new score.
        report_candidates = [
            args.output/"logs"/"evaluation"/run_id/"results.json",
            args.output/(args.model_name.replace("/", "__") + f".{run_id}.json"),
        ]
        report = next((path for path in report_candidates if path.is_file()), None)
        if done.returncode == 0 and report is not None:
            try:
                official = json.loads(report.read_text())
                score = official_score(official, {p["instance_id"] for p in preds})
                summary.update(score)
                summary["officialEvaluated"] = True
                summary["officialReport"] = official
                summary["officialReportPath"] = str(report)
            except (ValueError, KeyError, json.JSONDecodeError) as exc:
                summary["officialError"] = ("Official grader result did not validate: " + str(exc))
        else:
            summary["officialError"] = "Docker-based evaluator failed; no resolved score claimed"
    (args.output/"summary.json").write_text(
        json.dumps(summary,indent=2,ensure_ascii=False)+"\n")
    print(json.dumps(summary,ensure_ascii=False,indent=2))
    return 0 if not args.run_official or summary["officialEvaluated"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
