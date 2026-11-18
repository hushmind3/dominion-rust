#!/usr/bin/env python3
"""Benchmark adapter tests: zero API/model calls, real Git evidence."""
import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPT=Path(__file__).resolve().parents[1]/"dominion_adapter.py"
SPEC=importlib.util.spec_from_file_location("dominion_adapter",SCRIPT)
MODULE=importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)

def git(root,*args):
    process=subprocess.run(["git",*args],cwd=root,capture_output=True,text=True)
    if process.returncode:
        raise AssertionError(process.stderr)
    return process.stdout

class TestAdapter(unittest.TestCase):
    def test_only_real_run_worktree_paths_are_exported_into_exact_verifier_root(self):
        with tempfile.TemporaryDirectory() as temp:
            base=Path(temp)
            run=base/"run-uuid"
            verifier=base/"task"
            run.mkdir()
            verifier.mkdir()
            git(run,"init","-q")
            (run/"sum.py").write_text("print(sum(range(1, 5)))\n")
            (verifier/"sum.py").write_text("print(sum(range(1, 5)))\n")
            git(run,"add",".")
            git(run,"-c","user.name=Test","-c","user.email=test@example.invalid",
                "commit","-qm","baseline")
            assert MODULE.exported_paths(run)==[]
            (run/"sum.py").write_text("print(sum(range(1, 6)))\n")
            (run/"hello.txt").write_text("BENCH_OK")
            changed=MODULE.exported_paths(run)
            self.assertEqual(changed,["hello.txt","sum.py"])
            self.assertEqual(MODULE.apply_worktree_changes(run,verifier),2)
            self.assertEqual((verifier/"hello.txt").read_text(),"BENCH_OK")
            self.assertEqual((verifier/"sum.py").read_text(),
                "print(sum(range(1, 6)))\n")
            self.assertTrue((verifier/"hello.txt").is_file())
            # The source worktree remains isolated, not moved or adopted.
            self.assertTrue((run/"hello.txt").is_file())

    def test_required_paths_and_telemetry_are_distinct_from_success_claims(self):
        self.assertEqual(MODULE.safe_required_paths('["sum.py"]'),["sum.py"])
        with self.assertRaises(ValueError):
            MODULE.safe_required_paths('["../escape.py"]')
        with self.assertRaises(ValueError):
            MODULE.safe_required_paths('["sum.py","sum.py"]')
        run={"id":"run-1","status":"failed",
            "messages":[{"role":"assistant","tool_calls":[{"id":"call-1"}]}],
            "events":[{"seq":1,"kind":"model","detail":{}},
                {"seq":2,"kind":"tool_started","detail":{"name":"write_file",
                    "arguments":{"password":"never export"}}},
                {"seq":3,"kind":"tool_error","detail":{"code":"TEST"}}],
            "input_tokens":300,"output_tokens":20,"cached_input_tokens":100,
            "cost_krw":2.5,"delegated_spent_krw":1.0}
        usage=MODULE.observed_usage(run)
        self.assertEqual(usage["model_requests"],1)
        self.assertEqual(usage["tool_started"],1)
        self.assertEqual(usage["tool_calls"],1)
        self.assertEqual(usage["input_tokens"],300)
        self.assertEqual(usage["cost_krw"],3.5)
        with tempfile.TemporaryDirectory() as tmp:
            report=Path(tmp)/"evidence.json"
            MODULE.write_evidence(report,run,["sum.py"])
            text=report.read_text()
            self.assertNotIn("password",text)
            self.assertNotIn("never export",text)
            self.assertEqual(json.loads(text)["changed_files_exported"],["sum.py"])

if __name__=="__main__":
    unittest.main()
