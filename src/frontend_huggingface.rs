//! Read-only Hugging Face search/detail adapter for the latest main React
//! ModelExplorer. The public Hub API needs no user key. Download and execution
//! remain separate native actions; this adapter never writes model files.
use axum::{extract::Query,http::StatusCode,routing::get,Json,Router};
use serde::Deserialize;
use serde_json::{json,Value};
use std::{collections::{BTreeMap,HashSet},sync::Arc,time::Duration};

type Fault=(StatusCode,Json<Value>);
fn fail(status:StatusCode,reason:impl Into<String>)->Fault{
    (status,Json(json!({"error":reason.into()})))
}
pub fn routes()->Router<Arc<crate::engine::Engine>>{
    Router::new()
        .route("/model-library/catalog",get(catalog))
        .route("/model-library/details",get(details))
}
#[derive(Default,Deserialize)]
#[serde(rename_all="camelCase")]
struct CatalogQuery{
    #[serde(default)]q:String,
    #[serde(default)]format:Option<String>,
    #[serde(default)]sort:Option<String>,
    #[serde(default)]limit:Option<usize>,
    #[serde(default)]chat_only:Option<bool>,
    #[serde(default)]max_params:Option<usize>,
}
#[derive(Default,Deserialize)]
struct DetailQuery{id:String}
fn query_url(q:&CatalogQuery,format:&str)->Result<reqwest::Url,String>{
    if q.q.chars().count()>100||q.q.chars().any(char::is_control){
        return Err("model search query exceeds 100 characters or contains controls".into());
    }
    if !matches!(format,"gguf"|"mlx"){return Err("unsupported model format".into());}
    let mut url=reqwest::Url::parse("https://huggingface.co/api/models")
        .map_err(|e|e.to_string())?;
    {
        let mut pairs=url.query_pairs_mut();
        if !q.q.trim().is_empty(){pairs.append_pair("search",q.q.trim());}
        pairs.append_pair("filter",format);
        let sort=match q.sort.as_deref(){
            Some("likes")=>"likes",Some("lastModified")=>"lastModified",_=>"downloads"
        };
        pairs.append_pair("sort",sort);
        pairs.append_pair("direction","-1");
        pairs.append_pair("limit",&q.limit.unwrap_or(40).clamp(1,100).to_string());
        if let Some(n)=q.max_params.filter(|n|[1,2,3,4,8].contains(n)){
            pairs.append_pair("num_parameters",&format!("max:{n}B"));
        }
        if q.chat_only==Some(true){pairs.append_pair("pipeline_tag","text-generation");}
    }
    Ok(url)
}
pub(crate) fn valid_repo(repo:&str)->bool{
    if repo.len()>200{return false;}
    let mut parts=repo.split('/');
    let (Some(user),Some(name),None)=(parts.next(),parts.next(),parts.next())
        else{return false;};
    [user,name].iter().all(|part|!part.is_empty()&&*part!="."&&*part!=".."
        &&part.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'-'|b'_'|b'.')))
}
async fn public_json(url:reqwest::Url,max_bytes:usize)->Result<Value,String>{
    crate::frontend_public_http::read_json(url,
        crate::frontend_public_http::ReadOptions{
            timeout:Duration::from_secs(24),max_bytes,
            status_label:"Hugging Face HTTP",
            too_large:"Hub metadata response is too large",
            invalid_json:"invalid Hub metadata"
        }).await
}
fn list_row(item:&Value)->Option<Value>{
    let id=item["id"].as_str()?;
    if !valid_repo(id){return None;}
    let mut parts=id.split('/');
    let author=parts.next().unwrap_or("");
    let name=parts.next().unwrap_or(id);
    Some(json!({"id":id,"author":author,"name":name,
        "library":item["library_name"].as_str().unwrap_or(""),
        "createdAt":item["createdAt"],
        "downloads":item["downloads"].as_u64().unwrap_or(0),
        "likes":item["likes"].as_u64().unwrap_or(0),
        "gated":item["gated"]!=false&&item["gated"]!=Value::Null,
        "updatedAt":item["lastModified"],
        "description":"","task":item["pipeline_tag"].as_str().unwrap_or(""),
        "tags":item["tags"].as_array().cloned().unwrap_or_default()}))
}
async fn catalog_one(q:&CatalogQuery,format:&str)->Result<Vec<Value>,String>{
    let url=query_url(q,format)?;
    let value=public_json(url,4*1024*1024).await?;
    let rows=value.as_array().ok_or("Hub model search returned no list")?;
    Ok(rows.iter().filter_map(list_row).collect())
}
async fn catalog(Query(q):Query<CatalogQuery>)->Result<Json<Value>,Fault>{
    let format=q.format.as_deref().unwrap_or("gguf");
    if !matches!(format,"gguf"|"mlx"|"all"){
        return Err(fail(StatusCode::BAD_REQUEST,"unsupported Hub format"));
    }
    let rows=if format=="all"{
        let (gguf,mlx)=tokio::join!(catalog_one(&q,"gguf"),catalog_one(&q,"mlx"));
        if gguf.is_err()&&mlx.is_err(){
            return Err(fail(StatusCode::BAD_GATEWAY,format!("Hub GGUF: {}; MLX: {}",
                gguf.err().unwrap(),mlx.err().unwrap())));
        }
        let mut rows=gguf.unwrap_or_default();rows.extend(mlx.unwrap_or_default());
        let mut seen=HashSet::new();
        rows.retain(|row|row["id"].as_str().is_some_and(|id|seen.insert(id.to_owned())));
        let sort=q.sort.as_deref().unwrap_or("downloads");
        rows.sort_by(|a,b|match sort{
            "likes"=>b["likes"].as_u64().cmp(&a["likes"].as_u64()),
            "lastModified"=>b["updatedAt"].as_str().cmp(&a["updatedAt"].as_str()),
            _=>b["downloads"].as_u64().cmp(&a["downloads"].as_u64())
        });
        rows.truncate(q.limit.unwrap_or(40).clamp(1,100));rows
    }else{
        catalog_one(&q,format).await.map_err(|e|fail(StatusCode::BAD_GATEWAY,e))?
    };
    Ok(Json(json!(rows)))
}
pub(crate) fn safe_relative(file:&str)->bool{
    !file.is_empty()&&!file.starts_with('/')&&!file.contains('\\')
        &&file.split('/').all(|part|!part.is_empty()&&part!="."&&part!="..")
}
fn shard_parts(file:&str)->Option<(String,usize,usize)>{
    let stem=file.strip_suffix(".gguf")?;
    let (first,total)=stem.rsplit_once("-of-")?;
    let (prefix,number)=first.rsplit_once('-')?;
    if number.len()!=5||total.len()!=5{return None;}
    let number=number.parse::<usize>().ok()?;
    let total=total.parse::<usize>().ok()?;
    if !(1..=256).contains(&total)||number==0||number>total{return None;}
    Some((prefix.into(),number,total))
}
fn gguf_options(siblings:&[Value])->Vec<Value>{
    let mut rows=Vec::new();
    let mut groups=BTreeMap::<(String,usize),BTreeMap<usize,(String,u64)>>::new();
    for entry in siblings.iter().take(10_000){
        let Some(file)=entry["rfilename"].as_str()else{continue};
        if !file.to_ascii_lowercase().ends_with(".gguf")||!safe_relative(file){continue;}
        let size=entry["size"].as_u64().unwrap_or(0);
        if let Some((prefix,num,total))=shard_parts(file){
            groups.entry((prefix,total)).or_default()
                .insert(num,(file.to_string(),size));
            continue;
        }
        let quantization=file.trim_end_matches(".gguf").rsplit('-').next()
            .unwrap_or("").to_ascii_uppercase();
        rows.push(json!({"id":file,"format":"gguf","label":file.rsplit('/').next(),
            "quantization":quantization,"bytes":size,"files":[file],"shards":1}));
    }
    for ((prefix,total),group) in groups{
        if group.len()!=total || !(1..=total).all(|i|group.contains_key(&i)){continue;}
        let files=(1..=total).map(|i|group[&i].0.clone()).collect::<Vec<_>>();
        let bytes=group.values().map(|(_,size)|size).sum::<u64>();
        rows.push(json!({"id":files[0],"format":"gguf","label":prefix,
            "quantization":"","bytes":bytes,"files":files,"shards":total}));
    }
    rows.sort_by_key(|row|row["bytes"].as_u64().unwrap_or(0));
    rows
}
fn mlx_option(siblings:&[Value])->Option<Value>{
    let mut files=Vec::new();let mut shards=0;let mut config=false;let mut bytes=0u64;
    for entry in siblings.iter().take(10_000){
        let Some(file)=entry["rfilename"].as_str()else{continue};
        if !safe_relative(file){continue;}
        let lower=file.to_ascii_lowercase();
        if lower.starts_with("original/")||lower.starts_with("onnx/"){continue;}
        let filename=lower.rsplit('/').next().unwrap_or("");
        let include=lower=="config.json"||lower.ends_with(".safetensors")
            ||lower.ends_with(".safetensors.index.json")
            ||matches!(filename,"tokenizer.json"|"tokenizer_config.json"
                |"special_tokens_map.json"|"vocab.json"|"merges.txt"
                |"added_tokens.json"|"generation_config.json"|"chat_template.jinja");
        if !include{continue;}
        if lower=="config.json"{config=true;}
        if lower.ends_with(".safetensors"){shards+=1;}
        let size=entry["size"].as_u64().unwrap_or(0);
        bytes=bytes.saturating_add(size);
        files.push(file.to_string());
    }
    if !config||shards==0{return None;}
    Some(json!({"id":"mlx","format":"mlx","label":format!("MLX · {shards} shards"),
        "quantization":"","bytes":bytes,"files":files,"shards":shards}))
}
pub(crate) fn detail_row(repo:&str,doc:&Value)->Value{
    let siblings=doc["siblings"].as_array().cloned().unwrap_or_default();
    let mut options=gguf_options(&siblings);
    if let Some(mlx)=mlx_option(&siblings){options.push(mlx);}
    let mut names=repo.split('/');
    let author=names.next().unwrap_or("");
    let name=names.next().unwrap_or(repo);
    json!({"id":repo,"author":author,"name":name,
        "library":doc["library_name"].as_str().unwrap_or(""),
        "downloads":doc["downloads"].as_u64().unwrap_or(0),
        "likes":doc["likes"].as_u64().unwrap_or(0),
        "gated":doc["gated"]!=false&&doc["gated"]!=Value::Null,
        "updatedAt":doc["lastModified"],
        "description":doc["cardData"]["description"].as_str().unwrap_or(""),
        "task":doc["pipeline_tag"].as_str().unwrap_or(""),
        "tags":doc["tags"].as_array().cloned().unwrap_or_default(),
        "revision":doc["sha"].as_str().unwrap_or("main"),
        "license":doc["cardData"]["license"].as_str().unwrap_or(""),
        "architecture":doc["config"]["model_type"].as_str()
            .or_else(||doc["config"]["architectures"][0].as_str()).unwrap_or(""),
        "options":options,"readme":""})
}
pub(crate) async fn raw_repo_details(repo:&str)->Result<Value,String>{
    if !valid_repo(repo){return Err("invalid Hub repository ID".into());}
    let url=reqwest::Url::parse(&format!(
        "https://huggingface.co/api/models/{repo}?blobs=true&cardData=true&config=true"))
        .map_err(|e|e.to_string())?;
    public_json(url,12*1024*1024).await
}
async fn details(Query(q):Query<DetailQuery>)->Result<Json<Value>,Fault>{
    if !valid_repo(&q.id){return Err(fail(StatusCode::BAD_REQUEST,"invalid Hub repository ID"));}
    let value=raw_repo_details(&q.id).await
        .map_err(|e|fail(StatusCode::BAD_GATEWAY,e))?;
    Ok(Json(detail_row(&q.id,&value)))
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn public_hub_queries_are_bounded_and_no_dynamic_endpoint_is_accepted(){
        let q=CatalogQuery{q:"Qwen3".into(),format:Some("gguf".into()),
            sort:Some("likes".into()),limit:Some(900),chat_only:Some(true),
            max_params:Some(4)};
        let url=query_url(&q,"gguf").unwrap().to_string();
        assert!(url.starts_with("https://huggingface.co/api/models?"));
        assert!(url.contains("limit=100"));
        assert!(url.contains("pipeline_tag=text-generation"));
        assert!(!valid_repo("owner/../../secret"));
        assert!(!valid_repo("owner//secret"));
        assert!(valid_repo("owner/a-gguf_3.0"));
        assert!(!valid_repo("localhost@owner/model"));
        assert!(!safe_relative("../escape.gguf"));
    }
    #[test]
    fn real_hub_metadata_builds_complete_gguf_shards_and_mlx_options(){
        let doc=json!({"sha":"abc","siblings":[
            {"rfilename":"Q4_K-00001-of-00002.gguf","size":20},
            {"rfilename":"Q4_K-00002-of-00002.gguf","size":30},
            {"rfilename":"Q8-00001-of-00002.gguf","size":30},
            {"rfilename":"single-Q5.gguf","size":55},
            {"rfilename":"config.json","size":100},
            {"rfilename":"model.safetensors","size":300},
            {"rfilename":"tokenizer.json","size":50}
        ]});
        let detail=detail_row("org/model-gguf",&doc);
        let options=detail["options"].as_array().unwrap();
        assert_eq!(options.len(),3);
        assert_eq!(options[0]["id"],"Q4_K-00001-of-00002.gguf");
        assert_eq!(options[0]["bytes"],50);
        assert_eq!(options[0]["shards"],2);
        assert_eq!(options[1]["id"],"single-Q5.gguf");
        assert_eq!(options[2]["format"],"mlx");
        assert_eq!(detail["revision"],"abc");
    }
}
