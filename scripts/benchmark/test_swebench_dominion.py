"""Offline contract tests; no test here claims a SWE-bench resolved task."""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).with_name("swebench_dominion.py")
SPEC = importlib.util.spec_from_file_location("dominion_swebench_runner", ROOT)
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


class ExactPatchContract(unittest.TestCase):
    def setUp(self):
        self.patch = "diff --git a/a.py b/a.py\n--- a/a.py\n+++ b/a.py\n"
        self.base = "a" * 40
        self.value = {
            "source": "isolated_git_worktree_complete_patch_v2",
            "complete": True, "baseCommit": self.base, "untrackedFiles": 0,
            "modelPatch": self.patch, "bytes": len(self.patch.encode("utf-8")),
            "sha256": hashlib.sha256(self.patch.encode("utf-8")).hexdigest(),
        }

    def test_only_exact_worktree_source_passes(self):
        self.assertEqual(runner.validated_patch(self.value, self.base), self.patch)

    def test_complete_new_file_patch_is_admissible(self):
        self.value["untrackedFiles"] = 1
        self.assertEqual(runner.validated_patch(self.value, self.base), self.patch)

    def test_wrong_commit_is_never_graded(self):
        with self.assertRaises(ValueError):
            runner.validated_patch(self.value, "b" * 40)

    def test_truncated_or_tampered_patch_is_never_graded(self):
        for key, replacement in (
            ("modelPatch", self.patch[:-1]), ("bytes", 0),
            ("sha256", "0" * 64), ("source", "ui_clipped_diff_v1"),
            ("complete", False), ("untrackedFiles", -1),
        ):
            invalid = dict(self.value)
            invalid[key] = replacement
            with self.subTest(key=key), self.assertRaises(ValueError):
                runner.validated_patch(invalid, self.base)

    def test_instance_ids_cannot_escape_scratch_root(self):
        for unsafe in ("..", "../../outside", "/root/private", "folder/../private"):
            scratch = runner.instance_scratch_name(unsafe)
            self.assertTrue(scratch.startswith("case-"))
            self.assertNotIn("/", scratch)
            self.assertNotIn(".", scratch)
            self.assertEqual(len(scratch), 29)
        self.assertEqual(runner.instance_scratch_name("x"),
                         runner.instance_scratch_name("x"))
        self.assertNotEqual(runner.instance_scratch_name("x"),
                            runner.instance_scratch_name("y"))
        for invalid in ("", "x" * 1025):
            with self.assertRaises(ValueError):
                runner.instance_scratch_name(invalid)

    def test_repository_owner_and_name_reject_path_or_url_injection(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            for unsafe in ("../escape", "owner/..", "owner/bad@host",
                           "owner/repo?x=1", "owner/name\\\\with-slash"):
                with self.subTest(repo=unsafe), self.assertRaises(ValueError):
                    runner.checkout({"repo":unsafe,"base_commit":"a" * 40},
                                    root / "mirror", root / "work", True)
                self.assertFalse((root / "work").exists())

    def test_invalid_commit_blocks_checkout_before_git_network(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            with self.assertRaises(ValueError):
                runner.checkout({"repo": "owner/name", "base_commit": "invalid"},
                                root / "cache", root / "task", True)
            self.assertFalse((root / "task").exists())

    def test_official_evaluation_command_pins_the_submitted_cohort(self):
        output = Path("/tmp/dominion-contract-output")
        command = runner.official_evaluator_command(
            output, "dominion_binary_pinned", 2,
            ["astropy__astropy-14539", "sympy__sympy-20590"])
        self.assertIn("--dataset_name", command)
        self.assertEqual(command[command.index("--dataset_name") + 1],
                         runner.DATASET)
        self.assertEqual(command[command.index("--predictions_path") + 1],
                         str(output / "predictions.jsonl"))
        self.assertEqual(command[-3:],
                         ["--instance_ids", "astropy__astropy-14539",
                          "sympy__sympy-20590"])
        with self.assertRaises(ValueError):
            runner.official_evaluator_command(output, "same", 1, [])
        with self.assertRaises(ValueError):
            runner.official_evaluator_command(output, "same", 1, ["x", "x"])

    def test_official_score_requires_exact_cohort_and_matching_resolved_evidence(self):
        cohort={"x","y","z"}
        report={
            "schema_version":2,"submitted_ids":["x","y","z"],
            "resolved_ids":["x","z"],"resolved_instances":2,
        }
        result=runner.official_score(report,cohort)
        self.assertEqual(result["officialResolved"],2)
        self.assertEqual(result["officialDenominator"],3)
        self.assertAlmostEqual(result["officialResolvedRate"],2/3,places=6)
        for field,wrong in (
            ("submitted_ids",["x","y"]),
            ("resolved_ids",["x","not-in-cohort"]),
            ("resolved_instances",100),
            ("schema_version",999),
        ):
            invalid={**report,field:wrong}
            with self.subTest(field=field),self.assertRaises(ValueError):
                runner.official_score(invalid,cohort)

    def test_operation_metrics_include_failed_and_uncertain_attempts(self):
        events = [
            {"id":"op1", "status":"completed"},
            {"id":"op2", "status":"failed"},
            {"id":"op3", "status":"interrupted"},
            {"id":"op4", "status":"completed"},
        ]
        counts = runner.tool_operation_metrics(events)
        self.assertEqual(counts["toolCalls"],4)
        self.assertEqual(counts["successfulToolCalls"],2)
        self.assertEqual(counts["failedToolCalls"],1)
        self.assertEqual(counts["uncertainToolCalls"],1)

    def test_no_operations_is_measured_zero_not_fabricated_success(self):
        counts = runner.tool_operation_metrics([])
        self.assertEqual(counts["toolCalls"],0)
        self.assertEqual(counts["failedToolCalls"],0)
        self.assertEqual(counts["successfulToolCalls"],0)

    def test_offline_missing_mirror_never_forges_score_and_writes_manifest(self):
        # Exercise the actual CLI without a real provider or a Docker grader.
        # A synthetic fixture must ALWAYS remain setup_failed/unscored.
        with tempfile.TemporaryDirectory() as folder:
            work = Path(folder)
            (work / "empty-state").mkdir()
            binary = work / "fake-binary"
            binary.write_bytes(b"not an executable and never invoked")
            task = {"instance_id":"synthetic__case-1",
                    "repo":"example/repository",
                    "base_commit":"a" * 40,
                    "problem_statement":"Synthetic fixture. Not a SWE-bench result."}
            rows = work / "fixture.jsonl"
            rows.write_text(json.dumps(task) + "\n", encoding="utf-8")
            command = [
                sys.executable, str(ROOT),
                "--tasks-jsonl", str(rows), "--binary", str(binary),
                "--state-template", str(work / "empty-state"),
                "--provider-id", "offline-test", "--model", "none",
                "--model-name", "synthetic-contract-only",
                "--offline", "--cache", str(work / "empty-cache"),
                "--output", "relative-results"
            ]
            proc = subprocess.run(command, cwd=work,
                                  capture_output=True, text=True, timeout=20)
            self.assertEqual(proc.returncode, 0, proc.stderr)
            output = work / "relative-results"
            manifest = json.loads((output / "run-manifest.json").read_text())
            summary = json.loads((output / "summary.json").read_text())
            traces = [json.loads(line) for line in
                      (output / "traces.jsonl").read_text().splitlines()]
            self.assertEqual(manifest["binarySha256"],
                             hashlib.sha256(binary.read_bytes()).hexdigest())
            self.assertEqual(manifest["instanceIds"], ["synthetic__case-1"])
            self.assertEqual(summary["attemptedInstances"],1)
            self.assertFalse(summary["officialEvaluated"])
            self.assertIsNone(summary["officialResolved"])
            self.assertEqual(summary["nonemptyPatches"],0)
            self.assertEqual(traces[0]["status"],"setup_failed")

    def test_swe_bench_verified_is_pinned_not_user_label(self):
        self.assertEqual(runner.DATASET, "princeton-nlp/SWE-bench_Verified")
        self.assertNotEqual(runner.DATASET, "princeton-nlp/SWE-bench_Lite")


if __name__ == "__main__":
    unittest.main()
