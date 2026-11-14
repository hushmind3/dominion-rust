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

class SdkIntegrationContract(unittest.TestCase):
    def test_real_sdk_entrypoint_calls_registered_file_tool_and_records_proof(self):
        """No fake success: the SDK interface must perform a real file write."""
        import sys, types
        from unittest.mock import patch
        import qwen_agent_runner
        fake=types.ModuleType("qwen_agent")
        agents=types.ModuleType("qwen_agent.agents")
        tools=types.ModuleType("qwen_agent.tools")
        base=types.ModuleType("qwen_agent.tools.base")
        registry={}
        class BaseTool: pass
        def register_tool(name):
            def decorate(cls):
                registry[name]=cls
                return cls
            return decorate
        class Assistant:
            def __init__(self,llm,system_message,function_list):
                self.list=function_list
                if "127.0.0.1" not in llm["model_server"]:
                    raise ValueError("test must stay local")
            def run(self,messages):
                self.assert_message=messages[0]["content"]
                assert "dominion_write" in self.list
                assert "dominion_read" in self.list
                write=registry["dominion_write"]()
                receipt=json.loads(write.call('{"path":"report.txt","content":"actual bytes"}'))
                assert receipt["ok"] and receipt["changed"]
                yield [{"role":"assistant","content":"I saved report.txt"}]
        import json
        base.BaseTool=BaseTool
        base.register_tool=register_tool
        agents.Assistant=Assistant
        with tempfile.TemporaryDirectory() as root, patch.dict(sys.modules,{
            "qwen_agent":fake,"qwen_agent.agents":agents,
            "qwen_agent.tools":tools,"qwen_agent.tools.base":base,
        }):
            outcome=qwen_agent_runner.sdk_run({
                "workspace":root,"model":"qwen3:4b",
                "modelServer":"http://127.0.0.1:11434/v1",
                "prompt":"Create report.txt","maxEdits":3,
            })
            self.assertEqual(outcome["status"],"completed")
            self.assertEqual(len(outcome["fileReceipts"]),1)
            self.assertEqual(Path(root,"report.txt").read_text(),"actual bytes")
            self.assertEqual(outcome["fileReceipts"][0]["tool"],"workspace_write")

if __name__=="__main__":unittest.main()
