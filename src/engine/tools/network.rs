//! Pinned public HTTP requests, web search and HTML decoding.
//! Active Rust execution paths imported by tools.rs.
use super::*;

const MAX_HTTP_FETCH_BYTES: usize = 256 * 1024;

/// Exclude local, reserved, routing infrastructure and non-public addresses.
/// A request is pinned to the checked DNS answers to close rebinding gaps.
pub(super) fn public_fetch_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0 || a == 10 || a == 127 || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && (b == 168 || b == 0
                    || (b == 88 && c == 99) || (b == 0 && c == 2)))
                || (a == 198 && ((18..=19).contains(&b) || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        std::net::IpAddr::V6(ip) => {
            // Reject IPv4-mapped and compatible addresses, including private
            // addresses embedded in an IPv6 URL.
            if ip.to_ipv4().is_some() { return false; }
            let segments = ip.segments();
            (segments[0] & 0xe000) == 0x2000
                && segments[0] != 0x2002  // 6to4 tunnels may encode private IPv4
                && !(segments[0] == 0x2001
                    && (segments[1] == 0 || segments[1] == 0x0db8))
        }
    }
}

pub(super) fn validated_http_target(url: &str) -> Result<(reqwest::Url, String, u16), String> {
    let parsed = reqwest::Url::parse(url).map_err(|error| error.to_string())?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("HTTP fetch accepts only public HTTP(S) URLs without credentials".into());
    }
    let host = parsed.host_str().ok_or("HTTP fetch URL requires a hostname")?
        .trim_matches(|ch| ch == '[' || ch == ']')
        .trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || host == "localhost"
        || [".localhost",".local",".internal",".test",".invalid"].iter().any(|suffix|host.ends_with(suffix)) {
        return Err("HTTP fetch cannot access local or internal hosts".into());
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if !public_fetch_ip(ip) { return Err("HTTP fetch cannot access non-public IP addresses".into()); }
    }
    let port=parsed.port_or_known_default().ok_or("HTTP fetch URL port is missing")?;
    Ok((parsed,host,port))
}

pub(super) async fn http_fetch(url: &str) -> Result<Value, String> {
    const MAX_REDIRECTS: usize = 3;
    let mut next=url.to_string();
    for hop in 0..=MAX_REDIRECTS {
        let (parsed, host, port) = validated_http_target(&next)?;
        let resolved = tokio::net::lookup_host((host.as_str(),port)).await
            .map_err(|error|format!("HTTP fetch DNS lookup failed: {error}"))?
            .collect::<Vec<_>>();
        if resolved.is_empty() || resolved.iter().any(|address|!public_fetch_ip(address.ip())) {
            return Err("HTTP fetch resolved to a local or non-public address".into());
        }
        // Explicitly review every redirect target with a new DNS lookup.
        // Pin the validated answers and ignore environment proxies.
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .resolve_to_addrs(&host, &resolved)
            .timeout(Duration::from_secs(30))
            .build().map_err(|error|error.to_string())?;
        let mut response = client.get(parsed.clone())
            .header("User-Agent", "DOMINION/0.2 (+rust)")
            .send().await.map_err(|error| error.to_string())?;
        if response.status().is_redirection() {
            if hop==MAX_REDIRECTS {
                return Err("HTTP fetch exceeded the redirect limit".into());
            }
            let location=response.headers().get(reqwest::header::LOCATION)
                .and_then(|value|value.to_str().ok())
                .ok_or("HTTP fetch redirect has no valid location")?;
            next=parsed.join(location).map_err(|error|
                format!("HTTP fetch redirect URL is invalid: {error}"))?.to_string();
            continue;
        }
        let status=response.status().as_u16();
        if response.content_length().is_some_and(|length|length>MAX_HTTP_FETCH_BYTES as u64) {
            return Err("HTTP fetch response exceeds the byte limit".into());
        }
        let content_type = response.headers().get("content-type")
            .and_then(|value|value.to_str().ok()).unwrap_or("").to_string();
        let mut bytes=Vec::new();
        while let Some(chunk)=response.chunk().await.map_err(|error|error.to_string())? {
            if chunk.len()>MAX_HTTP_FETCH_BYTES.saturating_sub(bytes.len()) {
                return Err("HTTP fetch response exceeds the byte limit".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        let body=String::from_utf8_lossy(&bytes);
        let text=if content_type.contains("html") {strip_html(&body)} else {body.into_owned()};
        return Ok(json!({
            "status":status,"contentType":content_type,
            "text":text.chars().take(60_000).collect::<String>(),
            "receivedBytes":bytes.len(),"redirectsFollowed":hop,
        }));
    }
    Err("HTTP fetch exceeded the redirect limit".into())
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

