# pcbcraft

Tooling to speedup human & AI work on PCB design & PCB ordering. Use it by yourself with the CLI, or handover to AI with a MCP.

With this tool AI can burn multiple times less tokens, as some part of PCB analysis and parts search is done by `pcbcraft`.
You can ask e.g. `do I have space to put a second resistor next to R24?`, instead of AI analyzing your PCB geometry again it calls `pcbcraft geometry find-symbol R24` and `pcbcraft geometry is-occupied` and analyzing its output which is limited to nearby area with already computed values.

Before the order you can ask if all components are accessible on the supplier side, to find a compatible replacement for given component.
Connecting with GitHub Actions you can monitor components availability more frequently, e.g. weekly or on code push.

**Core features:**
- PCB unit tests with `pcbcraft tests run`
- Alternative parts search, matching by min-max, and list of parameters
- Fetching BOM from external APIs (e.g. JLCPCB/LCSC) for quick parameters inspection
- NETs analysis
- Finding components on the board, describing, taking pictures of single components
- PCB geometry support, a complete PCB map rendered by `pcbcraft`, with collisions detection - `pcbcraft geometry is-occupied --x X --y Y` with showing also nearby space
- Exporting to JLCPCB with correcting positions and rotations in CPL file (KiCad by default exports positions that are wrongly interpreted by JLCPCB and needs to be corrected). Partially the positions are corrected automatically, the rest you can adjust with a YAML file
- Integrated MCP
- JLCPCB DFM check made locally as part of a unit test to better prepare board for the order
- CLI can be embedded as a GitHub Actions step for constantly checking your board for breaking changes, missing components on the supplier side

## Commands

Here is an example list of commands accessible from the CLI.

```text
pcbcraft export bom <project.kicad_pro>
pcbcraft export pos <project.kicad_pro>
pcbcraft export cpl <project.kicad_pro> --local corrections.yaml
pcbcraft export gerbers <project.kicad_pro> -o board.zip
pcbcraft export drawing <project.kicad_pro>
pcbcraft export 2d <project.kicad_pro> --symbol J1 --around-space 2mm
pcbcraft geometry find-symbol J1
pcbcraft geometry is-occupied
pcbcraft geometry dump
pcbcraft tests run
pcbcraft parts alternatives spec.yaml
pcbcraft components sync <project.kicad_pro>
pcbcraft components show C12345
pcbcraft net GND dump
pcbcraft mcp
```

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
