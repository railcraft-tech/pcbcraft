# AI

`pcbcraft mcp` serves every command as a tool. The tool name is the command with underscores (`geometry_is_occupied`). Each call needs `cwd`, the absolute project directory. In Cursor, `.cursor/mcp.json` (or `~/.cursor/mcp.json`). In Claude Code, `.mcp.json`.

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

## Examples

### Finding free spot for a track or part placement

Ask for a free spot for a part or a track. The model calls `geometry_find_symbol` on a neighbour, then `geometry_is_occupied` on candidate points. `occupied: false` means the point is clear. `free` is the millimetres to the next blocker in each direction. Pass `layer: ["F.Cu"]` for a track. Omit `layer` when the question is whether a part body fits.


### Part substitute

Ask for a substitute. The model calls `parts_alternatives` with a spec. See [Parts](parts.md). After `components_sync`, `components_show` prints one catalog card already on the board, including its parameters.

### BOM

Ask what the board orders. `export_bom` is the JLCPCB bill. `components_show C12345` is printing information stored locally about the component without having to all the time ping the external API.

### Find a Symbol on the board

Ask where a part sits. `geometry_find_symbol` returns the bounding window and, per layer, the pads, nets, and outlines.

### Fix incorrect placements when placing an order to JLCPCB

Ask it to fix placement before the JLCPCB assembly order goes into the cart. Give it screenshots of the 2D preview. It writes `corrections.yaml`: the visible offset goes into `x` and `y` in millimetres (position-file axes, X right, Y up), and a wrong angle goes into `rotation` as whole degrees. Then `export_cpl` with `local` set to that file. See [Exporting](exporting.md).

Corrected CPL files upload again to the JLCPCB, take a look and verify if they are correct.
