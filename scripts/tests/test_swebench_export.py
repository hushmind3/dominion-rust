"""Real input-boundary tests; these do not claim a benchmark solve."""
import importlib.util
from pathlib import Path
import unittest

SOURCE = Path(__file__).resolve().parents[1] / "export_swebench_verified.py"
spec = importlib.util.spec_from_file_location("verified_export", SOURCE)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class ExportTests(unittest.TestCase):
    def test_gold_and_hidden_tests_never_enter_rust_prompt_input(self):
        row = {"instance_id":"sympy__sympy-20590","repo":"sympy/sympy",
               "base_commit":"a"*40,"problem_statement":"Repair parser",
               "patch":"GOLD_SOLVE_SECRET","test_patch":"HIDDEN_TEST_PATCH",
               "FAIL_TO_PASS":"undisclosed"}
        exported=module.export_records([row],limit=1)
        self.assertEqual(set(exported[0]),set(module.PUBLIC_INPUT_FIELDS))
        self.assertNotIn("GOLD",repr(exported))
        self.assertNotIn("HIDDEN",repr(exported))
        self.assertEqual(exported[0]["problem_statement"],"Repair parser")

    def test_requested_ids_must_match_official_dataset(self):
        data=[{"instance_id":"a","repo":"a/b","base_commit":"f"*40,
               "problem_statement":"Fix"}]
        with self.assertRaisesRegex(ValueError,"missing"):
            module.export_records(data,requested_ids=["a","b"])
        with self.assertRaisesRegex(ValueError,"duplicate"):
            module.export_records(data,requested_ids=["a","a"])

if __name__ == "__main__":
    unittest.main()
