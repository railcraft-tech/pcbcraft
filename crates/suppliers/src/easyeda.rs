//! Public EasyEDA component drawings. The Pro API is not used.

use std::io::Read;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use pcbcraft_application::ports::CadDocument;
use serde_json::{json, Value};

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

#[derive(Debug, Default, Clone, Copy)]
pub struct EasyedaClient;

impl EasyedaClient {
    pub fn fetch_component(&self, lcsc: &str) -> Result<CadDocument> {
        let url = format!("https://easyeda.com/api/products/{lcsc}/components");
        match get_text(&url)? {
            None => Ok(CadDocument::Missing),
            Some(text) => interpret_component(&text),
        }
    }

    pub fn fetch_step(&self, uuid: &str) -> Result<Option<Vec<u8>>> {
        let url = format!("https://modules.easyeda.com/qAxj6KHrDKw4blvCG8QJPs7Y/{uuid}");
        Ok(get_bytes(&url)?.filter(|bytes| is_step(bytes)))
    }

    pub fn fetch_obj(&self, uuid: &str) -> Result<Option<String>> {
        let url = format!("https://modules.easyeda.com/3dmodel/{uuid}");
        Ok(get_text(&url)?.filter(|text| looks_like_obj(text)))
    }
}

pub fn is_step(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(512)];
    String::from_utf8_lossy(head).contains("ISO-10303-21")
}

fn looks_like_obj(text: &str) -> bool {
    text.lines()
        .any(|line| line.split_whitespace().next() == Some("v"))
}

impl pcbcraft_application::ports::EasyedaCad for EasyedaClient {
    fn component(&self, lcsc: &str) -> Result<CadDocument> {
        self.fetch_component(lcsc)
    }

    fn step(&self, uuid: &str) -> Result<Option<Vec<u8>>> {
        self.fetch_step(uuid)
    }

    fn obj(&self, uuid: &str) -> Result<Option<String>> {
        self.fetch_obj(uuid)
    }
}

fn interpret_component(text: &str) -> Result<CadDocument> {
    let value: Value = serde_json::from_str(text).context("EasyEDA response was not JSON")?;
    match value.get("success") {
        Some(Value::Bool(true)) => match value.get("result") {
            Some(result) if result.is_object() => Ok(CadDocument::Found(result.clone())),
            _ => Ok(CadDocument::Missing),
        },
        Some(Value::Bool(false)) => {
            let missing = value
                .get("code")
                .is_some_and(|code| code == &json!(404) || code.as_str() == Some("404"));
            if missing {
                Ok(CadDocument::Missing)
            } else {
                let message = value
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("EasyEDA refused the component");
                Err(anyhow!("{message}"))
            }
        }
        _ => Err(anyhow!("EasyEDA response has no success flag")),
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build()
}

fn get_text(url: &str) -> Result<Option<String>> {
    match request(url)?.map(|response| response.into_string()) {
        None => Ok(None),
        Some(text) => Ok(Some(text.with_context(|| format!("response {url}"))?)),
    }
}

fn get_bytes(url: &str) -> Result<Option<Vec<u8>>> {
    let Some(response) = request(url)? else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .with_context(|| format!("response {url}"))?;
    Ok(Some(bytes))
}

fn request(url: &str) -> Result<Option<ureq::Response>> {
    match agent()
        .get(url)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/json, text/javascript, */*; q=0.01")
        .set("Referer", "https://easyeda.com/")
        .set("Accept-Encoding", "identity")
        .call()
    {
        Ok(response) => Ok(Some(response)),
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(ureq::Error::Status(code, _)) => Err(anyhow!("EasyEDA request failed: HTTP {code}")),
        Err(err) => Err(anyhow!("EasyEDA request failed: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_file_starts_with_the_iso_header_and_a_page_does_not() {
        assert!(is_step(b"ISO-10303-21;\nHEADER;\n"));
        assert!(!is_step(b"<html>not a model</html>"));
    }

    #[test]
    fn a_not_found_body_is_a_missing_component() {
        let body = r#"{"success":false,"code":404,"message":"Component not found"}"#;
        assert!(matches!(
            interpret_component(body).unwrap(),
            CadDocument::Missing
        ));
    }

    #[test]
    fn an_obj_without_vertices_is_rejected() {
        assert!(!looks_like_obj("<html></html>"));
        assert!(looks_like_obj("v 0 0 0\nf 1 2 3\n"));
    }
}
