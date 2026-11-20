//! One bounded, credential-free HTTP JSON reader for public model metadata.
//! The caller owns the allowlisted URL; this module never adds API tokens.
use serde_json::Value;
use std::{sync::OnceLock,time::Duration};

pub(crate) struct ReadOptions {
    pub timeout:Duration,
    pub max_bytes:usize,
    pub status_label:&'static str,
    pub too_large:&'static str,
    pub invalid_json:&'static str,
}
fn client()->Result<&'static reqwest::Client,String>{
    static CLIENT:OnceLock<Result<reqwest::Client,String>>=OnceLock::new();
    CLIENT.get_or_init(||reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build().map_err(|error|error.to_string()))
        .as_ref().map_err(Clone::clone)
}
pub(crate) async fn read_json(url:reqwest::Url,options:ReadOptions)
    ->Result<Value,String>{
    let mut response=client()?.get(url).timeout(options.timeout)
        .send().await.map_err(|error|error.to_string())?;
    if !response.status().is_success(){
        return Err(format!("{} {}",options.status_label,response.status()));
    }
    if response.content_length().is_some_and(|size|size>options.max_bytes as u64){
        return Err(options.too_large.into());
    }
    let mut bytes=Vec::new();
    while let Some(chunk)=response.chunk().await.map_err(|error|error.to_string())?{
        if bytes.len().saturating_add(chunk.len())>options.max_bytes{
            return Err(options.too_large.into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes)
        .map_err(|error|format!("{}: {error}",options.invalid_json))
}

#[cfg(test)]
mod tests{
    use super::*;
    use axum::{routing::get,Router,Json};
    use serde_json::json;
    #[tokio::test]
    async fn metadata_reader_shares_client_without_losing_size_or_json_bounds(){
        async fn ok()->Json<Value>{Json(json!({"source":"public","models":[1]}))}
        async fn large()->&'static str{"0123456789abcdef"}
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let server=tokio::spawn(async move{
            axum::serve(listener,Router::new()
                .route("/ok",get(ok))
                .route("/large",get(large))).await.unwrap();
        });
        assert!(std::ptr::eq(client().unwrap(),client().unwrap()));
        let options=|limit|ReadOptions{
            timeout:Duration::from_secs(3),max_bytes:limit,
            status_label:"public HTTP",too_large:"response too large",
            invalid_json:"bad JSON"
        };
        let good=read_json(format!("http://{address}/ok").parse().unwrap(),
            options(1024)).await.unwrap();
        assert_eq!(good["models"][0],1);
        let error=read_json(format!("http://{address}/large").parse().unwrap(),
            options(4)).await.unwrap_err();
        assert_eq!(error,"response too large");
        server.abort();
    }
}
