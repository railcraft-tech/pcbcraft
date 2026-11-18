use std::time::Duration;

use anyhow::{Context, Result};

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
