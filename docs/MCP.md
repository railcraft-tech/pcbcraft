# MCP

`pcbcraft mcp` speaks the Model Context Protocol on standard input and output. Each CLI command is a tool with the same arguments. The tool name is the command path with spaces replaced by underscores, for example `export_bom` and `tests_run`. `pcbcraft mcp` itself is not a tool.

Each tool requires `cwd`, an absolute project directory. Relative paths and `pcbcraft.yaml` are resolved from that directory, and the server does not change its own directory between calls. JSON keys use underscores where the flag uses a hyphen (`around_space`, `include_out_of_stock`). An omitted field means the same thing as an omitted flag.

## Cursor

In the project, `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "pcbcraft": {
      "command": "pcbcraft",
      "args": ["mcp"]
    }
  }
}
```

The same entry in `~/.cursor/mcp.json` enables the server for every project.

## Claude Code

In the project, `.mcp.json`:

```json
{
  "mcpServers": {
    "pcbcraft": {
      "command": "pcbcraft",
      "args": ["mcp"]
    }
  }
}
```

From a terminal:

```text
claude mcp add --transport stdio pcbcraft -- pcbcraft mcp
```
