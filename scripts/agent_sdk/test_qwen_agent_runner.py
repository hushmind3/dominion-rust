"""No Qwen download or API call required: isolated SDK file-tool regressions."""
import sys
from pathlib import Path
import tempfile
import unittest

sys.path.insert(0,str(Path(__file__).parent))
from qwen_agent_runner import Workspace

class WorkspaceTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory()
        self.root=Path(self.temp.name)
        self.tool=Workspace(str(self.root),3)
    def tearDown(self): self.temp.cleanup()
    def test_atomic_sha_edits(self):
        a=self.tool.write("source/x.txt","alpha")
        self.assertTrue(a["changed"])
        self.assertEqual(self.tool.read("source/x.txt")["sha256"],a["sha256"])
        with self.assertRaises(ValueError):
            self.tool.replace("source/x.txt","missing","z")
        with self.assertRaises(ValueError):
            self.tool.write("source/x.txt","beta","stale")
        b=self.tool.replace("source/x.txt","alpha","beta",a["sha256"])
        self.assertNotEqual(b["sha256"],a["sha256"])
        self.assertEqual(len(self.tool.receipts),2)
    def test_outside_and_sensitive_blocked(self):
        (self.root/"link").symlink_to(self.root.parent)
        for name in ["/etc/passwd","../outside","link/x",".git/config",".env","code/../../outside"]:
            with self.assertRaises((ValueError,FileNotFoundError)):
                self.tool.write(name,"evil")
    def test_edit_budget_and_noop(self):
        for i in range(3):self.tool.write(f"{i}.txt","ok")
        with self.assertRaises(ValueError):self.tool.write("four.txt","no")
        self.assertEqual(len(self.tool.receipts),3)
    def test_unicode_and_size_limit(self):
        self.tool.write("한글.txt","작업")
        self.assertEqual(self.tool.read("한글.txt")["content"],"작업")
        with self.assertRaises(ValueError):self.tool.write("big.txt","x"*100001)

if __name__=="__main__":unittest.main()
