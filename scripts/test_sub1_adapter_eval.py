"""Offline contract tests for the private paired-evaluation dataset."""
from importlib.util import module_from_spec, spec_from_file_location
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

SCRIPT=Path(__file__).with_name("sub1_adapter_eval.py")
spec=spec_from_file_location("sub1_adapter_eval",SCRIPT)
evaluator=module_from_spec(spec)
spec.loader.exec_module(evaluator)

class PrivateAdapterDatasetTests(unittest.TestCase):
    def samples(self):
        return [
            {"split":"evaluation","approvedForIndependentEvaluation":True,
             "prompt":"question %s?" % i,"completion":"answer %s" % i,
             "domain":"memory" if i<4 else "tools"} for i in range(8)
        ]
    def write(self, folder, rows):
        path=Path(folder)/"cases.jsonl"
        path.write_text("".join(json.dumps(row)+"\n" for row in rows),
                        encoding="utf-8")
        return path,hashlib.sha256(path.read_bytes()).hexdigest()
    def test_two_domains_and_case_hash_are_required(self):
        with tempfile.TemporaryDirectory() as folder:
            path,sha=self.write(folder,self.samples())
            self.assertEqual(len(evaluator.cases(path,sha)),8)
            with self.assertRaises(ValueError):evaluator.cases(path,"0"*64)
    def test_training_data_is_not_a_private_eval_case(self):
        with tempfile.TemporaryDirectory() as folder:
            rows=self.samples()
            rows[0]["split"]="train"
            path,sha=self.write(folder,rows)
            with self.assertRaises(ValueError):evaluator.cases(path,sha)
    def test_duplicates_are_rejected(self):
        with tempfile.TemporaryDirectory() as folder:
            rows=self.samples()
            rows[4]=dict(rows[0],domain="tools")
            path,sha=self.write(folder,rows)
            with self.assertRaises(ValueError):evaluator.cases(path,sha)
    def test_content_overlap_is_rejected_even_with_different_file_hashes(self):
        with tempfile.TemporaryDirectory() as folder:
            path,sha=self.write(folder,self.samples())
            private=evaluator.cases(path,sha)
            train=Path(folder)/"train.jsonl"
            rows=[{"split":"train","approvedForTraining":True,
                "prompt":"training prompt","completion":"training completion"},
                {"split":"train","approvedForTraining":True,
                "prompt":"question 3?","completion":"answer 3"}]
            train.write_text("".join(json.dumps(x)+"\n" for x in rows),encoding="utf-8")
            train_sha=hashlib.sha256(train.read_bytes()).hexdigest()
            self.assertNotEqual(train_sha,sha)
            with self.assertRaises(ValueError):
                evaluator.verify_training_separation(train,train_sha,private)
            rows[1]["prompt"]="distinct training prompt"
            train.write_text("".join(json.dumps(x)+"\n" for x in rows),encoding="utf-8")
            train_sha=hashlib.sha256(train.read_bytes()).hexdigest()
            evaluator.verify_training_separation(train,train_sha,private)

    def test_single_domain_is_ineligible(self):
        with tempfile.TemporaryDirectory() as folder:
            rows=self.samples()
            for sample in rows:sample["domain"]="memory"
            path,sha=self.write(folder,rows)
            with self.assertRaises(ValueError):evaluator.cases(path,sha)
if __name__=="__main__":
    unittest.main()
