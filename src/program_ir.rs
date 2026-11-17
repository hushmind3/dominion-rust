//! Shared typed program bindings and three-valued observation predicates.
//! Adapted from Dominion main engine/program.rs for the Sub4 Rust Run.
//! This is shared program logic, not another executor, scheduler or model.
//! Missing fields stay Unknown; dataflow inputs must never silently become null.
use serde_json::{Map,Value};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Verdict { True,False,Unknown }
impl Verdict {
    pub fn is_true(self)->bool{self==Self::True}
}
/// Reuse the main Program IR's observation binding format.
/// The tool admission layer is still applied *after* the binding is resolved.
pub fn resolve_inputs(inputs:&Value,observed:&Value)->Result<Value,String>{
    let values=inputs.as_object().ok_or("program tool arguments must be an object")?;
    let mut output=Map::new();
    for (key,value) in values {
        if let Some(from)=value.get("from").and_then(Value::as_str) {
            let props=value.as_object().ok_or("binding must be object")?;
            if props.keys().any(|name|name!="from"&&name!="path")
                || props.get("path").is_some_and(|v|!v.is_string())
                || from.is_empty(){
                return Err(format!("invalid typed observation binding for {key}"));
            }
            let base=observed.get(from).ok_or_else(||
                format!("input {key} references unavailable observation {from}"))?;
            // A failed, rejected or skipped tool cannot become a trusted
            // source for another operation's arguments. A recovery branch
            // can still use a literal input and a typed status predicate.
            if base.get("status").and_then(Value::as_str)
                .is_some_and(|status|status!="ok"){
                return Err(format!("input {key} references non-successful observation {from}"));
            }
            let selected=if let Some(pointer)=value.get("path").and_then(Value::as_str){
                base.pointer(pointer).ok_or_else(||
                    format!("input {key} references missing observation pointer {from}{pointer}"))?
            }else{base};
            if selected.is_null(){
                return Err(format!("input {key} resolves to null: {from}"));
            }
            output.insert(key.clone(),selected.clone());
        }else{output.insert(key.clone(),value.clone());}
    }
    Ok(Value::Object(output))
}
fn selected(predicate:&Value,observed:&Value)->Result<Option<Value>,String>{
    let source=predicate["from"].as_str().filter(|s|!s.is_empty())
        .ok_or("predicate.from required")?;
    let Some(value)=observed.get(source)else{return Ok(None)};
    if value.is_null(){return Ok(None);}
    match predicate["path"].as_str(){
        Some(path)=>Ok(value.pointer(path).cloned()),
        None=>Ok(Some(value.clone()))
    }
}
/// Main's 2026 typed Program predicate semantics. Missing evidence is Unknown
/// (neither false nor success); not(Unknown) stays Unknown.
pub fn predicate_verdict(predicate:&Value,observed:&Value)->Result<Verdict,String>{
    if !predicate.is_object(){return Err("predicate must be object".into());}
    match predicate["type"].as_str().unwrap_or(""){
        "exists"=>{
            let source=predicate["from"].as_str().filter(|s|!s.is_empty())
                .ok_or("exists predicate requires from")?;
            let Some(value)=observed.get(source)else{return Ok(Verdict::Unknown)};
            let exists=if let Some(path)=predicate["path"].as_str(){
                value.pointer(path).is_some()
            }else{!value.is_null()};
            Ok(if exists{Verdict::True}else{Verdict::False})
        },
        "equals"=>{
            let Some(actual)=selected(predicate,observed)?else{return Ok(Verdict::Unknown)};
            if predicate.get("value").is_none(){return Err("equals requires value".into());}
            Ok(if actual==predicate["value"]{Verdict::True}else{Verdict::False})
        },
        "compare"=>{
            let Some(actual)=selected(predicate,observed)?else{return Ok(Verdict::Unknown)};
            let (Some(a),Some(b))=(actual.as_f64(),predicate["value"].as_f64())
                else{return Ok(Verdict::Unknown)};
            let yes=match predicate["op"].as_str().unwrap_or(""){
                "gt"=>a>b,"gte"=>a>=b,"lt"=>a<b,"lte"=>a<=b,
                _=>return Err("unsupported compare operator".into())
            };
            Ok(if yes{Verdict::True}else{Verdict::False})
        },
        "not"=>match predicate_verdict(&predicate["predicate"],observed)?{
            Verdict::True=>Ok(Verdict::False),
            Verdict::False=>Ok(Verdict::True),
            Verdict::Unknown=>Ok(Verdict::Unknown)
        },
        "all"|"any"=>{
            let kind=predicate["type"].as_str().unwrap_or("");
            let children=predicate["predicates"].as_array().ok_or(
                "composed predicates must contain predicates array")?;
            if children.is_empty()||children.len()>12{
                return Err("composed predicates must have 1..12 children".into());
            }
            let mut unknown=false;
            for condition in children{
                match predicate_verdict(condition,observed)?{
                    Verdict::True if kind=="any"=>return Ok(Verdict::True),
                    Verdict::False if kind=="all"=>return Ok(Verdict::False),
                    Verdict::Unknown=>unknown=true,
                    _=>{}
                }
            }
            if unknown{Ok(Verdict::Unknown)}
            else if kind=="all"{Ok(Verdict::True)}
            else{Ok(Verdict::False)}
        },
        other=>Err(format!("unknown predicate type: {other}"))
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    #[test]
    fn missing_observation_is_unknown_even_under_negation(){
        let map=json!({});
        assert_eq!(predicate_verdict(&json!({"type":"equals","from":"x","value":5}),&map).unwrap(),Verdict::Unknown);
        assert_eq!(predicate_verdict(&json!({"type":"not","predicate":{"type":"equals","from":"x","value":5}}),&map).unwrap(),Verdict::Unknown);
    }
    #[test]
    fn tainted_error_or_skipped_observations_cannot_supply_write_arguments(){
        for status in ["error","skipped"] {
            let evidence=json!({"probe":{"status":status,"value":"unsafe/path.rs"}});
            let input=json!({"path":{"from":"probe","path":"/value"}});
            assert!(resolve_inputs(&input,&evidence).is_err(),
                "{status} must not be promoted to an executable path");
        }
        let good=json!({"probe":{"status":"ok","value":"checked.rs"}});
        assert_eq!(resolve_inputs(&json!({"path":{"from":"probe","path":"/value"}}),
            &good).unwrap(),json!({"path":"checked.rs"}));
    }
    #[test]
    fn nested_observation_pointer_and_comparison(){
        let map=json!({"scan":{"status":"ok","value":{"hits":[{"path":"src/a.rs","count":4}]}}});
        let input=json!({"path":{"from":"scan","path":"/value/hits/0/path"}});
        assert_eq!(resolve_inputs(&input,&map).unwrap(),json!({"path":"src/a.rs"}));
        assert_eq!(predicate_verdict(&json!({"type":"all","predicates":[
            {"type":"equals","from":"scan","path":"/status","value":"ok"},
            {"type":"compare","from":"scan","path":"/value/hits/0/count","op":"gte","value":3}
        ]}),&map).unwrap(),Verdict::True);
        assert!(resolve_inputs(&json!({"path":{"from":"scan","path":"/invalid"}}),&map).is_err());
    }
}
