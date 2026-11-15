#!/usr/bin/env python3
"""Matched-workspace CLI benchmark: independent tests and observed cost only."""
import argparse
import json
import os
import pathlib
import shutil
import statistics
import subprocess
import sys
import tempfile
import time


def run_command(argv, cwd, timeout, env):
    start = time.monotonic()
    try:
        proc = subprocess.run(argv, cwd=cwd, env=env, text=True,
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                              timeout=timeout, check=False)
        return {"returncode": proc.returncode,
                "seconds": round(time.monotonic()-start, 3),
                "stderr": proc.stderr[-1200:],
                "timeout": False}
    except subprocess.TimeoutExpired:
        return {"returncode": None, "seconds": round(time.monotonic()-start, 3),
                "stderr": "time limit exceeded", "timeout": True}


def safe_child(root, relative):
    candidate = (root/relative).resolve()
    if candidate == root or root not in candidate.parents:
        raise ValueError("path escapes evaluation workspace")
    return candidate


def compare(manifest, directory, attempts, result_root):
    if manifest.get("version") != 1:
        raise ValueError("unsupported evaluation schema")
    model = manifest.get("model")
    budget = manifest.get("budget_krw")
    limit = int(manifest.get("max_steps", 20))
    timeout = int(manifest.get("timeout_seconds", 180))
    if not isinstance(model, str) or not model.strip():
        raise ValueError("one model must be explicitly configured")
    if not isinstance(budget, (int,float)) or budget < 0:
        raise ValueError("one nonnegative budget is required")
    if not 1 <= limit <= 200 or not 1 <= timeout <= 1800:
        raise ValueError("step or time limit out of bounds")
    harnesses = manifest.get("harnesses", [])
    tasks = manifest.get("tasks", [])
    if not harnesses or not tasks or not 1 <= attempts <= 5:
        raise ValueError("harness/tasks required; repeats 1..5")
    result_root.mkdir(parents=True, exist_ok=True)
    outcomes = []
    for task in tasks:
        fixture = safe_child(directory, task["fixture"])
        if not fixture.is_dir():
            raise ValueError("missing fixture: " + str(fixture))
        for harness in harnesses:
            if harness.get("model", model) != model:
                raise ValueError("cannot compare different declared models")
            argv = harness.get("argv")
            if not isinstance(argv, list) or not argv:
                raise ValueError("harness commands must be argv arrays")
            for repeat in range(attempts):
                index = len(outcomes)
                workspace = result_root/(str(index).zfill(4)+"-"+task["id"])
                shutil.copytree(fixture,workspace,symlinks=False)
                prompt = workspace/".dominion-benchmark-task.txt"
                prompt.write_text(task["prompt"],encoding="utf-8")
                telemetry = workspace/".dominion-benchmark-usage.json"
                fields = {
                    "workspace":str(workspace),"prompt_file":str(prompt),
                    "telemetry_file":str(telemetry),"model":model,
                    "dominion_adapter":str((pathlib.Path(__file__).resolve().parent/"dominion_adapter.py")),
                    "budget_krw":str(budget),"max_steps":str(limit)
                }
                def expand(values):
                    return [str(v).format_map(fields) for v in values]
                env = os.environ.copy()
                env.update({
                    "DOMINION_BENCH_MODEL":model,
                    "DOMINION_WORKSPACE_DIR":str(result_root),
                    "DOMINION_BENCH_WORKSPACE":str(workspace),
                    "DOMINION_BENCH_BUDGET_KRW":str(budget),
                    "DOMINION_BENCH_MAX_STEPS":str(limit),
                    "DOMINION_BENCH_TELEMETRY_FILE":str(telemetry)
                })
                ran = run_command(expand(argv),workspace,timeout,env)
                # The agent must not be able to rewrite its own acceptance
                # tests. Detect tampering before restoring trusted verifier
                # files from the immutable fixture.
                protected = task.get("protected_paths", ["tests"])
                tampered = False
                for name in protected:
                    trusted = safe_child(fixture,name)
                    candidate = safe_child(workspace,name)
                    if trusted.is_file():
                        original = trusted.read_bytes()
                        tampered |= not candidate.is_file() or candidate.read_bytes()!=original
                        candidate.parent.mkdir(parents=True,exist_ok=True)
                        shutil.copy2(trusted,candidate)
                    elif trusted.is_dir():
                        original_files = {
                            str(f.relative_to(trusted)):f.read_bytes()
                            for f in trusted.rglob("*") if f.is_file()
                        }
                        workspace_files = {
                            str(f.relative_to(candidate)):f.read_bytes()
                            for f in candidate.rglob("*") if f.is_file()
                        } if candidate.is_dir() else {}
                        tampered |= original_files != workspace_files
                        if candidate.exists():shutil.rmtree(candidate)
                        shutil.copytree(trusted,candidate)
                verified = run_command(expand(task["verify"]),workspace,
                                       min(timeout,120),env)
                artifacts = all(safe_child(workspace,p).exists()
                                for p in task.get("artifacts",[]))
                passed = ran["returncode"] == 0 and verified["returncode"] == 0 and artifacts and not tampered
                usage = None
                if telemetry.exists():
                    try:
                        data = json.loads(telemetry.read_text(encoding="utf-8"))
                        usage = {key:data[key] for key in (
                            "tool_calls","input_tokens","cached_input_tokens",
                            "output_tokens","cost_krw"
                        ) if isinstance(data.get(key),(int,float)) and data[key] >= 0}
                    except (OSError,ValueError):
                        usage = None
                outcomes.append({
                    "harness":harness["name"],"task":task["id"],
                    "repeat":repeat,"passed":passed,"seconds":ran["seconds"],
                    "returncode":ran["returncode"],"verify_code":verified["returncode"],
                    "timed_out":ran["timeout"],"artifacts":artifacts,
                    "protected_files_modified":tampered,
                    "usage":usage,"run_error":ran["stderr"],
                    "verify_error":verified["stderr"]
                })
    summary = []
    for harness in harnesses:
        own = [r for r in outcomes if r["harness"] == harness["name"]]
        costs = [r["usage"]["cost_krw"] for r in own
                 if r["usage"] is not None and "cost_krw" in r["usage"]]
        delays = sorted(r["seconds"] for r in own)
        summary.append({
            "harness":harness["name"],"attempts":len(own),
            "passed":sum(r["passed"] for r in own),
            "success_rate":round(sum(r["passed"] for r in own)/len(own),4),
            "median_seconds":round(statistics.median(delays),3),
            "p95_seconds":delays[min(len(delays)-1,int(len(delays)*0.95))],
            "measured_cost_krw":round(sum(costs),4)
                if len(costs)==len(own) else None,
            "model_and_budget_enforced":bool(harness.get("limits_enforced",False))
        })
    return {
        "schema":"dominion-harness-matrix-v1",
        "model":model,"budget_krw":budget,"max_steps":limit,
        "timeout_seconds":timeout,"repeats":attempts,
        "note":"Declared comparable settings; billing/tool limits are not proven unless both harness adapters enforce them.",
        "summary":summary,"attempts":outcomes
    }


def selftest():
    with tempfile.TemporaryDirectory(prefix="dominion-matrix-") as scratch:
        root = pathlib.Path(scratch)
        fixture = root/"fixture"
        fixture.mkdir()
        (fixture/"base.txt").write_text("baseline",encoding="utf-8")
        writer = [sys.executable,"-c",
                  "from pathlib import Path;Path('proof.txt').write_text('yes')"]
        checker = [sys.executable,"-c",
                   "from pathlib import Path;assert Path('proof.txt').read_text()=='yes'"]
        spec = {"version":1,"model":"local-mock-not-a-frontier-score",
                "budget_krw":0,"max_steps":1,"timeout_seconds":8,
                "harnesses":[{"name":"fixture-A","argv":writer},
                             {"name":"fixture-B","argv":writer}],
                "tasks":[{"id":"smoke","fixture":"fixture","prompt":"write proof",
                          "verify":checker,"artifacts":["proof.txt"]}]}
        out = compare(spec,root,2,root/"out")
        assert len(out["attempts"]) == 4
        assert all(r["passed"] and r["usage"] is None for r in out["attempts"])
        assert all(r["measured_cost_krw"] is None for r in out["summary"])
        assert (fixture/"base.txt").read_text()=="baseline"
        print(json.dumps({"runner_selftest":"PASS","attempts":4,"real_model_score":None}))


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--matrix",type=pathlib.Path)
    ap.add_argument("--output",type=pathlib.Path,
                    default=pathlib.Path("benchmarks/results/matched.json"))
    ap.add_argument("--repeats",type=int,default=1)
    ap.add_argument("--selftest",action="store_true")
    args = ap.parse_args()
    if args.selftest:
        selftest()
        return
    if args.matrix is None:
        ap.error("a manifest is required")
    config = args.matrix.resolve()
    output = args.output.resolve()
    result = compare(json.loads(config.read_text(encoding="utf-8")),
                     config.parent,args.repeats,output.parent/"workspaces")
    output.parent.mkdir(parents=True,exist_ok=True)
    output.write_text(json.dumps(result,ensure_ascii=False,indent=2),
                      encoding="utf-8")
    print(json.dumps({"summary":result["summary"],"report":str(output)},
                     ensure_ascii=False))


if __name__=="__main__":
    main()
