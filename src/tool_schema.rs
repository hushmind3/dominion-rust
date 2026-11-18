//! Model-facing tool contracts enforced BEFORE any tool starts.
//! The advertised schema is a capability snapshot for the model turn.
//! Unsupported constraints fail closed, not silently ignored.
use crate::domain::ToolCall;
use serde_json::Value;

const MAX_DEPTH:usize=16;
const MAX_ARGUMENT_BYTES:usize=65_536;
const MAX_ERROR_BYTES:usize=650;

#[derive(Debug,Clone,PartialEq,Eq)]
pub enum AdmissionCode { UnknownTool, InvalidArguments, InvalidSchema }
impl AdmissionCode{
    pub fn as_str(&self)->&'static str{
        match self{
            Self::UnknownTool=>"UNKNOWN_TOOL",
            Self::InvalidArguments=>"INVALID_ARGUMENTS",
            Self::InvalidSchema=>"UNSUPPORTED_TOOL_SCHEMA",
        }
    }
}
#[derive(Debug,Clone)]
pub struct AdmissionError { pub code:AdmissionCode,pub reason:String }
impl AdmissionError{
    fn new(code:AdmissionCode,reason:impl Into<String>)->Self{
        let reason=reason.into();
        Self{code,reason:reason.chars().take(MAX_ERROR_BYTES).collect()}
    }
}

pub fn admit(call:&ToolCall,catalog:&[Value])->Result<(),AdmissionError>{
    if call.arguments.to_string().len()>MAX_ARGUMENT_BYTES{
        return Err(AdmissionError::new(AdmissionCode::InvalidArguments,"tool arguments exceed 64 KiB"));
    }
    let mut matches=catalog.iter().filter(|t|t.pointer("/function/name").and_then(Value::as_str)==Some(&call.name));
    let tool=matches.next().ok_or_else(||AdmissionError::new(AdmissionCode::UnknownTool,
        format!("{} was not available in this model turn",call.name)))?;
    if matches.next().is_some(){
        return Err(AdmissionError::new(AdmissionCode::InvalidSchema,
            format!("duplicate advertised tool name {}",call.name)));
    }
    let schema=tool.pointer("/function/parameters").ok_or_else(||AdmissionError::new(
        AdmissionCode::InvalidSchema,"tool is missing a parameters schema"))?;
    check_schema(schema,0,"$").map_err(|reason|AdmissionError::new(AdmissionCode::InvalidSchema,reason))?;
    validate_value(schema,&call.arguments,0,"$").map_err(|reason|AdmissionError::new(AdmissionCode::InvalidArguments,reason))
}

/// Only JSON Schema constructs whose constraints this validator actually
/// enforces may enter a model-advertised capability. Common MCP nullable
/// unions are supported without silently accepting arbitrary schema keywords.
fn schema_types(node:&Value)->Result<Vec<&str>,String>{
    let Some(map)=node.as_object()else{return Err("schema node must be an object".into());};
    for key in map.keys(){
        if !matches!(key.as_str(),"type"|"properties"|"required"|"additionalProperties"|"items"|
            "enum"|"const"|"minimum"|"maximum"|"exclusiveMinimum"|"exclusiveMaximum"|"minLength"|"maxLength"|"minItems"|"maxItems"|
            "minProperties"|"maxProperties"|"anyOf"|"oneOf"|"allOf"|
            "description"|"title"|"default"|"examples"){
            return Err(format!("unsupported schema keyword {key}"));
        }
    }
    let Some(ty)=node.get("type")else{return Ok(Vec::new())};
    let types=match ty {
        Value::String(t)=>vec![t.as_str()],
        Value::Array(items) if !items.is_empty() && items.len()<=8 =>
            items.iter().map(|v|v.as_str()
                .ok_or("schema type union must contain only strings")).collect::<Result<Vec<_>,_>>()?,
        _=>return Err("schema type must be a string or 1..8 unique strings".into()),
    };
    let mut seen=std::collections::HashSet::new();
    for t in &types {
        if !matches!(*t,"object"|"string"|"integer"|"number"|"boolean"|"array"|"null")
            ||!seen.insert(*t) {
            return Err(format!("invalid or duplicate schema type {t}"));
        }
    }
    Ok(types)
}
fn check_schema(node:&Value,depth:usize,path:&str)->Result<(),String>{
    if depth>MAX_DEPTH{return Err(format!("{path}: schema nesting exceeds {MAX_DEPTH}"));}
    let ty=schema_types(node)?;
    if ["properties","required","additionalProperties","minProperties","maxProperties"]
        .iter().any(|key|node.get(*key).is_some()) && !ty.contains(&"object"){
        return Err(format!("{path}: object-only constraints on non-object schema"));
    }
    if ["items","minItems","maxItems"]
        .iter().any(|key|node.get(*key).is_some()) && !ty.contains(&"array"){
        return Err(format!("{path}: array-only constraints on non-array schema"));
    }
    if ["minLength","maxLength"]
        .iter().any(|key|node.get(*key).is_some()) && !ty.contains(&"string"){
        return Err(format!("{path}: string-only constraints on non-string schema"));
    }
    if ["minimum","maximum","exclusiveMinimum","exclusiveMaximum"]
        .iter().any(|key|node.get(*key).is_some())
        && !ty.iter().any(|t|matches!(*t,"number"|"integer")){
        return Err(format!("{path}: number-only constraints on non-number schema"));
    }
    for name in ["minimum","maximum","exclusiveMinimum","exclusiveMaximum"]{
        if node.get(name).is_some_and(|x|!x.is_number()){return Err(format!("{path}: {name} must be numeric"));}
    }
    for name in ["minLength","maxLength","minItems","maxItems","minProperties","maxProperties"]{
        if node.get(name).is_some_and(|x|x.as_u64().is_none()){return Err(format!("{path}: {name} must be a nonnegative integer"));}
    }
    for (min,max) in [("minLength","maxLength"),("minItems","maxItems"),("minProperties","maxProperties")]{
        if let (Some(a),Some(b))=(node.get(min).and_then(Value::as_u64),node.get(max).and_then(Value::as_u64)){
            if a>b{return Err(format!("{path}: {min} cannot exceed {max}"));}
        }
    }
    let lower=["minimum","exclusiveMinimum"].iter().filter_map(|key|
        node.get(*key).and_then(Value::as_f64).map(|n|(n,*key=="exclusiveMinimum"))
    ).max_by(|a,b|a.0.total_cmp(&b.0).then_with(||a.1.cmp(&b.1)));
    let upper=["maximum","exclusiveMaximum"].iter().filter_map(|key|
        node.get(*key).and_then(Value::as_f64).map(|n|(n,*key=="exclusiveMaximum"))
    ).min_by(|a,b|a.0.total_cmp(&b.0).then_with(||b.1.cmp(&a.1)));
    if let (Some(a),Some(b))=(lower,upper){
        if a.0>b.0 || (a.0==b.0 && (a.1||b.1)){
            return Err(format!("{path}: impossible numeric bounds"));
        }
    }
    if let Some(required)=node.get("required"){
        let entries=required.as_array().ok_or_else(||format!("{path}: required must be an array"))?;
        if entries.iter().any(|entry|entry.as_str().is_none()){return Err(format!("{path}: required values must be strings"));}
        let props=node.get("properties").and_then(Value::as_object);
        for entry in entries {
            if !props.is_some_and(|p|p.contains_key(entry.as_str().unwrap())){
                return Err(format!("{path}: required item not declared in properties"));
            }
        }
    }
    if let Some(extra)=node.get("additionalProperties"){
        if !extra.is_boolean(){check_schema(extra,depth+1,&format!("{path}.*"))?;}
    }
    for choice in ["anyOf","oneOf","allOf"] {
        if let Some(branches)=node.get(choice){
            let variants=branches.as_array().ok_or_else(||format!("{path}: {choice} must be an array"))?;
            if variants.is_empty() || variants.len()>8 {
                return Err(format!("{path}: {choice} must contain 1..8 schemas"));
            }
            for (index,variant) in variants.iter().enumerate(){
                check_schema(variant,depth+1,&format!("{path}.{choice}[{index}]"))?;
            }
        }
    }
    if let Some(properties)=node.get("properties"){
        let properties=properties.as_object().ok_or_else(||format!("{path}: properties must be an object"))?;
        if properties.len()>256{return Err(format!("{path}: too many tool properties"));}
        for (name,child) in properties{
            check_schema(child,depth+1,&format!("{path}.{name}"))?;
        }
    }
    if let Some(items)=node.get("items"){
        check_schema(items,depth+1,&format!("{path}[]"))?;
    }
    if let Some(enumeration)=node.get("enum"){
        let values=enumeration.as_array().ok_or_else(||format!("{path}: enum must be an array"))?;
        if values.is_empty()||values.len()>1024{return Err(format!("{path}: unsupported enum length"));}
    }
    Ok(())
}
fn validate_value(schema:&Value,value:&Value,depth:usize,path:&str)->Result<(),String>{
    if depth>MAX_DEPTH{return Err(format!("{path}: input too deep"));}
    if let Some(ty)=schema.get("type"){
        let types:Vec<&str>=match ty {
            Value::String(t)=>vec![t],
            Value::Array(items)=>items.iter().filter_map(Value::as_str).collect(),
            _=>return Err(format!("{path}: malformed type")),
        };
        if !types.iter().any(|t|match *t{
            "object"=>value.is_object(),"array"=>value.is_array(),"string"=>value.is_string(),
            "integer"=>value.as_i64().is_some()||value.as_u64().is_some(),
            "number"=>value.is_number(),"boolean"=>value.is_boolean(),"null"=>value.is_null(),
            _=>false
        }){return Err(format!("{path}: expected {}",types.join(" or ")));}
    }
    for choice in ["anyOf","oneOf","allOf"] {
        if let Some(variants)=schema.get(choice).and_then(Value::as_array){
            let passed=variants.iter().filter(|rule|
                validate_value(rule,value,depth+1,path).is_ok()).count();
            let accepted=match choice {
                "anyOf"=>passed>=1,"oneOf"=>passed==1,"allOf"=>passed==variants.len(),
                _=>false
            };
            if !accepted{return Err(format!("{path}: {choice} constraint failed"));}
        }
    }
    if schema.get("enum").and_then(Value::as_array).is_some_and(|list|!list.contains(value)){
        return Err(format!("{path}: value is not an allowed enum member"));
    }
    if schema.get("const").is_some_and(|v|v!=value){
        return Err(format!("{path}: expected constant value"));
    }
    if let Some(obj)=value.as_object(){
        if let Some(required)=schema.get("required").and_then(Value::as_array){
            for field in required {
                let name=field.as_str().ok_or_else(||format!("{path}: invalid required field"))?;
                if !obj.contains_key(name){return Err(format!("{path}.{name}: required argument missing"));}
            }
        }
        let size=obj.len() as u64;
        if size<schema.get("minProperties").and_then(Value::as_u64).unwrap_or(0){
            return Err(format!("{path}: too few object properties"));
        }
        if schema.get("maxProperties").and_then(Value::as_u64).is_some_and(|max|size>max){
            return Err(format!("{path}: too many object properties"));
        }
        let properties=schema.get("properties").and_then(Value::as_object);
        for (name,child) in obj{
            if let Some(rule)=properties.and_then(|p|p.get(name)){
                validate_value(rule,child,depth+1,&format!("{path}.{name}"))?;
            } else {
                match schema.get("additionalProperties"){
                    Some(Value::Bool(false))=>return Err(format!("{path}.{name}: unknown argument")),
                    Some(Value::Object(rule))=>
                        validate_value(&Value::Object(rule.clone()),child,depth+1,&format!("{path}.{name}"))?,
                    _=>{}
                }
            }
        }
    }
    if let Some(items)=value.as_array(){
        let len=items.len() as u64;
        if len<schema.get("minItems").and_then(Value::as_u64).unwrap_or(0){
            return Err(format!("{path}: too few array items"));
        }
        if schema.get("maxItems").and_then(Value::as_u64).is_some_and(|max|len>max){
            return Err(format!("{path}: too many array items"));
        }
        if let Some(rule)=schema.get("items"){
            for (idx,child) in items.iter().enumerate(){
                validate_value(rule,child,depth+1,&format!("{path}[{idx}]"))?;
            }
        }
    }
    if let Some(text)=value.as_str(){
        let size=text.chars().count() as u64;
        if size<schema.get("minLength").and_then(Value::as_u64).unwrap_or(0){
            return Err(format!("{path}: string too short"));
        }
        if schema.get("maxLength").and_then(Value::as_u64).is_some_and(|max|size>max){
            return Err(format!("{path}: string too long"));
        }
    }
    if let Some(number)=value.as_f64(){
        if schema.get("minimum").and_then(Value::as_f64).is_some_and(|min|number<min){
            return Err(format!("{path}: smaller than minimum"));
        }
        if schema.get("maximum").and_then(Value::as_f64).is_some_and(|max|number>max){
            return Err(format!("{path}: exceeds maximum"));
        }
        if schema.get("exclusiveMinimum").and_then(Value::as_f64).is_some_and(|min|number<=min){
            return Err(format!("{path}: must be strictly greater than exclusiveMinimum"));
        }
        if schema.get("exclusiveMaximum").and_then(Value::as_f64).is_some_and(|max|number>=max){
            return Err(format!("{path}: must be strictly less than exclusiveMaximum"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    fn tool(schema:Value)->Vec<Value>{
        vec![json!({"type":"function","function":{"name":"update","parameters":schema}})]
    }
    fn check(schema:Value,args:Value)->Result<(),AdmissionError>{
        admit(&ToolCall{id:"id".into(),name:"update".into(),arguments:args},&tool(schema))
    }
    #[test]
    fn all_builtin_tool_schemas_are_accepted_by_real_admission_validator(){
        for tool in crate::tools::definitions(true){
            let name=tool.pointer("/function/name").and_then(Value::as_str).unwrap();
            let schema=tool.pointer("/function/parameters").unwrap();
            check_schema(schema,0,"$").unwrap_or_else(|error|
                panic!("native tool {name} advertises unusable schema: {error}"));
        }
    }
    #[test]
    fn exclusive_numeric_bounds_are_enforced_in_actual_model_calls(){
        let schema=json!({"type":"object","properties":{
            "price":{"type":"number","exclusiveMinimum":0,"exclusiveMaximum":100}},
            "required":["price"]});
        assert!(check(schema.clone(),json!({"price":0.01})).is_ok());
        for price in [0.0,-1.0,100.0,101.0]{
            assert_eq!(check(schema.clone(),json!({"price":price})).unwrap_err().code,
                AdmissionCode::InvalidArguments);
        }
        assert_eq!(check(json!({"type":"object","properties":{"x":{
            "type":"number","exclusiveMinimum":5,"maximum":5
        }}}),json!({"x":5})).unwrap_err().code,AdmissionCode::InvalidSchema);
    }
    #[test]
    fn validates_required_nested_types_and_bounds(){
        let schema=json!({"type":"object","properties":{
            "path":{"type":"string","minLength":2},
            "count":{"type":"integer","minimum":1,"maximum":5},
            "flags":{"type":"array","items":{"type":"boolean"},"maxItems":2}},
            "required":["path","count"],"additionalProperties":false});
        assert!(check(schema.clone(),json!({"path":"src","count":2,"flags":[true]})).is_ok());
        for args in [
            json!({"count":2}),json!({"path":"ok","count":0}),
            json!({"path":"ok","count":5.2}),json!({"path":"ok","count":2,"flags":[3]}),
            json!({"path":"ok","count":2,"unknown":1}),
            json!({"path":"x","count":2})
        ]{assert!(check(schema.clone(),args).is_err());}
    }
    #[test]
    fn unknown_tools_are_never_implicitly_executable(){
        let call=ToolCall{id:"id".into(),name:"write_file".into(),arguments:json!({"path":"a"})};
        assert_eq!(admit(&call,&tool(json!({"type":"object"}))).unwrap_err().code,AdmissionCode::UnknownTool);
    }
    #[test]
    fn unsupported_schema_constraints_fail_closed(){
        assert_eq!(check(json!({"type":"object","properties":{"id":{"type":"string","pattern":"^good$"}}}),
            json!({"id":"bad"})).unwrap_err().code,AdmissionCode::InvalidSchema);
    }
    #[test]
    fn mcp_union_schema_validates_all_branches_and_nullable_fields(){
        let schema=json!({"type":"object","properties":{
            "target":{"oneOf":[
                {"type":"object","properties":{"id":{"type":"integer"}},
                 "required":["id"],"additionalProperties":false},
                {"type":"object","properties":{"url":{"type":"string","minLength":4}},
                 "required":["url"],"additionalProperties":false}
            ]},
            "note":{"type":["string","null"],"minLength":2}
        },"required":["target"],"additionalProperties":false});
        assert!(check(schema.clone(),json!({"target":{"id":4},"note":null})).is_ok());
        assert!(check(schema.clone(),json!({"target":{"url":"http"},"note":"ok"})).is_ok());
        for invalid in [
            json!({"target":{}}),
            json!({"target":{"id":"4"}}),
            json!({"target":{"id":1,"url":"http"}}),
            json!({"target":{"url":"a"},"note":"a"})
        ]{assert_eq!(check(schema.clone(),invalid).unwrap_err().code,AdmissionCode::InvalidArguments);}
        assert_eq!(check(json!({"type":"object","properties":{"broken":{
            "type":["string","string"]}}}),json!({"broken":"x"}))
            .unwrap_err().code,AdmissionCode::InvalidSchema);
    }
    #[test]
    fn mcp_anyof_allof_and_typed_additional_properties_fail_closed(){
        let schema=json!({"type":"object","minProperties":1,"maxProperties":2,
            "additionalProperties":{"anyOf":[
                {"type":"integer","minimum":1},
                {"type":"string","minLength":2}
            ]}});
        assert!(check(schema.clone(),json!({"a":3,"b":"ok"})).is_ok());
        for invalid in [json!({}),json!({"a":0}),json!({"a":null}),
            json!({"a":1,"b":2,"c":3}),json!({"a":"x"})]{
            assert!(check(schema.clone(),invalid).is_err());
        }
        assert!(check(json!({"type":"object","properties":{"x":{
            "allOf":[{"type":"integer","minimum":1},{"type":"integer","maximum":5}]
        }}}),json!({"x":4})).is_ok());
        assert!(check(json!({"type":"object","properties":{"x":{
            "allOf":[{"type":"integer","minimum":1},{"type":"integer","maximum":5}]
        }}}),json!({"x":9})).is_err());
        // Unknown constraints must not be ignored, even under an alternative.
        assert_eq!(check(json!({"type":"object","properties":{"x":{
            "anyOf":[{"type":"string"},{"type":"string","pattern":"^safe$"}]
        }}}),json!({"x":"unsafe"}))
            .unwrap_err().code,AdmissionCode::InvalidSchema);
    }
    #[test]
    fn enum_constant_and_nested_structures(){
        assert!(check(json!({"type":"object","properties":{
            "mode":{"type":"string","enum":["fast","safe"]},
            "metadata":{"type":"object","properties":{"n":{"type":"null","const":null}}}}}),
            json!({"mode":"safe","metadata":{"n":null}})).is_ok());
        assert!(check(json!({"type":"object","properties":{
            "mode":{"type":"string","enum":["fast","safe"]}}}),
            json!({"mode":"unsafe"})).is_err());
    }
}
