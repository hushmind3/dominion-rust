//! First-party Rust tool registry for embedded DOMINION SDK hosts.
//! A native Rust tool executes in the *existing* agent dispatch, approval,
//! audit, effect and verification path, never in a separate agent runtime.
//!
//! Inspired by the architectural separation used in Codex's typed tool
//! registry, AgentScope middleware, and OpenManus' small tool collection.
//! No external SDK or policy engine is linked or copied.
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{Arc, RwLock},
};

pub(crate) type ToolFuture = Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'static>>;
pub(crate) type Callback = Arc<dyn Fn(Value) -> ToolFuture + Send + Sync>;
pub(crate) type VerifyFuture = Pin<Box<dyn Future<Output = Result<bool, String>> + Send + 'static>>;
/// A separate owner-registered Rust postcondition, run after the callback.
/// The LLM and ordinary tool result cannot manufacture its return value.
pub(crate) type Verifier = Arc<dyn Fn(Value, Value) -> VerifyFuture + Send + Sync>;

// In-process user tools can run arbitrary Rust. Their futures need bounded
// lifetimes and output sizes to protect unrelated jobs and the model context.
#[derive(Debug,Clone,Copy)]
pub struct ToolLimits {
    pub execution_timeout: std::time::Duration,
    pub verification_timeout: std::time::Duration,
    pub max_result_bytes: usize,
}

impl Default for ToolLimits {
    fn default() -> Self {
        Self {
            execution_timeout: std::time::Duration::from_secs(120),
            verification_timeout: std::time::Duration::from_secs(30),
            max_result_bytes: 1024 * 1024,
        }
    }
}

impl ToolLimits {
    pub fn validate(self) -> Result<Self,String> {
        let max_time=std::time::Duration::from_secs(3600);
        if self.execution_timeout.is_zero() || self.execution_timeout>max_time
            || self.verification_timeout.is_zero() || self.verification_timeout>max_time
            || self.max_result_bytes==0 || self.max_result_bytes>8*1024*1024
        {
            return Err("native SDK limits must have 1..3600s timeouts and 1..8MiB result budget".into());
        }
        Ok(self)
    }
}


#[derive(Clone)]
struct RegisteredTool {
    description: String,
    schema: Value,
    needs_approval: bool,
    callback: Callback,
    verifier: Option<Verifier>,
    limits: ToolLimits,
}

/// Cloning a registry shares its entries with the SDK/HTTP/agent state.
/// A different embedded SDK has a different registry.
#[derive(Clone, Default)]
pub struct Registry {
    entries: Arc<RwLock<BTreeMap<String, RegisteredTool>>>,
}

fn validate_schema(schema: &Value, value: &Value, depth: usize) -> Result<(), String> {
    if depth > 8 {
        return Err("tool argument schema nesting exceeds 8 levels".into());
    }
    let expected = schema["type"].as_str().unwrap_or("object");
    let valid = match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => return Err(format!("unsupported tool argument type: {expected}")),
    };
    if !valid {
        return Err(format!("tool argument must have type {expected}"));
    }
    if let Some(choices) = schema["enum"].as_array() {
        if !choices.contains(value) {
            return Err("tool argument does not match the declared enum".into());
        }
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = schema["required"].as_array() {
            for key in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(key) {
                    return Err(format!("missing required tool argument: {key}"));
                }
            }
        }
        if let Some(properties) = schema["properties"].as_object() {
            for (key, item) in object {
                if let Some(property) = properties.get(key) {
                    validate_schema(property, item, depth + 1)
                        .map_err(|error| format!("{key}: {error}"))?;
                } else if schema["additionalProperties"] == false {
                    return Err(format!("unexpected tool argument: {key}"));
                }
            }
        } else if schema["additionalProperties"] == false && !object.is_empty() {
            return Err("unexpected tool arguments".into());
        }
    }
    if let (Some(items), Some(values)) = (schema.get("items"), value.as_array()) {
        for (index, item) in values.iter().enumerate() {
            validate_schema(items, item, depth + 1)
                .map_err(|error| format!("item[{index}]: {error}"))?;
        }
    }
    Ok(())
}

impl Registry {
    /// Owner-hosted tool namespace is never confused with built-ins or MCP.
    /// Approval defaults are chosen by the registering caller; SDK does not
    /// classify or suppress the user's natural language instruction.
    pub(crate) fn register(
        &self,
        name: &str,
        description: &str,
        schema: Value,
        needs_approval: bool,
        callback: Callback,
    ) -> Result<String, String> {
        self.register_inner(name, description, schema, needs_approval, callback, None, ToolLimits::default())
    }

    pub(crate) fn register_verified(
        &self,
        name: &str,
        description: &str,
        schema: Value,
        needs_approval: bool,
        callback: Callback,
        verifier: Verifier,
    ) -> Result<String, String> {
        self.register_inner(name, description, schema, needs_approval, callback, Some(verifier), ToolLimits::default())
    }

    /// Configure execution budgets separately per tool, not as one global
    /// process limit. The Rust harness still owns approvals and evidence.
    pub(crate) fn register_with_limits(
        &self, name:&str, description:&str, schema:Value,
        needs_approval:bool, callback:Callback, verifier:Option<Verifier>,
        limits:ToolLimits,
    )->Result<String,String>{
        self.register_inner(name,description,schema,needs_approval,callback,verifier,limits)
    }

    fn register_inner(
        &self,
        name: &str,
        description: &str,
        schema: Value,
        needs_approval: bool,
        callback: Callback,
        verifier: Option<Verifier>,
        limits: ToolLimits,
    ) -> Result<String, String> {
        let limits=limits.validate()?;
        if name.is_empty() || name.len() > 48
            || !name.starts_with(|ch: char| ch.is_ascii_lowercase())
            || !name.bytes().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == b'_')
        {
            return Err("tool name must be 1..48 lowercase ASCII letters, digits, underscores, beginning with a letter".into());
        }
        if description.trim().is_empty() || description.len() > 2048 {
            return Err("tool description must be 1..2048 bytes".into());
        }
        if !schema.is_object() || schema["type"] != "object"
            || serde_json::to_vec(&schema).map_err(|error| error.to_string())?.len() > 8192 {
            return Err("tool schema must be a JSON object schema of at most 8192 bytes".into());
        }
        // Reject schema structures the registry cannot faithfully enforce.
        // No promise of full JSON Schema compliance.
        let required = schema["required"].as_array();
        if let Some(keys) = required {
            if keys.iter().any(|key| key.as_str().is_none_or(|s| s.is_empty())) {
                return Err("required field names must be strings".into());
            }
        }
        let qualified = format!("sdk__{name}");
        let mut entries = self.entries.write().map_err(|_| "tool registry poisoned")?;
        if entries.contains_key(&qualified) {
            return Err(format!("tool already registered: {qualified}"));
        }
        entries.insert(qualified.clone(), RegisteredTool {
            description: description.to_string(),
            schema,
            needs_approval,
            callback,
            verifier,
            limits,
        });
        Ok(qualified)
    }

    pub(crate) fn remove(&self, qualified: &str) -> bool {
        if !qualified.starts_with("sdk__") {
            return false;
        }
        self.entries.write().map(|mut entries| entries.remove(qualified).is_some()).unwrap_or(false)
    }

    pub(crate) fn definitions(&self) -> Vec<Value> {
        self.entries.read().map(|entries| {
            entries.iter().map(|(name, entry)| json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": entry.description,
                    "parameters": entry.schema
                }
            })).collect()
        }).unwrap_or_default()
    }

    pub(crate) fn requires_approval(&self, qualified: &str) -> bool {
        // No unregistered SDK tool can execute with a default no-approval path.
        self.entries.read().ok().and_then(|entries| {
            entries.get(qualified).map(|tool| tool.needs_approval)
        }).unwrap_or(true)
    }

    /// Lookup/validation happen before executing user code. The lock is
    /// released before any await, so callbacks may register other tools.
    pub(crate) async fn execute(&self, qualified: &str, args: &Value) -> Result<Value, String> {
        let entry = self.entries.read().map_err(|_| "tool registry poisoned")?
            .get(qualified).cloned()
            .ok_or_else(|| format!("unknown native SDK tool: {qualified}"))?;
        validate_schema(&entry.schema, args, 0)?;

        // A user callback may have already changed files, charged an API, or
        // spawned a subprocess before returning an error or timing out.
        // Such results are UNKNOWN, never a clean failure eligible for blind
        // automatic replay of the same non-idempotent operation.
        let output = match tokio::time::timeout(
            entry.limits.execution_timeout, (entry.callback)(args.clone()),
        ).await {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => return Ok(json!({
                "error":error,"outcomeUnknown":true,"requiresInspection":true,
                "stage":"native_sdk_callback",
            })),
            Err(_) => return Ok(json!({
                "error":"native Rust tool timed out",
                "outcomeUnknown":true,"requiresInspection":true,
                "stage":"native_sdk_callback_timeout",
            })),
        };
        // Tool authors may put any data in a normal JSON payload, but the
        // verification stamp is strictly reserved for the Rust harness.
        if output.get("_dominion_sdk_receipt").is_some() {
            return Ok(json!({
                "error":"tool result uses a reserved verification field",
                "outcomeUnknown":true,"requiresInspection":true,
                "stage":"native_sdk_receipt_validation",
            }));
        }
        let serialized=serde_json::to_vec(&output)
            .map_err(|error|format!("failed to encode native SDK tool output: {error}"))?;
        if serialized.len()>entry.limits.max_result_bytes{
            return Ok(json!({
                "error":"native Rust tool output exceeded the declared byte budget",
                "bytes":serialized.len(),"maxBytes":entry.limits.max_result_bytes,
                "outcomeUnknown":true,"requiresInspection":true,
                "stage":"native_sdk_result_budget",
            }));
        }
        let Some(verifier) = entry.verifier else { return Ok(output); };
        match tokio::time::timeout(
            entry.limits.verification_timeout,
            (verifier)(args.clone(), output.clone()),
        ).await {
            Ok(Ok(true)) => {},
            Ok(Ok(false)) => return Ok(json!({
                "error":"native Rust tool postcondition did not pass",
                "outcomeUnknown":true,"requiresInspection":true,
                "stage":"native_sdk_postcondition",
            })),
            Ok(Err(error)) => return Ok(json!({
                "error":error,"outcomeUnknown":true,"requiresInspection":true,
                "stage":"native_sdk_postcondition",
            })),
            Err(_) => return Ok(json!({
                "error":"native Rust tool postcondition timed out",
                "outcomeUnknown":true,"requiresInspection":true,
                "stage":"native_sdk_postcondition_timeout",
            })),
        }
        let mut result = if output.is_object() {
            output
        } else {
            json!({"value": output})
        };
        result["_dominion_sdk_receipt"] = json!({
            "verified": true,
            "verifier": "owner_registered_rust_postcondition",
        });
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn echo() -> Callback {
        Arc::new(|arg| Box::pin(async move { Ok(arg) }))
    }

    #[tokio::test]
    async fn native_tools_enforce_name_schema_and_do_not_execute_invalid_payloads() {
        let registry = Registry::default();
        let name = registry.register("echo", "Echo exact user arguments",
            json!({"type":"object","properties":{"message":{"type":"string"}},"required":["message"],"additionalProperties":false}),
            false, echo()).unwrap();
        assert_eq!(name, "sdk__echo");
        assert!(!registry.requires_approval(&name));
        assert!(registry.register("echo", "Duplicate",json!({"type":"object"}),true,echo()).is_err());
        assert!(registry.register("mcp.bad", "Bad",json!({"type":"object"}),true,echo()).is_err());
        assert!(registry.execute(&name, &json!({"unknown":"x"})).await.is_err());
        assert!(registry.execute(&name, &json!({"message":10})).await.is_err());
        assert!(registry.execute(&name, &json!({})).await.is_err());
        let result = registry.execute(&name, &json!({"message":"사용자 원문"})).await.unwrap();
        assert_eq!(result["message"], "사용자 원문");
        assert_eq!(registry.definitions()[0]["function"]["name"], name);
        assert!(registry.remove(&name));
        assert!(registry.execute(&name,&json!({"message":"x"})).await.is_err());
    }

    #[tokio::test]
    async fn native_tool_verifier_is_separate_from_result_and_cannot_be_forged() {
        let registry = Registry::default();
        let forged: Callback = Arc::new(|_| Box::pin(async {
            Ok(json!({"_dominion_sdk_receipt":{"verified":true}}))
        }));
        registry.register("forged","Forged result",json!({"type":"object"}),true,forged).unwrap();
        let forged_result=registry.execute("sdk__forged",&json!({})).await.unwrap();
        assert_eq!(forged_result["outcomeUnknown"],true);
        assert_eq!(forged_result["stage"],"native_sdk_receipt_validation");

        let writer: Callback = Arc::new(|arg| Box::pin(async move {
            Ok(json!({"written":arg["message"]}))
        }));
        let verifier: Verifier = Arc::new(|args, output| Box::pin(async move {
            Ok(args["message"] == output["written"])
        }));
        registry.register_verified("writer","Verified native host result",
            json!({"type":"object","properties":{"message":{"type":"string"}},"required":["message"]}),
            true,writer,verifier).unwrap();
        let success=registry.execute("sdk__writer",&json!({"message":"owner"})).await.unwrap();
        assert_eq!(success["written"],"owner");
        assert_eq!(success["_dominion_sdk_receipt"]["verified"],true);
        assert_eq!(success["_dominion_sdk_receipt"]["verifier"],
            "owner_registered_rust_postcondition");

        let failed: Verifier = Arc::new(|_, _| Box::pin(async { Ok(false) }));
        registry.register_verified("failure","Cannot claim verified success",
            json!({"type":"object"}),true,
            Arc::new(|v|Box::pin(async move{Ok(v)})),failed).unwrap();
        let failed=registry.execute("sdk__failure",&json!({})).await.unwrap();
        assert_eq!(failed["outcomeUnknown"],true);
        assert_eq!(failed["stage"],"native_sdk_postcondition");
    }

    #[tokio::test]
    async fn timed_out_or_failed_host_callback_cannot_earn_a_success_receipt(){
        let registry=Registry::default();
        let slow:Callback=Arc::new(|_|Box::pin(async {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            Ok(json!({"done":true}))
        }));
        registry.register_with_limits("slow","Potentially mutating native tool",
            json!({"type":"object"}),true,slow,None,
            ToolLimits{execution_timeout:std::time::Duration::from_millis(10),
                verification_timeout:std::time::Duration::from_secs(1),
                max_result_bytes:1024}).unwrap();
        let unknown=registry.execute("sdk__slow",&json!({})).await.unwrap();
        assert_eq!(unknown["outcomeUnknown"],true);
        assert_eq!(unknown["stage"],"native_sdk_callback_timeout");
        assert!(unknown["_dominion_sdk_receipt"].is_null());

        let failure:Callback=Arc::new(|_|Box::pin(async {
            Err("partial remote write".into())
        }));
        registry.register("error","Native user tool may have side effects",
            json!({"type":"object"}),true,failure).unwrap();
        let uncertain=registry.execute("sdk__error",&json!({})).await.unwrap();
        assert_eq!(uncertain["outcomeUnknown"],true);

        let oversized:Callback=Arc::new(|_|Box::pin(async {
            Ok(json!({"payload":"x".repeat(2048)}))
        }));
        registry.register_with_limits("large","Large output",
            json!({"type":"object"}),true,oversized,None,
            ToolLimits{execution_timeout:std::time::Duration::from_secs(1),
                verification_timeout:std::time::Duration::from_secs(1),
                max_result_bytes:128}).unwrap();
        let too_large=registry.execute("sdk__large",&json!({})).await.unwrap();
        assert_eq!(too_large["stage"],"native_sdk_result_budget");
        assert_eq!(too_large["outcomeUnknown"],true);
    }

    #[tokio::test]
    async fn native_tool_can_mutate_registry_without_deadlock() {
        let registry = Registry::default();
        let copy = registry.clone();
        let callback: Callback = Arc::new(move |arg| {
            let own = copy.clone();
            Box::pin(async move {
                own.register("second", "Second native tool", json!({"type":"object"}),true,
                    Arc::new(|v| Box::pin(async move { Ok(v) })))?;
                Ok(arg)
            })
        });
        registry.register("first","Can register another tool",json!({"type":"object"}),true,callback).unwrap();
        registry.execute("sdk__first", &json!({})).await.unwrap();
        assert!(registry.requires_approval("sdk__second"));
        assert_eq!(registry.definitions().len(),2);
    }
}
