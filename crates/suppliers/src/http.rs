use std::io::Read;
use std::time::Duration;

use anyhow::{Context, Result};

const BROWSER: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

pub struct HttpBody {
    pub bytes: Vec<u8>,
    pub content_type: String,
}

pub fn post_json(url: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build();
    let response = agent
        .post(url)
        .set("Content-Type", "application/json")
        .set("Accept", "application/json")
        .set("User-Agent", "pcbcraft/0.1.0")
        .send_string(&body.to_string())
        .map_err(|err| anyhow::anyhow!("{url}: {err}"))?;
    let text = response
        .into_string()
        .with_context(|| format!("response {url}"))?;
    Ok(serde_json::from_str(&text).with_context(|| format!("JSON {url}"))?)
}

pub fn get_json(url: &str) -> Result<serde_json::Value> {
    let body = get_bytes(url, Duration::from_secs(60))?;
    serde_json::from_slice(&body.bytes).with_context(|| format!("JSON {url}"))
}

/// File download. `Accept-Encoding: identity` keeps the body as the server stored it;
/// this crate's ureq build does not decode gzip.
pub fn get_bytes(url: &str, timeout: Duration) -> Result<HttpBody> {
    let agent = ureq::AgentBuilder::new().timeout(timeout).build();
    let response = agent
        .get(url)
        .set("User-Agent", BROWSER)
        .set("Accept", "*/*")
        .set("Accept-Encoding", "identity")
        .call()
        .map_err(|err| anyhow::anyhow!("{url}: {err}"))?;
    let content_type = response.header("content-type").unwrap_or("").to_string();
    let mut bytes = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .with_context(|| format!("response {url}"))?;
    Ok(HttpBody {
        bytes,
        content_type,
    })
}
