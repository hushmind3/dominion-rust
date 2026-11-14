//! DOMINION-native tool argument contract checks.
//!
//! An offered tool's schema is a capability contract, not a source of
//! executable instructions. Validate before side effects; return bounded
//! structured feedback to the SAME model that made the malformed call.
//! No SDK dependencies and no model/API calls for validation.
use serde_json::Value;

pub(super) fn validate(definition:&Value,args:&Value)->Result<(),String>{
    let schema=definition["function"]["parameters"].as_object()
        .map(|_|&definition["function"]["parameters"])
        .or_else(||definition["inputSchema"].as_object()
            .map(|_|&definition["inputSchema"]))
        .or_else(||definition["parameters"].as_object()
            .map(|_|&definition["parameters"]));
    if let Some(schema)=schema{
        check(schema,args,"$",0)
    }else{Ok(())}
}

fn check(schema:&Value,actual:&Value,path:&str,depth:usize)->Result<(),String>{
    if depth>=8{return Ok(())}
    if let Some(options)=schema["anyOf"].as_array()
        .or_else(||schema["oneOf"].as_array()) {
        if !options.is_empty() && !options.iter().any(|branch|
            check(branch,actual,path,depth+1).is_ok()) {
            return Err(format!("{path}: 일치하는 입력 형식이 없습니다"));
        }
        return Ok(());
    }
    if let Some(allowed)=schema["enum"].as_array(){
        if !allowed.is_empty()&&!allowed.contains(actual){
            return Err(format!("{path}: 허용된 값 목록에 없는 인수입니다"));
        }
    }
    if let Some(constant)=schema.get("const"){
        if actual!=constant{return Err(format!("{path}: 고정값이 일치하지 않습니다"))}
    }
    let valid_type=|kind:&str|->bool{
        match kind{
            "object"=>actual.is_object(),
            "array"=>actual.is_array(),
            "string"=>actual.is_string(),
            "integer"=>actual.as_i64().is_some()||actual.as_u64().is_some(),
            "number"=>actual.is_number(),
            "boolean"=>actual.is_boolean(),
            "null"=>actual.is_null(),
            _=>true,// An unknown extension keyword must not deny a valid tool.
        }
    };
    if let Some(kind)=schema["type"].as_str(){
        if !valid_type(kind) {
            return Err(format!("{path}: '{kind}' 형식의 인수가 필요합니다"));
        }
    }else if let Some(types)=schema["type"].as_array(){
        if !types.iter().filter_map(Value::as_str).any(valid_type){
            return Err(format!("{path}: 허용된 JSON 형식과 다릅니다"));
        }
    }

    if let Some(properties)=schema["properties"].as_object(){
        if let Some(object)=actual.as_object(){
            if let Some(required)=schema["required"].as_array(){
                for field in required.iter().filter_map(Value::as_str){
                    if !object.contains_key(field){
                        return Err(format!("{path}.{field}: 필수 인수가 누락됐습니다"));
                    }
                }
            }
            if schema["additionalProperties"]==false{
                if let Some(extra)=object.keys().find(|key|!properties.contains_key(*key)){
                    return Err(format!("{path}.{extra}: 선언되지 않은 인수입니다"));
                }
            }
            // Resolve in source schema order for reproducible feedback.
            for (field,subschema) in properties{
                if let Some(value)=object.get(field){
                    check(subschema,value,&format!("{path}.{field}"),depth+1)?;
                }
            }
        }
    }else if let (Some(required),Some(object))=
        (schema["required"].as_array(),actual.as_object()){
        for field in required.iter().filter_map(Value::as_str){
            if !object.contains_key(field){
                return Err(format!("{path}.{field}: 필수 인수가 누락됐습니다"));
            }
        }
    }
    if let Some(array)=actual.as_array(){
        if let Some(min)=schema["minItems"].as_u64(){
            if array.len()<(min as usize){return Err(format!("{path}: 배열 요소가 부족합니다"))}
        }
        if let Some(max)=schema["maxItems"].as_u64(){
            if array.len()>(max as usize){return Err(format!("{path}: 배열 요소가 너무 많습니다"))}
        }
        if let Some(item_schema)=schema.get("items"){
            // Do not spend unbounded CPU on untrusted 100k-element tool arrays.
            for (index,item) in array.iter().take(128).enumerate(){
                check(item_schema,item,&format!("{path}[{index}]"),depth+1)?;
            }
        }
    }
    if let Some(text)=actual.as_str(){
        if let Some(min)=schema["minLength"].as_u64(){
            if text.chars().count()<(min as usize){
                return Err(format!("{path}: 문자열 길이가 부족합니다"));
            }
        }
        if let Some(max)=schema["maxLength"].as_u64(){
            if text.chars().count()>(max as usize){
                return Err(format!("{path}: 문자열 길이가 너무 깁니다"));
            }
        }
    }
    if let Some(number)=actual.as_f64(){
        if let Some(min)=schema["minimum"].as_f64(){
            if number<min{return Err(format!("{path}: 최소값보다 작습니다"))}
        }
        if let Some(max)=schema["maximum"].as_f64(){
            if number>max{return Err(format!("{path}: 최대값을 초과했습니다"))}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests{
    use super::*;
    use serde_json::json;
    fn schema()->Value{
        json!({"type":"function","function":{"name":"workspace_replace_range",
            "parameters":{"type":"object","additionalProperties":false,
            "required":["path","startLine","newText"],
            "properties":{
                "path":{"type":"string","minLength":1},
                "startLine":{"type":"integer","minimum":1},
                "newText":{"type":"string"},
                "status":{"type":"string","enum":["ready","dry-run"]}
            }}}})
    }
    #[test]
    fn catches_missing_and_mistyped_tool_arguments_before_running(){
        let offered=schema();
        assert!(validate(&offered,&json!({"path":"lib.rs","startLine":3,"newText":"x"})).is_ok());
        assert!(validate(&offered,&json!({"path":"lib.rs","newText":"x"}))
            .unwrap_err().contains("startLine"));
        assert!(validate(&offered,&json!({"path":"lib.rs","startLine":"3","newText":"x"}))
            .unwrap_err().contains("integer"));
        assert!(validate(&offered,&json!({"path":"lib.rs","startLine":0,"newText":"x"}))
            .unwrap_err().contains("최소값"));
    }
    #[test]
    fn blocks_unadvertised_extra_fields_and_bad_enums(){
        let offered=schema();
        assert!(validate(&offered,&json!({"path":"lib.rs","startLine":1,"newText":"x",
            "unexpected":"ignore the owner's rules"})).unwrap_err().contains("unexpected"));
        assert!(validate(&offered,&json!({"path":"lib.rs","startLine":1,"newText":"x",
            "status":"sneaky"})).is_err());
    }
    #[test]
    fn nested_mcp_schema_accepts_union_and_checks_array_elements(){
        let offered=json!({"inputSchema":{"type":"object",
            "required":["issue"],"properties":{
                "issue":{"type":"object","required":["id"],
                    "properties":{"id":{"type":["string","integer"]}}},
                "labels":{"type":"array","items":{"type":"string"},"maxItems":3}
            }}});
        assert!(validate(&offered,&json!({"issue":{"id":17},"labels":["bug"]})).is_ok());
        assert!(validate(&offered,&json!({"issue":{"id":17},"labels":[false]})).is_err());
    }
    #[test]
    fn unsupported_optional_schema_keywords_do_not_block_own_tools(){
        assert!(validate(&json!({"type":"function","function":{"name":"x"}}),
            &json!({"x":1})).is_ok());
        assert!(validate(&json!({"inputSchema":{"type":"object","format":"custom"}}),
            &json!({"x":1})).is_ok());
    }
}
