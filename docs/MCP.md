# MCP

`pcbcraft mcp` speaks the Model Context Protocol on standard input and output. Each CLI command is a tool with the same arguments. The tool name is the command path with spaces replaced by underscores, for example `export_bom` and `tests_run`. `pcbcraft mcp` itself is not a tool.

Each tool requires `cwd`, an absolute project directory. Relative paths and `pcbcraft.yaml` are resolved from that directory, and the server does not change its own directory between calls. JSON keys use underscores where the flag uses a hyphen (`around_space`, `include_out_of_stock`). An omitted field means the same thing as an omitted flag.

`parts_search` is `pcbcraft parts search`. The CLI reads a JSON attribute object from stdin or from the `params` file. The tool takes that object as `attributes`. `param` is the list of `key=value` entries, the same as repeating `-p`.

`parts_download_model` is `pcbcraft parts download-model`. `lcsc` is the list of LCSC numbers. `library` and `project` are optional paths relative to `cwd`.

`parts_download_datasheet` is `pcbcraft parts download-datasheet`. `supplier` is `lcsc`, `part` is the LCSC number, and `output_dir` is the directory for `datasheet.md`. Relative paths are resolved from `cwd`.

`tests_single_run` is `pcbcraft tests single-run TYPE`. The CLI reads the test spec JSON from stdin. The tool takes that object as `spec`. A path in `spec.project` is relative to `cwd`. The command does not write a test file.

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
