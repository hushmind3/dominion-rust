"""Offline contract tests; no test here claims a SWE-bench resolved task."""
import hashlib
import importlib.util
from pathlib import Path
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
            "source": "isolated_git_binary_diff_after_terminal_goal_v1",
            "complete": True, "baseCommit": self.base, "untrackedFiles": 0,
            "modelPatch": self.patch, "bytes": len(self.patch.encode("utf-8")),
            "sha256": hashlib.sha256(self.patch.encode("utf-8")).hexdigest(),
        }

    def test_only_exact_worktree_source_passes(self):
        self.assertEqual(runner.validated_patch(self.value, self.base), self.patch)

    def test_wrong_commit_is_never_graded(self):
        with self.assertRaises(ValueError):
            runner.validated_patch(self.value, "b" * 40)

    def test_truncated_or_tampered_patch_is_never_graded(self):
        for key, replacement in (
            ("modelPatch", self.patch[:-1]), ("bytes", 0),
            ("sha256", "0" * 64), ("source", "ui_clipped_diff_v1"),
            ("complete", False), ("untrackedFiles", 1),
        ):
            invalid = dict(self.value)
            invalid[key] = replacement
            with self.subTest(key=key), self.assertRaises(ValueError):
                runner.validated_patch(invalid, self.base)

    def test_invalid_commit_blocks_checkout_before_git_network(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            with self.assertRaises(ValueError):
                runner.checkout({"repo": "owner/name", "base_commit": "invalid"},
                                root / "cache", root / "task", True)
            self.assertFalse((root / "task").exists())

    def test_swe_bench_verified_is_pinned_not_user_label(self):
        self.assertEqual(runner.DATASET, "princeton-nlp/SWE-bench_Verified")
        self.assertNotEqual(runner.DATASET, "princeton-nlp/SWE-bench_Lite")


if __name__ == "__main__":
    unittest.main()
