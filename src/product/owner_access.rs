//! Host and browser-origin admission for the local owner application.
//! This protects the whole HTTP router (UI plus both API namespaces), not
//! only writes or endpoints that happen to require a bearer token.
//! Authentication for remote owner calls and scoped workers stays in the
//! existing native_api::authorize / remote_worker handlers.
use crate::{config::Settings,engine::Engine};
use axum::{
    extract::State,
    http::{header,HeaderMap,Method,StatusCode,uri::Authority},
    middleware::Next,
    response::{IntoResponse,Response},
    Json,
};
use serde_json::json;
use std::{net::SocketAddr,str::FromStr,sync::Arc};

fn one_header<'a>(headers:&'a HeaderMap,name:header::HeaderName)
    ->Result<Option<&'a str>,&'static str>{
    let mut values=headers.get_all(name).iter();
    let Some(first)=values.next() else{return Ok(None)};
    if values.next().is_some(){return Err("ambiguous HTTP authority or origin");}
    let value=first.to_str().map_err(|_|"invalid HTTP authority or origin")?;
    if value.trim()!=value || value.is_empty(){return Err("invalid HTTP authority or origin");}
    Ok(Some(value))
}
fn authority(raw:&str)->Result<Authority,&'static str>{
    // HTTP Host is an authority, never an arbitrary URL or userinfo.
    if raw.chars().any(|ch|matches!(ch,'@'|'/'|'\\'|'?'|'#'|','|'%'|' ')) ||
        raw.bytes().any(|byte|byte.is_ascii_control()){
        return Err("invalid Host authority");
    }
    let parsed=Authority::from_str(raw).map_err(|_|"invalid Host authority")?;
    if let Some(port)=parsed.port_u16(){
        if port==0{return Err("invalid Host port");}
    }else if raw.starts_with('['){
        if let Some((_,suffix))=raw.split_once(']'){
            if !suffix.is_empty(){return Err("invalid Host port");}
        }else{return Err("invalid IPv6 Host");}
    }else if raw.contains(':'){
        return Err("invalid Host port");
    }
    Ok(parsed)
}
fn hostname(value:&str)->&str{
    value.strip_prefix('[').and_then(|v|v.strip_suffix(']'))
        .unwrap_or(value)
}
fn is_loopback_host(host:&str)->bool{
    matches!(hostname(host).to_ascii_lowercase().as_str(),
        "localhost"|"127.0.0.1"|"::1")
}
fn is_api(path:&str)->bool{
    path=="/api"||path.starts_with("/api/")
}
fn valid_origin(origin:&str,host:&Authority,bind:SocketAddr)
    ->Result<bool,&'static str>{
    if origin=="null"{return Ok(false);}
    let url=reqwest::Url::parse(origin).map_err(|_|"invalid Origin")?;
    if !matches!(url.scheme(),"http"|"https") ||
        !url.username().is_empty()||url.password().is_some()||
        url.path()!="/"||url.query().is_some()||url.fragment().is_some(){
        return Ok(false);
    }
    let Some(origin_host)=url.host_str() else{return Ok(false)};
    if bind.ip().is_loopback(){
        // Browser development uses a locally bound Vite proxy on :5173;
        // production UI uses the exact configured Rust port.
        return Ok(url.scheme()=="http" && is_loopback_host(origin_host)
            && (url.port_or_known_default()==Some(bind.port()) ||
                url.port_or_known_default()==Some(5173)));
    }
    // Remotely exposed owners must provide an explicit token, and browsers
    // must have a same-authority Origin. HTTPS reverse proxies retain Host.
    Ok(origin_host.eq_ignore_ascii_case(hostname(host.host())) &&
        url.port_or_known_default()==Some(host.port_u16()
            .unwrap_or(if url.scheme()=="https"{443}else{80})))
}
fn check_request(settings:&Settings,method:&Method,path:&str,headers:&HeaderMap)
    ->Result<(),&'static str>{
    let host=one_header(headers,header::HOST)?
        .ok_or("Host required for owner HTTP requests")?;
    let host=authority(host)?;
    if settings.bind.ip().is_loopback(){
        if !is_loopback_host(host.host()){
            return Err("untrusted Host for local owner service");
        }
        if host.port_u16().is_some_and(|port|port!=settings.bind.port()){
            return Err("Host port does not match owner service");
        }
    }else if settings.token.is_none() && is_api(path){
        // Settings::from_env normally refuses this, but the HTTP adapter
        // must not become unauthenticated under programmatic construction.
        return Err("remote owner API requires an access token");
    }

    let callback=super::native_api::is_oauth_callback(method,path);
    if let Some(origin)=one_header(headers,header::ORIGIN)?{
        if !callback && !valid_origin(origin,&host,settings.bind)?{
            return Err("cross-origin owner access denied");
        }
    }
    if is_api(path)&&!callback &&
        one_header(headers,header::HeaderName::from_static("sec-fetch-site"))?
            ==Some("cross-site"){
        return Err("cross-site browser owner access denied");
    }
    Ok(())
}
pub(crate) async fn guard(
    State(engine):State<Arc<Engine>>,
    request:axum::extract::Request,
    next:Next
)->Response{
    if let Err(reason)=check_request(&engine.settings,request.method(),
        request.uri().path(),request.headers()){
        return (StatusCode::FORBIDDEN,Json(json!({"error":reason})))
            .into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests{
    use super::*;
    use axum::http::{HeaderValue,Method};
    fn local()->Settings{
        let mut config=Settings::from_env().unwrap();
        config.bind="127.0.0.1:4323".parse().unwrap();
        config.token=None;
        config
    }
    fn request(host:&str,origin:Option<&str>,site:Option<&str>)->HeaderMap{
        let mut headers=HeaderMap::new();
        headers.insert(header::HOST,HeaderValue::from_str(host).unwrap());
        if let Some(value)=origin{
            headers.insert(header::ORIGIN,HeaderValue::from_str(value).unwrap());
        }
        if let Some(value)=site{
            headers.insert("sec-fetch-site",HeaderValue::from_str(value).unwrap());
        }
        headers
    }
    #[test]
    fn local_owner_api_rejects_rebound_hosts_for_reads_and_writes(){
        let config=local();
        for method in [Method::GET,Method::POST,Method::DELETE]{
            for path in ["/api/sessions","/api/v1/runs","/api/memories"]{
                for host in ["attacker.example:4323","127.0.0.1.attacker.example:4323",
                    "localhost.attacker.example:4323","evil@localhost:4323",
                    "localhost:9999","localhost:4323,attacker.example"]{
                    assert!(check_request(&config,&method,path,
                        &request(host,None,None)).is_err(),
                        "{method} {path} with spoofed Host {host}");
                }
                assert!(check_request(&config,&method,path,
                    &request("127.0.0.1:4323",None,None)).is_ok());
                assert!(check_request(&config,&method,path,
                    &request("localhost:4323",None,None)).is_ok());
            }
        }
        assert!(check_request(&config,&Method::GET,"/", &HeaderMap::new()).is_err(),
            "an empty Host must not bypass the owner gate");
    }
    #[test]
    fn cross_site_origin_and_fetch_metadata_do_not_bypass_local_read_api(){
        let settings=local();
        for origin in ["null","https://attacker.example",
            "http://localhost.attacker.example:4323",
            "http://127.0.0.1:9999","file:///tmp/evil.html",
            "http://localhost:4323.evil.example"]{
            assert!(check_request(&settings,&Method::GET,"/api/sessions",
                &request("localhost:4323",Some(origin),None)).is_err(),
                "untrusted browser Origin {origin}");
        }
        assert!(check_request(&settings,&Method::GET,"/api/sessions",
            &request("127.0.0.1:4323",Some("http://127.0.0.1:4323"),
                Some("same-origin"))).is_ok());
        assert!(check_request(&settings,&Method::POST,"/api/chat",
            &request("127.0.0.1:4323",Some("http://localhost:5173"),
                Some("same-site"))).is_ok());
        assert!(check_request(&settings,&Method::GET,"/api/sessions",
            &request("127.0.0.1:4323",None,Some("cross-site"))).is_err());
    }
    #[test]
    fn authorized_oauth_redirect_remains_external_but_not_general_owner_get(){
        let settings=local();
        let headers=request("localhost:4323",Some("https://github.com"),
            Some("cross-site"));
        assert!(check_request(&settings,&Method::GET,
            "/api/oauth/github/callback",&headers).is_ok());
        assert!(check_request(&settings,&Method::GET,
            "/api/sessions",&headers).is_err());
        assert!(check_request(&settings,&Method::POST,
            "/api/oauth/github/callback",&headers).is_err());
    }
    #[test]
    fn remote_api_requires_owner_token_but_preserves_scoped_worker_admission(){
        let mut config=local();
        config.bind="0.0.0.0:4323".parse().unwrap();
        let headers=request("agent.example:443",None,None);
        assert!(check_request(&config,&Method::GET,"/api/sessions",&headers)
            .is_err());
        config.token=Some("fixture-strong-token".into());
        assert!(check_request(&config,&Method::GET,"/api/sessions",&headers)
            .is_ok());
        assert!(check_request(&config,&Method::POST,
            "/api/v1/remote-worker/runs",&headers).is_ok(),
            "actual scope authorization remains in remote_worker");
        assert!(check_request(&config,&Method::POST,"/api/chat",
            &request("agent.example:443",Some("https://evil.example"),None))
            .is_err());
        assert!(check_request(&config,&Method::POST,"/api/chat",
            &request("agent.example:443",Some("https://agent.example"),None))
            .is_ok());
    }
}
