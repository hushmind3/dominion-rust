use crate::db::Store;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// Everything a tool needs in order to run. Tools are pure Rust: no Node,
/// no sidecar process.
pub struct ToolContext<'a> {
    pub workspace: &'a Path,
    pub store: &'a Store,
    pub state: &'a crate::state::AppState,
    pub session_id: &'a str,
    pub agent_id: Option<&'a str>,
    /// Parent turn's model tool permission. Delegation is attenuated, never
    /// widened.
    pub tools_enabled: bool,
    /// Parent turn's overall execution permission.
    pub tools_execution_enabled: bool,
}

/// OpenAI-style tool definitions handed to the model.
pub fn definitions() -> Vec<Value> {
    let object = |properties: Value, required: Vec<&str>| {
        json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        })
    };
    let text = |description: &str| json!({ "type": "string", "description": description });

    vec![
        tool("workspace_list", "List files and folders in the workspace", object(json!({"path": text("Relative folder path")}), vec![])),
        tool("workspace_read", "Read a UTF-8 file from the workspace", object(json!({"path": text("Relative file path")}), vec!["path"])),
        tool("workspace_read_range", "Read only a line range from a workspace file. Use the line numbers from the repository index instead of reading a large file end to end.", object(json!({"path": text("Relative file path"), "startLine": json!({"type":"integer","minimum":1}), "endLine": json!({"type":"integer","minimum":1, "description":"Inclusive end line; at most 400 lines are returned"})}), vec!["path","startLine"])),
        tool("code_search", "Search the persistent repository index for file paths, symbols and line numbers. Use this before reading a large file.", object(json!({"query": text("Function, type, path or keyword fragment"), "limit": json!({"type":"integer","minimum":1,"maximum":50})}), vec!["query"])),
        tool("workspace_replace_range", "Replace 1-120 previously read lines using sourceSha256 from workspace_read_range. Send only newText, not the old block. Abort if the file changed.", object(json!({"path": text("Relative source path"), "startLine":json!({"type":"integer","minimum":1}), "endLine":json!({"type":"integer","minimum":1}), "newText":text("Replacement source lines"), "expectedSha256":text("Required exact sourceSha256 from workspace_read_range")}), vec!["path","startLine","endLine","newText","expectedSha256"])),
        tool("workspace_replace_function", "Replace one EXISTING top-level Rust function from a verified workspace_read_range SHA. Send a complete fn declaration plus body as newText; DOMINION finds exact boundaries and preserves all other functions. Prefer this for an explicitly named function goal.", object(json!({"path":text("Relative source .rs path"),"functionName":text("Existing top-level Rust function identifier"),"newText":text("Complete replacement function beginning with fn / pub fn"),"expectedSha256":text("Exact full-file sourceSha256 observed by workspace_read_range")}), vec!["path","functionName","newText","expectedSha256"])),
        tool("workspace_replace", "Edit source by unique oldText or by a verified startLine/endLine pair. In isolated RSI only, a complete single Rust function with a missing endLine may recover its own exact function target. Edits always require a pinned source SHA and independent write verification.", object(json!({
            "path":text("Relative source file path"),
            "oldText":text("Option A: exact source substring, occurring once"),
            "startLine":json!({"type":"integer","minimum":1,"description":"Option B: observed absolute source line"}),
            "endLine":json!({"type":"integer","minimum":1,"description":"Option B: observed inclusive last line, max 120 lines"}),
            "newText":text("Replacement source code, not numbered lines"),
            "expectedSha256":text("Observed full sourceSha256; isolated RSI may use its pinned code index")
        }), vec!["path","newText"])),
        tool("workspace_write", "Write a UTF-8 file inside the workspace (approval required)", object(json!({"path": text("Relative file path"), "content": text("UTF-8 content")}), vec!["path", "content"])),
        tool("file_list", "List any folder on this computer using an absolute path", object(json!({"path": text("Absolute folder path")}), vec!["path"])),
        tool("file_read", "Read any UTF-8 text file using an absolute path", object(json!({"path": text("Absolute file path")}), vec!["path"])),
        tool("file_write", "Create or overwrite any file using an absolute path (approval required)", object(json!({"path": text("Absolute file path"), "content": text("UTF-8 content")}), vec!["path", "content"])),
        tool("shell_exec", "Run a shell command on this computer and return stdout/stderr (approval required)", object(json!({"command": text("Shell command line"), "cwd": text("Optional absolute working directory")}), vec!["command"])),
        tool("terminal_exec", "Run an executable with an argument array without a shell (approval required)", object(json!({"command": text("Executable"), "args": json!({"type": "array", "items": {"type": "string"}})}), vec!["command"])),
        tool("web_search", "Search the public web for current information", object(json!({"query": text("Search query")}), vec!["query"])),
        tool("http_fetch", "Fetch a URL and return its readable text content", object(json!({"url": text("HTTP or HTTPS URL")}), vec!["url"])),
        tool("memory_store", "Save a grounded memory for this agent. Quote the user's exact words; invented facts are rejected. Shared memories are managed by the user.", object(json!({"text": text("Fact to remember, maximum 1600 characters"), "evidenceQuote": text("Exact quote from the current user message supporting the fact"), "sourceMessageId": text("Optional earlier source message ID"), "type": json!({"type":"string","enum":["fact","preference","decision","procedure","lesson"]}), "key": text("Optional stable fact key; a new value replaces the previous value for this key"), "scope": json!({"type":"string","enum":["agent"]})}), vec!["text", "evidenceQuote"])),
        tool("memory_search", "Find saved knowledge relevant to a topic", object(json!({"query": text("Search query")}), vec!["query"])),
        tool("skill_propose", "Draft a reusable skill for user review", object(json!({"name": text("Skill name"), "instructions": text("Reusable instructions")}), vec!["name", "instructions"])),
        tool("algorithm_propose", "Propose domain selection and optional source-targeting formulas. Trial is independent and does not activate a proposal.", object(json!({
            "name": text("Algorithm name"),
            "expression": text("Domain score over attempts, accepted, regressions, inconclusive, novelty, successRate, usefulGenerations, barrenRate, tokenCost, objectiveMatch, unresolved, exploration"),
            "targetExpression": text("Optional file score over pathEvidence, symbolEvidence, summaryEvidence, bodyEvidence, productiveEvidence, domainEvidence, repeatPenalty, novelty, sourceCost"),
            "description": text("Why this formula should choose better improvement targets"),
            "evidence": json!({"type":"array","items":{"type":"string"},"description":"Observed failures or regressions that justify the formula"})
        }), vec!["expression"])),
        tool("unsafe_audit", "Scan workspace Rust files for unsafe blocks, functions, impls, traits or extern items without a preceding SAFETY justification. Read-only; available only during RSI generations.", object(json!({
            "path": text("Optional relative file or folder; defaults to the workspace root")
        }), vec![])),
        tool("rsi_system_plan", "Invent a causal modification to your own task and meta-agent architecture, including NEW modules and multi-component edits. The proposal becomes an inherited method only after every target is actually edited and independently adopted.", object(json!({
            "component":text("Primary editable agent subsystem, or system for multi-component self-redesign"),
            "components":json!({"type":"array","items":{"type":"string"},"minItems":1,"maxItems":12,"description":"Optional independently evolvable subcomponents; required when component=system"}),
            "componentFiles":json!({"type":"object","description":"Required for coupled redesign: map each named subsystem to its targetFiles"}),
            "hypothesis":text("Observed limitation, root cause and falsifiable prediction"),
            "design":text("Concrete executable new architecture or learning/control algorithm to implement"),
            "targetFiles":json!({"type":"array","items":{"type":"string"},"minItems":1,"maxItems":12}),
            "nextDomain":json!({"type":"string","enum":crate::engine::rsi_evolve::DOMAINS}),
            "newComponents":json!({"type":"array","maxItems":8,
                "description":"Invented intelligence roles that must be source-written AND wired into the running compiled agent before adoption",
                "items":{"type":"object","properties":{
                    "role":{"type":"string"},
                    "domain":{"type":"string","enum":crate::engine::rsi_evolve::DOMAINS},
                    "purpose":{"type":"string"},
                    "improvesRole":{"type":"string","description":"Previously existing intelligence role whose future self-improvement ability this new executable component develops"},
                    "sourcePaths":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":4},
                    "integrationPaths":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":4}
                },"required":["role","domain","purpose","sourcePaths","integrationPaths"]}}),
            "interactionEdges":json!({"type":"array","maxItems":12,
                "description":"New mutual-improvement mechanisms that connect actual changed components with observable consequences",
                "items":{"type":"object","properties":{
                    "fromRole":{"type":"string"},"toRole":{"type":"string"},
                    "fromPath":{"type":"string"},"toPath":{"type":"string"},
                    "mechanism":{"type":"string"},"observable":{"type":"string"}
                },"required":["fromRole","toRole","fromPath","toPath","mechanism","observable"]}}),
            "improvementPrinciples":json!({"type":"array","maxItems":12,
                "description":"Model-created and source-implemented, freely composable GENERATIVE principles. Their value is learned from next-generation verified intelligence improvements, never from a claim of adoption.",
                "items":{"type":"object","properties":{
                    "name":{"type":"string"},"methodRole":{"type":"string"},
                    "methodSourcePaths":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":8},
                    "improvesRoles":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":12},
                    "mechanism":{"type":"string"},
                    "falsifiableOutcome":{"type":"string"},
                    "composedFrom":{"type":"array","items":{"type":"string"},"maxItems":6,
                        "description":"Optional other principles whose techniques are recombined into this NEW source-changing improvement mechanism"}
                },"required":["name","methodRole","methodSourcePaths",
                    "improvesRoles","mechanism","falsifiableOutcome"]}}),
            "nextTargetFiles":json!({"type":"array","items":{"type":"string"},"maxItems":3,"description":"Next child's source targets after this design is adopted"}),
            "methodHypothesis":text("Optional new recursive design/editing method and its causal rationale"),
            "actionProgram":json!({"type":"array","items":{"type":"string"},"minItems":2,"maxItems":8,"description":"Optional grounding/editing tool sequence. Independent adoption is required before inheritance."}),
            "successorStages":json!({"type":"array","maxItems":10,"description":"Optional self-authored MULTI-STAGE executable meta-improvement policy after adoption; EACH stage has role, domain, path, objective, learningPolicy, executionPolicy, priority, maxRetries","items":{"type":"object","properties":{
                "role":{"type":"string"},
                "domain":{"type":"string","enum":crate::engine::rsi_evolve::DOMAINS},
                "path":{"type":"string"},
                "objective":{"type":"string"},
                "learningPolicy":{"type":"string","enum":["avoid_failed","repair_failed","explore_novel","repeat_verified"]},
                "executionPolicy":{"type":"string","enum":["grounded_edit","meta_rewrite_first","cross_component_revision","repair_then_edit"]},
                "priority":{"type":"integer","minimum":-20,"maximum":20},
                "maxRetries":{"type":"integer","minimum":0,"maximum":8}
            },"required":["role","domain","path","objective"]}})
        }),vec!["component","hypothesis","design","targetFiles","nextDomain"])),
        tool("rsi_method_propose", "Save a reusable executable procedure learned during RSI research. Existing procedure nodes run through the normal tool runtime in later scheduled work; registration is not improvement evidence.", object(json!({
            "name":text("Reusable method name"),
            "description":text("Principle, source and limits of the method"),
            "domain":json!({"type":"string","enum":crate::engine::rsi_evolve::DOMAINS}),
            "verification":json!({"type":"object","description":"Existing procedure format: type=procedure, appliesTo.requestContains, nodes [{id,tool,arguments}], edges [{from,to}], after [{kind:file_equals/file_contains/file_exists/tool_called,path,text,tool}]. Arguments can bind a prior node result via $node.field. Include concrete postconditions."})
        }), vec!["name","domain","verification"])),
        tool("delegate_task", "Delegate a self-contained task to another agent and optionally wait for its result", object(json!({
            "agentId": text("Target agent id"),
            "prompt": text("Task instructions"),
            "wait": json!({"type":"boolean","description":"Wait for completion (default true)"}),
            "timeoutSeconds": json!({"type":"number","description":"5-300 seconds"})
        }), vec!["agentId", "prompt"])),
        tool("task_status", "Check a delegated task's status", object(json!({"jobId": text("Job id")}), vec!["jobId"])),
        tool("task_result", "Read a delegated task's result, artifacts and final answer", object(json!({"jobId": text("Job id")}), vec!["jobId"])),
        tool("account_status", "List OAuth accounts connected to DOMINION", object(json!({}), vec![])),
        tool("browser_control", "Control a local Chrome/Chromium window: open pages, observe text and interactive elements, click, type, press keys and capture screenshots (approval required)", object(json!({
            "action": json!({"type": "string", "enum": ["status", "open", "observe", "click", "type", "press", "screenshot", "close"]}),
            "url": text("URL to open"),
            "index": json!({"type": "number", "description": "Element index returned by observe"}),
            "text": text("Text to type into the indexed element"),
            "key": text("Key to press: Enter, Escape, Tab, ArrowUp, ArrowDown, ArrowLeft, ArrowRight, Backspace")
        }), vec!["action"])),
        tool("github_api", "Call the GitHub REST API as the connected account. Write methods require approval (approval required)", object(json!({"method": json!({"type": "string", "enum": ["GET", "POST", "PATCH", "PUT", "DELETE"]}), "path": text("GitHub API path beginning with /"), "body": json!({"type": "object"})}), vec!["method", "path"])),
    ]
}

fn tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({
        "type": "function",
        "function": { "name": name, "description": description, "parameters": parameters }
    })
}

pub fn requires_approval(name: &str) -> bool {
    matches!(
        name,
        "workspace_write"
            | "workspace_replace"
            | "workspace_replace_range"
            | "workspace_replace_function"
            | "file_write"
            | "shell_exec"
            | "terminal_exec"
            | "github_api"
            | "browser_control"
    )
}

pub fn rsi_allowed(name:&str)->bool{
    matches!(name,"code_search"|"workspace_read_range"|"workspace_replace"|"workspace_replace_range"|"workspace_write"|"workspace_replace_function"|"web_search"|"http_fetch"|"algorithm_propose"|"unsafe_audit"|"rsi_method_propose"|"rsi_system_plan")
}


#[path = "tools/dispatch.rs"]
mod dispatch;
pub use dispatch::execute;

#[path = "tools/gateway.rs"]
mod gateway;
pub(crate) use gateway::*;

#[path = "tools/workspace.rs"]
mod workspace;
pub(crate) use workspace::*;
pub use workspace::WORKSPACE_ESCAPE;
pub(super) use workspace::resolve_workspace;

#[path = "tools/edit.rs"]
mod edit;
pub(crate) use edit::*;

#[path = "tools/process.rs"]
mod process;
pub(crate) use process::*;
pub(crate) use process::run_command;

#[path = "tools/network.rs"]
mod network;
pub(crate) use network::*;

#[cfg(test)]
#[path = "tools/tests.rs"]
mod tests;
