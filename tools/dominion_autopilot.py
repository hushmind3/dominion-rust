#!/usr/bin/env python3
"""Independent, bounded parallel development cycles across DOMINION branches.

This runs locally, not inside ChatGPT. It never resets watched checkouts,
force-pushes, merges branches, launches daemons, or grants a judge write access.
"""
from __future__ import annotations

import argparse
import concurrent.futures
import datetime as dt
import json
import os
import contextlib
import signal
import threading
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import time

BRANCHES = ("main", "dominion-sub2", "dominion-sub3", "dominion-sub4")
ROLES = {"main": "메인", "dominion-sub2": "서브2", "dominion-sub3": "서브3", "dominion-sub4": "서브4"}
STOP = threading.Event()
WORKTREE_LOCK = threading.Lock()
ROLE_FOCUS = {
    "main": "Maintain the trusted release product and manually curated main UI. Audit chat continuity, completion, runtime budgets, and regressions; never automatically merge a competitor.",
    "dominion-sub2": "Improve common Driver/Harness contract, Room participant cursors, Team DAG/remote workers, and evidence-preserving continuation without copying main wholesale.",
    "dominion-sub3": "Improve independent Rust SDK, Native+Chat wire/stream parity, approval/recovery, and keyboard/mobile execution UX; keep current product menus.",
    "dominion-sub4": "Improve single durable Run admission, lightweight chat, Room source ownership, Team worker/patch UX, and real-model context efficiency without invented benchmarks.",
}
WATCHED = {
    "dominion-sub2": Path("/private/tmp/dominion-sub2-live"),
    "dominion-sub3": Path("/private/tmp/dominion-sub3-live"),
    "dominion-sub4": Path("/private/tmp/sub4-live"),
}



@contextlib.contextmanager
def singleton_lock(path: Path):
    """OS-released advisory lock: never run two coordinators on one state root."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a+") as f:
        if os.name == "nt":
            import msvcrt
            f.seek(0)
            try:
                msvcrt.locking(f.fileno(), msvcrt.LK_NBLCK, 1)
            except OSError as exc:
                raise RuntimeError("another DOMINION orchestrator is active") from exc
        else:
            import fcntl
            try:
                fcntl.flock(f.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError as exc:
                raise RuntimeError("another DOMINION orchestrator is active") from exc
        try:
            yield
        finally:
            if os.name == "nt":
                f.seek(0)
                msvcrt.locking(f.fileno(), msvcrt.LK_UNLCK, 1)
            else:
                fcntl.flock(f.fileno(), fcntl.LOCK_UN)


def forbidden_file(path: str) -> bool:
    """Do not automatically publish common local credentials/state artifacts."""
    name = Path(path).name.lower()
    return (name in {".env", "auth.json", "id_rsa", "id_ed25519", ".npmrc", ".pypirc"}
            or name.startswith(".env.") and not name.endswith(".example")
            or name.endswith((".pem", ".p12", ".key", ".sqlite", ".sqlite3")))


def git(repo: Path, *args: str, check: bool = True) -> str:
    p = subprocess.run(["git", "-C", str(repo), *args], text=True, capture_output=True)
    if check and p.returncode:
        raise RuntimeError(f"git {' '.join(args)}: {p.stderr.strip() or p.stdout.strip()}")
    return p.stdout.strip()


def stop_process(p: subprocess.Popen) -> None:
    if p.poll() is not None:
        return
    try:
        if os.name != "nt":
            os.killpg(p.pid, signal.SIGTERM)
        else:
            p.terminate()
    except ProcessLookupError:
        return
    try:
        p.wait(timeout=3)
    except subprocess.TimeoutExpired:
        try:
            if os.name != "nt":
                os.killpg(p.pid, signal.SIGKILL)
            else:
                p.kill()
        except ProcessLookupError:
            pass
        p.wait()


def invoke(command: list[str], cwd: Path, log: Path, timeout: int, env: dict[str, str],
           *, separate_stderr: bool = False) -> tuple[int, bool]:
    log.parent.mkdir(parents=True, exist_ok=True)
    with log.open("w", encoding="utf-8") as stream:
        errfile = None
        try:
            if STOP.is_set():
                stream.write("[orchestrator] cancelled before launch\n")
                return 130, False
            errfile = log.with_suffix(".stderr.log").open("w", encoding="utf-8") if separate_stderr else None
            p = subprocess.Popen(command, cwd=cwd, env=env, stdout=stream,
                                 stderr=errfile if errfile else subprocess.STDOUT,
                                 start_new_session=(os.name != "nt"))
            deadline = time.monotonic() + timeout
            while True:
                remaining = deadline - time.monotonic()
                if STOP.is_set() or remaining <= 0:
                    stop_process(p)
                    stream.write("[orchestrator] cancelled\n" if STOP.is_set() else "[orchestrator] agent timed out\n")
                    return (130 if STOP.is_set() else 124), not STOP.is_set()
                try:
                    return p.wait(timeout=min(0.5, remaining)), False
                except subprocess.TimeoutExpired:
                    continue
        except OSError as e:
            stream.write(f"[orchestrator] process start failed: {e}\n")
            return 127, False
        finally:
            if errfile:
                errfile.close()


def command_from_template(template: str | None, prompt: str, *, judge: bool = False) -> list[str]:
    if template:
        parts = shlex.split(template)
        if not parts or not any("{prompt}" in part for part in parts):
            raise ValueError("Command template must include {prompt} as an argument")
        return [part.replace("{prompt}", prompt) for part in parts]
    return ["codex", "exec", "--sandbox", "read-only" if judge else "workspace-write",
            "--ephemeral", prompt]


def prompt_for(branch: str, previous_feedback: str, base: str, goal: str = "") -> str:
    return f"""You are the independent DOMINION {ROLES[branch]} developer, assigned ONLY branch {branch}.
This is an autonomous development cycle in a detached, isolated git worktree at {base[:12]}.
Branch-specific context: {ROLE_FOCUS[branch]}
ONE SHARED OWNER GOAL FOR ALL FOUR CANDIDATES: {goal.strip() if goal.strip() else '(not specified; choose independently)'}
If a shared owner goal is supplied, deliver that goal on YOUR branch using your native design.
If absent, identify and implement the highest-value user-facing deficiency independently.
Your task is to independently identify the highest-value real user problem or missing capability,
trace its actual caller through Core/Harness/Strategy/Host/UI, implement a usable improvement,
remove newly obsolete paths, run targeted regressions, and commit only your own verified changes.
Do not wait for further user messages. Do not touch another branch or any sibling worktree.
Do not force-push, merge, reset user data, read private credentials, launch background daemons,
run paid model evaluations or modify external accounts. CI completion is not the goal.
Preserve the main UI design language and top-level navigation. Prefer actual user task completion
and evidence over cosmetic refactoring. No invented results or success claims.
Before editing, inspect current files and git status. Keep the repository clean and do not
stage unrelated, secret or generated files. End by reporting implementation, tests actually run,
remaining gaps, and commit SHA. If blocked, record the precise blocker.
Review the user's latest role rules if they exist in the repository.
Previous independent judge feedback (may be empty or unverified):
{previous_feedback[:12000] or '(first round; independently select the most valuable improvement)'}"""


def prepare(repo: Path, state: Path, branch: str, round_id: str,
            previous: dict | None = None) -> tuple[Path, str, str]:
    # Never checkout, reset, or mutate the user's watched application tree.
    remote_ref = f"refs/remotes/origin/{branch}"
    remote_sha = git(repo, "rev-parse", "--verify", remote_ref)
    sha = remote_sha
    if previous:
        prior_status = previous.get("status")
        if prior_status in {"agent_failed", "needs_review_uncommitted",
                            "needs_review_sensitive_paths", "needs_review_watched_checkout_dirty",
                            "push_conflict_or_error", "error", "interrupted"}:
            raise RuntimeError("prior work needs review; keeping its detached worktree intact")
        if prior_status in {"committed_local", "committed_local_no_new_changes"}:
            prior_head = previous.get("head", "")
            ancestor = subprocess.run(["git", "-C", str(repo), "merge-base", "--is-ancestor",
                                       remote_sha, prior_head], capture_output=True)
            if ancestor.returncode:
                raise RuntimeError("remote diverged from unpushed local commits; refusing to discard them")
            sha = prior_head
    target = state / "worktrees" / round_id / branch
    if target.exists():
        raise RuntimeError(f"Worktree already exists; refusing to overwrite: {target}")
    target.parent.mkdir(parents=True, exist_ok=True)
    # Git worktree registration uses shared metadata; serialize this small step.
    with WORKTREE_LOCK:
        git(repo, "worktree", "add", "--detach", str(target), sha)
    return target, sha, remote_sha


def run_branch(repo: Path, state: Path, branch: str, round_id: str, feedback: str,
               timeout: int, template: str | None, publish: bool,
               previous: dict | None = None, goal: str = "") -> dict:
    out = state / "runs" / round_id / branch
    out.mkdir(parents=True, exist_ok=True)
    result: dict = {"branch": branch, "round": round_id, "status": "not_started"}
    started = time.monotonic()
    try:
        target, base, remote_sha = prepare(repo, state, branch, round_id, previous)
        result.update({"base": base, "remote_base": remote_sha, "worktree": str(target)})
        prompt = prompt_for(branch, feedback, base, goal)
        (out / "prompt.txt").write_text(prompt, encoding="utf-8")
        command = command_from_template(template, prompt)
        env = {**os.environ, "DOMINION_BRANCH": branch, "DOMINION_ROLE": "developer"}
        code, timed_out = invoke(command, target, out / "agent.log", timeout, env)
        head = git(target, "rev-parse", "HEAD")
        porcelain = git(target, "status", "--porcelain")
        changes = git(target, "diff", "--name-only", f"{base}..HEAD").splitlines()
        risky = [path for path in changes if forbidden_file(path)]
        result.update({"exit_code": code, "timeout": timed_out, "head": head,
                       "risky_paths": risky,
                       "changed_commits": git(target, "rev-list", "--count", f"{base}..HEAD"),
                       "diff_stat": git(target, "diff", "--stat", f"{base}..HEAD"),
                       "dirty": bool(porcelain), "dirty_paths": porcelain[:2000]})
        # A successful agent process does not imply task success or clean code.
        if STOP.is_set():
            result["status"] = "interrupted"
        elif timed_out or code:
            result["status"] = "agent_failed"
        elif porcelain:
            result["status"] = "needs_review_uncommitted"
        elif risky:
            result["status"] = "needs_review_sensitive_paths"
        elif head == remote_sha:
            result["status"] = "no_commit"
        else:
            result["status"] = "committed_local" if head != base else "committed_local_no_new_changes"
            if publish:
                watched = repo if branch == "main" else WATCHED.get(branch)
                if watched and watched.is_dir() and git(watched, "status", "--porcelain"):
                    result["status"] = "needs_review_watched_checkout_dirty"
                    result["publish_blocked"] = "watched user checkout contains uncommitted changes"
                else:
                    # Normal fast-forward-only push. A concurrently updated remote wins.
                    push = subprocess.run(["git", "-C", str(target), "push", "origin",
                                           f"HEAD:refs/heads/{branch}"], text=True, capture_output=True)
                    result["push_detail"] = (push.stderr or push.stdout)[-2000:]
                    result["status"] = "published" if push.returncode == 0 else "push_conflict_or_error"
        result["elapsed_seconds"] = round(time.monotonic() - started, 1)
    except Exception as e:
        result.update({"status": "error", "error": str(e), "elapsed_seconds": round(time.monotonic() - started, 1)})
    (out / "result.json").write_text(json.dumps(result, indent=2, ensure_ascii=False), encoding="utf-8")
    return result


def judge_round(repo: Path, state: Path, round_id: str, results: list[dict],
                timeout: int, template: str | None, goal: str = "") -> dict:
    folder = state / "runs" / round_id / "judge"
    folder.mkdir(parents=True, exist_ok=True)
    evidence = json.dumps(results, indent=2, ensure_ascii=False)
    prompt = f"""You are the independent DOMINION judge. Read-only. Never modify any branch or git file.
Judge 4 competing products against the same rubric (completion 20, run/recovery 15,
context/Room/Team 15, model/tool/SDK 10, architecture 10, UI/UX 15, measured cost/latency 10,
frontier implementation 5). CI status and code volume confer ZERO bonus.
Shared user task for this round: {goal.strip() if goal.strip() else '(none specified; compare general product progress)'}
Source evidence for round {round_id}: {evidence}
You may inspect each listed detached worktree's code and the actual agent logs.
Do not invent runtime successes, numerical grades, costs or UI improvements: those require
common scenarios and real results. Distinguish code review, tests, and live-model E2E.
Return exactly one JSON object with keys:
"summary" (string), "feedback" (object mapping all four branch names to concrete actionable
Korean feedback), "verified" (array), "missing_evidence" (array), "recommendations" (array).
Prioritize branch-specific next improvements and note any severe safety risks. No markdown fences.
"""
    (folder / "prompt.txt").write_text(prompt, encoding="utf-8")
    # Judge sees the same frozen source snapshot, never a watched checkout.
    judge_tree = state / "worktrees" / round_id / "judge"
    base = next((r.get("base") for r in results if r.get("branch") == "main" and r.get("base")), None)
    if not base:
        report = {"round": round_id, "status": "judge_skipped_missing_main_snapshot", "feedback": {}}
        (folder / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
        return report
    git(repo, "worktree", "add", "--detach", str(judge_tree), base)
    cmd = command_from_template(template, prompt, judge=True)
    env = {**os.environ, "DOMINION_ROLE": "judge"}
    code, timed_out = invoke(cmd, judge_tree, folder / "judge.log", timeout, env, separate_stderr=True)
    raw = (folder / "judge.log").read_text(encoding="utf-8", errors="replace")
    # CLI final output is normally stdout; strip only well-formed fences.
    cleaned = raw.strip()
    if cleaned.startswith("```"):
        cleaned = cleaned.split("\n", 1)[-1].rsplit("```", 1)[0].strip()
    report: dict = {"round": round_id, "exit_code": code, "timeout": timed_out,
                    "status": "unverified_judge_output", "feedback": {}}
    if code == 0 and not timed_out:
        try:
            parsed = json.loads(cleaned)
            if isinstance(parsed, dict) and isinstance(parsed.get("feedback"), dict):
                report.update({"status": "judge_feedback_recorded", "analysis": parsed,
                               "feedback": {b: str(parsed["feedback"].get(b, "")) for b in BRANCHES}})
        except (TypeError, ValueError):
            pass
    (folder / "report.json").write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    return report


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description="4-branch DOMINION development + independent judge, one command")
    p.add_argument("--repo", required=True, type=Path, help="Git clone containing all origin branch refs")
    p.add_argument("--state", type=Path, help="Outside-repo durable logs/worktrees; defaults to adjacent directory")
    p.add_argument("--rounds", type=int, default=1, help="1=one full cycle; 0=run cycles until interrupted")
    p.add_argument("--parallel", type=int, default=4, choices=(1, 2, 3, 4))
    p.add_argument("--timeout", type=int, default=2700, help="Per agent and judge timeout in seconds")
    p.add_argument("--cooldown", type=int, default=30, help="Seconds between rounds; 0 disables delay")
    p.add_argument("--agent-command", help='Optional argv template, e.g. "codex exec --sandbox workspace-write {prompt}"')
    p.add_argument("--judge-command", help='Optional argv template, e.g. "codex exec --sandbox read-only {prompt}"')
    p.add_argument("--publish", action="store_true", help="Fast-forward push each committed branch, never force/merge")
    p.add_argument("--goal", default="", help="One common owner task sent unchanged to all four candidates")
    p.add_argument("--skip-judge", action="store_true")
    args = p.parse_args(argv)
    if args.rounds < 0 or args.timeout < 5 or args.cooldown < 0:
        p.error("--rounds must be >= 0, --timeout >= 5, --cooldown >= 0")
    repo = args.repo.resolve()
    if not repo.is_dir() or git(repo, "rev-parse", "--show-toplevel") != str(repo):
        p.error("--repo must be the top-level directory of a Git checkout")
    state = (args.state or (repo.parent / "dominion-autopilot-state")).resolve()
    if state == repo or repo in state.parents:
        p.error("--state must be outside the target repository")
    state.mkdir(parents=True, exist_ok=True)
    for t in (args.agent_command, args.judge_command):
        if t:
            command_from_template(t, "probe")
    if not args.agent_command and shutil.which("codex") is None:
        p.error("codex not found; install/sign in to Codex CLI or set --agent-command")
    if not args.skip_judge and not args.judge_command and shutil.which("codex") is None:
        p.error("codex not found for read-only judge; set --judge-command or --skip-judge")
    STOP.clear()
    iteration = 0
    old_handler = signal.getsignal(signal.SIGINT)
    signal.signal(signal.SIGINT, lambda _sig, _frame: STOP.set())
    try:
        with singleton_lock(state / "autopilot.lock"):
            while (args.rounds == 0 or iteration < args.rounds) and not STOP.is_set():
                iteration += 1
                round_id = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%S%fZ")
                print(f"[DOMINION] round {iteration}: {round_id}", flush=True)
                # Only fetch changes; never update a checked-out branch.
                git(repo, "fetch", "--no-tags", "origin", *(f"+refs/heads/{b}:refs/remotes/origin/{b}" for b in BRANCHES))
                feedback_path = state / "latest_feedback.json"
                feedback = json.loads(feedback_path.read_text(encoding="utf-8")) if feedback_path.is_file() else {}
                prior_path = state / "latest_round.json"
                prior_runs = json.loads(prior_path.read_text(encoding="utf-8")).get("results", []) if prior_path.is_file() else []
                prior_by_branch = {r["branch"]: r for r in prior_runs if "branch" in r}
                with concurrent.futures.ThreadPoolExecutor(max_workers=args.parallel) as pool:
                    futures = [pool.submit(run_branch, repo, state, b, round_id,
                                           str(feedback.get(b, "")), args.timeout,
                                           args.agent_command, args.publish, prior_by_branch.get(b), args.goal) for b in BRANCHES]
                    results = [future.result() for future in futures]
                print("[DOMINION] " + " | ".join(f"{r['branch']}:{r['status']}" for r in results), flush=True)
                if STOP.is_set():
                    break
                if not args.skip_judge:
                    report = judge_round(repo, state, round_id, results, args.timeout, args.judge_command, args.goal)
                    if report["status"] == "judge_feedback_recorded":
                        feedback_path.write_text(json.dumps(report["feedback"], ensure_ascii=False, indent=2), encoding="utf-8")
                    print(f"[DOMINION] judge: {report['status']}", flush=True)
                (state / "latest_round.json").write_text(json.dumps({"id": round_id, "results": results},
                                                                      ensure_ascii=False, indent=2), encoding="utf-8")
                if (args.rounds == 0 or iteration < args.rounds) and args.cooldown:
                    STOP.wait(args.cooldown)
    except Exception as e:
        print(f"[DOMINION] stopped: {e}", file=sys.stderr)
        return 1
    finally:
        signal.signal(signal.SIGINT, old_handler)
    if STOP.is_set():
        print("[DOMINION] stopped by user; detached worktrees/logs preserved", file=sys.stderr)
        return 130
    return 0


if __name__ == "__main__":
    raise SystemExit(main())