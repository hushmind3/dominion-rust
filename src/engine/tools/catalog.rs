//! Native tool schemas supplied to model requests. Actual authority stays in gateway.rs.
use super::*;

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
        tool("workspace_replace", "Edit a small source range without rewriting the whole file (approval required). Either supply a unique oldText, or the startLine/endLine and expectedSha256 from workspace_read_range. For line mode, newText replaces those inclusive lines.", object(json!({"path": text("Relative file path"), "oldText": text("Option A: exact existing source text, occurring once"), "startLine":json!({"type":"integer","minimum":1,"description":"Option B: first line from workspace_read_range"}), "endLine":json!({"type":"integer","minimum":1,"description":"Option B: inclusive last line; max 120 lines"}), "newText": text("Replacement code text (do not include numbered line prefixes)"), "expectedSha256": text("Required in line mode: sourceSha256 from workspace_read_range; optional in oldText mode")}), vec!["path","newText"])),
        tool("workspace_replace_function", "Replace one existing, uniquely named top-level Rust function from a version-checked source hash. Send a complete fn declaration and body; all unrelated source remains byte-for-byte unchanged.", object(json!({
            "path": text("Relative Rust .rs source path"),
            "functionName": text("Unique existing top-level Rust function name"),
            "newText": text("Complete replacement Rust function with body"),
            "expectedSha256": text("Full-file SHA-256 returned by workspace_read_range")
        }), vec!["path","functionName","newText","expectedSha256"])),
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
        tool("rsi_system_plan", "Design executable improvements across one or several intelligence subsystems. For a coupled design set component=system, specify components and componentFiles assigning EACH target file to exactly one role, then really edit EVERY target. successorStages is a program handed to the REBUILT descendant for its next design decisions; a plan alone never earns evidence.", object(json!({
            "component":text("Existing subsystem (algorithms, harness, memory, learning, tools, models, improvement_methods, frontend), new intelligence role, or 'system' for coupled redesign (Rust and Python)"),
            "components":json!({"type":"array","items":{"type":"string"},"minItems":2,"maxItems":16,
                "description":"For component=system, all redesign roles including any new modules"}),
            "componentFiles":json!({"type":"object","description":"For component=system map each role to its own nonempty list of files from targetFiles; files cannot be credited twice",
                "additionalProperties":{"type":"array","items":{"type":"string"},"minItems":1}}),
            "moduleIntroductions":json!({"type":"array","maxItems":8,
                "description":"Invent a NEW intelligence role with new implementation source AND an existing integrating consumer source; both files must change and pass independent evaluation.",
                "items":{"type":"object","properties":{
                    "role":{"type":"string"},"newFiles":{"type":"array",
                        "minItems":1,"maxItems":8,"items":{"type":"string"}},
                    "integrationFiles":{"type":"array",
                        "minItems":1,"maxItems":8,"items":{"type":"string"}},
                    "interfaceContract":{"type":"string"},
                    "integrationSymbol":{"type":"string",
                        "description":"Exact identifier appearing in BOTH new implementation source and existing consumer source after edit"},
                    "improvementMechanism":{"type":"string"}
                },"required":["role","newFiles","integrationFiles",
                    "interfaceContract","integrationSymbol","improvementMechanism"],
                "additionalProperties":false}}),
            "interactionEdges":json!({"type":"array","maxItems":16,
                "description":"Source-bound A->B improvement mechanism: both assigned sources must actually change; acceptance and descendant transfer are independently evaluated",
                "items":{"type":"object","properties":{
                    "fromRole":{"type":"string"},"toRole":{"type":"string"},
                    "fromPath":{"type":"string"},"toPath":{"type":"string"},
                    "mechanism":{"type":"string"},"expectedEffect":{"type":"string"}
                },"required":["fromRole","toRole","fromPath","toPath","mechanism","expectedEffect"],
                "additionalProperties":false}}),
            "nextDomain":json!({"type":"string","enum":crate::engine::rsi_evolve::DOMAINS}),
            "hypothesis":text("Falsifiable causal limitation; at least 20 characters"),
            "design":text("Executable architecture, interface contracts, memory/learning/tool changes and failure handling; at least 30 characters"),
            "targetFiles":json!({"type":"array","items":{"type":"string"},"minItems":1,"maxItems":24}),
            "improvementPrinciple": json!({"type":"object",
                "description":"Model-authored source-bound learning principle: change both one task-intelligence component AND the editable method that discovers its improvements, with independent next-generation predictions",
                "properties": {
                    "name":{"type":"string"},
                    "taskRole":{"type":"string","description":"Any actual source-backed intelligence role discovered or invented by this descendant; not the improvement_methods role"},
                    "taskFile":{"type":"string"},
                    "methodFile":{"type":"string"},
                    "failureMode":{"type":"string"},
                    "mechanism":{"type":"string"},
                    "counterfactual":{"type":"string"},
                    "transferPrediction":{"type":"string"},
                    "learningSignal":{"type":"string"}
                },
                "required":["name","taskRole","taskFile","methodFile",
                    "failureMode","mechanism","counterfactual",
                    "transferPrediction","learningSignal"],
                "additionalProperties":false}),
            "expectedOutcomes":json!({"type":"object",
                "description":"Probability estimates before code modification; independently calibrated after evaluation.",
                "properties":{
                    "candidateCaptured":{"type":"number","minimum":0,"maximum":1},
                    "adoption":{"type":"number","minimum":0,"maximum":1},
                    "metaImprovement":{"type":"number","minimum":0,"maximum":1},
                    "regression":{"type":"number","minimum":0,"maximum":1}
                },"additionalProperties":false}),
            "openProgram":json!({"type":"object",
                "description":"Model-authored open recursive experiment graph. Unlike fixed successorStages, nodes, roles, mechanics, branching, loops and replacement plans are authored by the model; an adopted source change is required before the compiled descendant interprets it.",
                "properties":{
                    "entry":{"type":"string"},
                    "nodes":{"type":"array","minItems":1,"maxItems":96,
                        "items":{"type":"object","properties":{
                            "id":{"type":"string"},"role":{"type":"string"},
                            "domain":{"type":"string","enum":crate::engine::rsi_evolve::DOMAINS},
                            "path":{"type":"string"},"objective":{"type":"string"},
                            "mechanism":{"type":"string"}},
                            "required":["id","role","domain","path","objective","mechanism"],
                            "additionalProperties":false}},
                    "edges":{"type":"array","maxItems":384,
                        "items":{"type":"object","properties":{
                            "from":{"type":"string"},"to":{"type":"string"},
                            "when":{"description":"String outcome or recursive aggregate-metric condition: {all:[...]}, {any:[...]}, {not:...}, or {metric:'improvedHoldouts',op:'>=',value:2}. Model may combine predicates without prewritten experiment stages.",
                                "anyOf":[{"type":"string","enum":["always","adopted",
                                    "not_adopted","improved","regressed",
                                    "rejected","no_candidate","neutral"]},
                                    {"type":"object"}]}},
                            "required":["from","to","when"],
                            "additionalProperties":false}}
                },"required":["entry","nodes","edges"],"additionalProperties":false}),
            "successorStages":json!({"type":"array","maxItems":8,
                "description":"Executable planning intentions for the NEXT compiled descendant. Each stage is rebound to fresh indexed source; no stage receives improvement credit before actual editing.",
                "items":{"type":"object","properties":{
                    "role":{"type":"string"},"domain":{"type":"string","enum":crate::engine::rsi_evolve::DOMAINS},
                    "path":{"type":"string"},"objective":{"type":"string"},
                    "learningPolicy":{"type":"string","enum":["avoid_failed","retry_failed"],
                        "description":"Rank successor stages by fewer failures, or revisit difficult observed failures"},
                    "toolPolicy":{"type":"string","enum":["patch_only","allow_new_modules"],
                        "description":"Restrict inherited descendant tool gateway to existing source patches, or permit new modules"},
                    "executionPolicy":{"type":"string","enum":["grounded_edit","explore_then_edit"],
                        "description":"Bind the inherited source to the native edit checkpoint, or retain exploration"}
                    },
                    "required":["role","domain","path","objective"],"additionalProperties":false}})
        }), vec!["component","nextDomain","hypothesis","design","targetFiles"])),
        tool("rsi_method_propose", "Save a reusable executable procedure learned during RSI research. Existing procedure nodes run through the normal tool runtime in later scheduled work; registration is not improvement evidence.", object(json!({
            "name":text("Reusable method name"),
            "description":text("Principle, source and limits of the method"),
            "domain":json!({"type":"string","enum":crate::engine::rsi_evolve::DOMAINS}),
            "verification":json!({"type":"object","description":"Existing procedure format: type=procedure, appliesTo.requestContains, nodes [{id,tool,arguments}], edges [{from,to}], after [{kind:file_equals/file_contains/file_exists/tool_called,path,text,tool}]. Arguments can bind a prior node result via $node.field. Include concrete postconditions."})
        }), vec!["name","domain","verification"])),
        tool("delegate_tasks", "Launch up to 4 independent read-only research agents in parallel; only the parent may edit files.", object(json!({
            "tasks":json!({"type":"array","minItems":1,"maxItems":4,"items":{"type":"object",
                "properties":{"agentId":text("Research agent id"),"prompt":text("Independent read-only investigation")},
                "required":["agentId","prompt"],"additionalProperties":false}}),
            "wait":json!({"type":"boolean","description":"Wait for all replies (default false)"}),
            "timeoutSeconds":json!({"type":"integer","minimum":5,"maximum":300})
        }),vec!["tasks"])),
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

