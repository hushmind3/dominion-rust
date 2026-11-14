#!/usr/bin/env python3
"""Optional Apache-2.0 Qwen-Agent SDK coding worker for Dominion Rust.
pip install 'qwen-agent>=0.0.26,<0.1'
One JSON input line on stdin; one JSON result on stdout. Only local Ollama
endpoints admitted by the Rust host. No shell/network/MCP tool is registered.
The approved workspace is not a substitute for OS process sandboxing.
"""
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile

READ_LIMIT = 80_000
WRITE_LIMIT = 100_000
DENIED = {".git", ".ssh", ".gnupg", ".venv", "node_modules", "target", ".env"}
ACTIVE_WORKSPACE = None

def sha(data):
    return hashlib.sha256(data).hexdigest() if data is not None else None

class Workspace:
    """Bounded file tools, with optimistic SHA and post-write receipts."""
    def __init__(self, root, max_edits):
        self.root = Path(root).resolve(strict=True)
        if not self.root.is_dir() or not 1 <= max_edits <= 40:
            raise ValueError("invalid local workspace or edit limit")
        self.max_edits = max_edits
        self.receipts = []

    def path(self, relative, allow_missing=False):
        if not isinstance(relative, str) or not relative or len(relative)>512 or "\x00" in relative or "\\" in relative:
            raise ValueError("invalid relative path")
        part = Path(relative)
        if part.is_absolute() or any(x==".." or x in DENIED or x.startswith(".env") for x in part.parts):
            raise ValueError("path not allowed")
        current = self.root
        for name in part.parts:
            current = current / name
            if current.is_symlink():
                raise ValueError("symlink traversal disabled")
            if current.exists() and not current.resolve().is_relative_to(self.root):
                raise ValueError("workspace escape")
        if not allow_missing and not current.is_file():
            raise FileNotFoundError(relative)
        return current

    def list(self, relative="."):
        directory = self.root if relative=="." else self.path(relative,True)
        if not directory.is_dir():
            raise ValueError("not a directory")
        entries=[]
        for item in sorted(directory.iterdir(),key=lambda p:p.name):
            if item.name in DENIED or item.name.startswith(".env") or item.is_symlink():
                continue
            entries.append({"name":item.name,"directory":item.is_dir()})
            if len(entries)==80: break
        return {"entries":entries}

    def read(self, relative):
        path=self.path(relative)
        if path.stat().st_size>READ_LIMIT:
            raise ValueError("file too large")
        data=path.read_bytes()
        return {"content":data.decode("utf-8"),"sha256":sha(data)}

    def _write(self,relative,content,expected_sha,tool):
        if len(self.receipts)>=self.max_edits:
            raise ValueError("edit allowance exhausted")
        if not isinstance(content,str) or len(content.encode("utf-8"))>WRITE_LIMIT:
            raise ValueError("write exceeds 100KB")
        path=self.path(relative,True)
        if path.exists() and not path.is_file():
            raise ValueError("not a regular file")
        old=path.read_bytes() if path.exists() else None
        before=sha(old)
        if expected_sha is not None and before!=expected_sha:
            raise ValueError("stale edit; read file again")
        new=content.encode("utf-8")
        if old==new:
            return {"changed":False,"sha256":before}
        path.parent.mkdir(parents=True,exist_ok=True)
        self.path(relative,True)
        fd,tmp=tempfile.mkstemp(prefix=".dominion-sdk-",dir=path.parent)
        try:
            with os.fdopen(fd,"wb") as stream:
                stream.write(new)
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(tmp,path)
        finally:
            if os.path.exists(tmp):
                os.unlink(tmp)
        record={"tool":tool,"path":relative,"beforeSha256":before,
                "afterSha256":sha(new),"bytes":len(new),"changed":True}
        self.receipts.append(record)
        return {"changed":True,"sha256":record["afterSha256"]}

    def write(self,path,content,expected_sha=None):
        return self._write(path,content,expected_sha,"workspace_write")

    def replace(self,path,old,new,expected_sha=None):
        if not old:
            raise ValueError("empty target is invalid")
        current=self.read(path)
        if current["content"].count(old)!=1:
            raise ValueError("replacement target must occur exactly once")
        if expected_sha is not None and current["sha256"]!=expected_sha:
            raise ValueError("stale edit")
        return self._write(path,current["content"].replace(old,new,1),
                           current["sha256"],"workspace_replace")

def parse_params(value):
    if isinstance(value,dict):
        return value
    try:
        return json.loads(value)
    except (TypeError,json.JSONDecodeError):
        import json5
        return json5.loads(value)

def sdk_run(spec):
    from qwen_agent.agents import Assistant
    from qwen_agent.tools.base import BaseTool,register_tool
    global ACTIVE_WORKSPACE
    workspace=Workspace(spec["workspace"],spec.get("maxEdits",12))
    ACTIVE_WORKSPACE=workspace

    class Tool(BaseTool):
        def call(self,params,**kwargs):
            try:
                return json.dumps({"ok":True,**self.action(parse_params(params))},ensure_ascii=False)
            except (OSError,KeyError,ValueError,UnicodeError) as error:
                return json.dumps({"ok":False,"error":str(error)[:300]})

    @register_tool("dominion_list")
    class List(Tool):
        description="List files within the approved local workspace."
        parameters=[{"name":"path","type":"string","required":True}]
        def action(self,p): return workspace.list(p["path"])

    @register_tool("dominion_read")
    class Read(Tool):
        description="Read a UTF-8 file, including SHA-256 for safe subsequent edits."
        parameters=[{"name":"path","type":"string","required":True}]
        def action(self,p): return workspace.read(p["path"])

    @register_tool("dominion_write")
    class Write(Tool):
        description="Write a UTF-8 file within the approved local workspace."
        parameters=[
            {"name":"path","type":"string","required":True},
            {"name":"content","type":"string","required":True},
            {"name":"expectedSha256","type":"string","required":False}]
        def action(self,p): return workspace.write(p["path"],p["content"],p.get("expectedSha256"))

    @register_tool("dominion_replace")
    class Replace(Tool):
        description="Replace exactly one old substring with new text using source SHA."
        parameters=[
            {"name":"path","type":"string","required":True},
            {"name":"old","type":"string","required":True},
            {"name":"new","type":"string","required":True},
            {"name":"expectedSha256","type":"string","required":False}]
        def action(self,p):
            return workspace.replace(p["path"],p["old"],p["new"],p.get("expectedSha256"))

    bot=Assistant(
        llm={"model":spec["model"],"model_server":spec["modelServer"],
             "api_key":"EMPTY","generate_cfg":{"max_tokens":1200}},
        system_message=("You are a local coding agent. Read and modify the "
            "workspace with dominion_* tools. Never invent a file edit. "
            "No shell or network tool is available."),
        function_list=["dominion_list","dominion_read","dominion_write","dominion_replace"])
    messages=[{"role":"user","content":spec["prompt"]}]
    last=[]
    for last in bot.run(messages=messages):
        if len(workspace.receipts)>=workspace.max_edits:
            break
    final="\n".join(str(m.get("content","")) for m in last
                    if isinstance(m,dict) and m.get("role")=="assistant")[-12000:]
    return {"sdk":"QwenLM/Qwen-Agent","status":"completed" if workspace.receipts else "needs_review",
            "output":final,"fileReceipts":workspace.receipts,
            "completedWithVerifiedWrites":bool(workspace.receipts),"modelUsage":None,
            "reason":None if workspace.receipts else "No verified file writes from the SDK agent"}

def main():
    try:
        raw=sys.stdin.buffer.readline(READ_LIMIT+1)
        if len(raw)>READ_LIMIT:
            raise ValueError("input too large")
        spec=json.loads(raw)
        if not isinstance(spec,dict) or spec.get("approvedLocalWrite") is not True:
            raise ValueError("owner approval required")
        from contextlib import redirect_stdout
        with redirect_stdout(sys.stderr):
            result=sdk_run(spec)
    except Exception as error:
        result={"sdk":"QwenLM/Qwen-Agent","status":"needs_review",
                "reason":f"{type(error).__name__}: {str(error)[:500]}",
                "fileReceipts":ACTIVE_WORKSPACE.receipts if ACTIVE_WORKSPACE else [],
                "completedWithVerifiedWrites":False,"modelUsage":None}
    print(json.dumps(result,ensure_ascii=False),flush=True)

if __name__=="__main__":
    main()
