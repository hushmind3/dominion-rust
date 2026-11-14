//! Native HTTP and web search boundary: DNS pinning, SSRF rejection, bounded responses.
use super::*;

pub(super) const HTTP_FETCH_MAX_BYTES: usize = 256 * 1024;
pub(super) const HTTP_FETCH_MAX_REDIRECTS: usize = 5;

/// Public-network requests must not resolve to local interfaces, metadata
/// services, private infrastructure, or reserved addresses.
pub(super) fn public_http_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let [a,b,c,d]=v4.octets();
            let _=d;
            !(a==0 || a==10 || a==127 || a>=224
                || (a==100 && (64..=127).contains(&b))
                || (a==169 && b==254)
                || (a==172 && (16..=31).contains(&b))
                || (a==192 && (b==168 || (b==0 && c<=2)))
                || (a==198 && (b==18 || b==19 || b==51 && c==100))
                || (a==203 && b==0 && c==113)
                || (a==192 && b==88 && c==99))
        }
        std::net::IpAddr::V6(v6) => {
            if let Some(v4)=v6.to_ipv4_mapped() {
                return public_http_ip(std::net::IpAddr::V4(v4));
            }
            let segments=v6.segments();
            // Restrict to global-unicast 2000::/3. Reject NAT64,
            // site-local, documentation, and link-local translation ranges.
            !((segments[0]&0xe000)!=0x2000
                || v6.is_unspecified() || v6.is_loopback() || v6.is_multicast()
                || (segments[0]&0xfe00)==0xfc00
                || (segments[0]&0xffc0)==0xfe80
                || (segments[0]&0xff00)==0x2000 && segments[0]==0x2001 && segments[1]==0xdb8)
        }
    }
}

pub(super) fn validate_http_target(parsed:&reqwest::Url)->Result<(String,u16),String> {
    if !matches!(parsed.scheme(),"http"|"https") {
        return Err("http_fetch requires HTTP(S)".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some(){
        return Err("http_fetch does not permit embedded credentials".into());
    }
    let host=parsed.host_str().ok_or("http_fetch URL has no host")?
        .trim_matches(['[',']']).trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty()
        || host=="localhost" || host.ends_with(".localhost")
        || [".local",".internal",".lan",".test",".invalid",".home.arpa",".onion"]
            .iter().any(|suffix|host.ends_with(suffix))
    {
        return Err("http_fetch blocked a local or non-public hostname".into());
    }
    if let Ok(ip)=host.parse::<std::net::IpAddr>(){
        if !public_http_ip(ip){return Err("http_fetch blocked a non-public IP".into());}
    }
    let port=parsed.port_or_known_default().ok_or("http_fetch has no valid port")?;
    Ok((host,port))
}

/// Resolve before connecting, reject mixed public/private DNS answers, and
/// pin exactly the approved addresses in reqwest to prevent DNS rebinding.
/// Redirects are handled manually so the same validation runs on every hop.
pub(super) async fn http_fetch(url: &str) -> Result<Value, String> {
    let mut current=reqwest::Url::parse(url).map_err(|error|error.to_string())?;
    for redirects in 0..=HTTP_FETCH_MAX_REDIRECTS {
        let (host,port)=validate_http_target(&current)?;
        let addresses=tokio::net::lookup_host((host.as_str(),port))
            .await.map_err(|e|format!("http_fetch DNS lookup failed: {e}"))?
            .collect::<Vec<_>>();
        if addresses.is_empty() || addresses.iter().any(|addr|!public_http_ip(addr.ip())){
            return Err("http_fetch blocked a private or unresolvable DNS destination".into());
        }
        let client=reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .resolve_to_addrs(&host,&addresses)
            .timeout(Duration::from_secs(30))
            .build().map_err(|e|e.to_string())?;
        let mut response=client.get(current.clone())
            .header("User-Agent","DOMINION/0.2 (+rust)")
            .send().await.map_err(|e|e.to_string())?;
        if response.status().is_redirection() {
            if redirects==HTTP_FETCH_MAX_REDIRECTS {
                return Err("http_fetch exceeded redirect limit".into());
            }
            let location=response.headers().get(reqwest::header::LOCATION)
                .ok_or("http_fetch redirect omitted Location")?
                .to_str().map_err(|e|e.to_string())?;
            current=current.join(location).map_err(|e|e.to_string())?;
            continue;
        }
        if response.content_length().is_some_and(|size|size>HTTP_FETCH_MAX_BYTES as u64){
            return Err("http_fetch response exceeds 256 KiB".into());
        }
        let status=response.status().as_u16();
        let content_type=response.headers().get(reqwest::header::CONTENT_TYPE)
            .and_then(|value|value.to_str().ok()).unwrap_or("").to_string();
        let mut bytes=Vec::new();
        while let Some(chunk)=response.chunk().await.map_err(|e|e.to_string())?{
            if chunk.len()>HTTP_FETCH_MAX_BYTES.saturating_sub(bytes.len()){
                return Err("http_fetch response exceeds 256 KiB".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let body=String::from_utf8_lossy(&bytes);
        let text=if content_type.contains("html"){strip_html(&body)}else{body.into_owned()};
        return Ok(json!({"status":status,"contentType":content_type,
            "text":text.chars().take(60_000).collect::<String>(),
            "bytesRead":bytes.len(),"redirects":redirects}));
    }
    Err("http_fetch redirect limit reached".into())
}

pub(super) async fn web_search(query: &str) -> Result<Value, String> {
    let response = reqwest::Client::new()
        .post("https://html.duckduckgo.com/html/")
        .form(&[("q", query)])
        .timeout(Duration::from_secs(25))
        .header(
            "User-Agent",
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)",
        )
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let html = response.text().await.map_err(|error| error.to_string())?;
    let mut results = Vec::new();
    for chunk in html.split("result__a").skip(1).take(10) {
        let Some(href_start) = chunk.find("href=\"") else {
            continue;
        };
        let rest = &chunk[href_start + 6..];
        let Some(href_end) = rest.find('"') else {
            continue;
        };
        let href = decode_ddg(&rest[..href_end]);
        let title = rest[href_end..]
            .find('>')
            .map(|start| {
                let tail = &rest[href_end + start + 1..];
                let end = tail.find("</a>").unwrap_or(tail.len().min(200));
                strip_html(&tail[..end]).trim().to_string()
            })
            .unwrap_or_default();
        if !href.is_empty() && !title.is_empty() {
            results.push(json!({ "title": title, "url": href }));
        }
    }
    Ok(json!({ "query": query, "results": results }))
}

pub(super) fn decode_ddg(raw: &str) -> String {
    let href = raw
        .strip_prefix("//")
        .map(|rest| format!("https://{rest}"))
        .unwrap_or_else(|| raw.to_string());
    if let Some(index) = href.find("uddg=") {
        let value = &href[index + 5..];
        let value = value.split('&').next().unwrap_or(value);
        return percent_decode(value);
    }
    href
}

pub(super) fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(if bytes[index] == b'+' {
            b' '
        } else {
            bytes[index]
        });
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub(super) fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

