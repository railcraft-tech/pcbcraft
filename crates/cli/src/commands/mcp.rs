//! `pcbcraft mcp` speaks MCP on standard input and output.
//! Each tool call carries its own `cwd`. The process directory stays put.

use std::sync::Arc;

use anyhow::Result;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, JsonObject,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::Value;

use super::output::CommandOutput;
use super::tools;

pub fn serve() -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let running = Server.serve(rmcp::transport::io::stdio_pipes()).await?;
        running.waiting().await?;
        Ok(())
    })
}

struct Server;

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Each tool requires cwd, an absolute project directory. Relative paths and pcbcraft.yaml are resolved from that directory.",
        )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let listed = tools()
            .iter()
            .map(|tool| {
                rmcp::model::Tool::new(
                    tool.name,
                    tool.description,
                    Arc::new(object_schema(&tool.input_schema)),
                )
            })
            .collect();
        Ok(ListToolsResult::with_all_items(listed))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let Some(tool) = tools()
            .iter()
            .find(|tool| tool.name == request.name.as_ref())
        else {
            return Ok(tool_error(format!("unknown tool '{}'\n", request.name)).into());
        };
        let call = tool.call;
        let args = Value::Object(request.arguments.unwrap_or_default());
        let output = match tokio::task::spawn_blocking(move || call(&args)).await {
            Ok(output) => output,
            Err(err) => CommandOutput::error(1, format!("pcbcraft: {err}\n")),
        };
        Ok(tool_result(output).into())
    }
}

fn object_schema(value: &Value) -> JsonObject {
    match value.as_object() {
        Some(object) => object.clone(),
        None => JsonObject::new(),
    }
}

fn tool_result(output: CommandOutput) -> CallToolResult {
    let text = tool_text(&output);
    if output.code == 0 {
        CallToolResult::success(vec![ContentBlock::text(text)])
    } else {
        tool_error(text)
    }
}

fn tool_error(text: String) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(text)])
}

fn tool_text(output: &CommandOutput) -> String {
    let mut text = String::new();
    text.push_str(&output.stdout);
    if !output.stderr.is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&output.stderr);
    }
    if text.is_empty() && output.code != 0 {
        text = format!("exit {}\n", output.code);
    }
    text
}
