# Board setup

`pcbcraft board setup` raises the KiCad design rules to a fabricator's process limits, so the interactive DRC flags what that fabricator's DFM would reject. A value that is already tighter stays. Net-class patterns, colors, and the rest of the board file stay.

```text
pcbcraft board setup jlcpcb [board.kicad_pro]
pcbcraft board setup pcbway
pcbcraft board setup custom profile.json
```

Without a project file, the only `.kicad_pro` in the current directory is used. `--mask` selects the solder-mask row and defaults to `green`. The color is not stored on the board.

JLCPCB and PCBWay are built in. `custom` reads a JSON file with the same fields. `tracks` is required. Every other field is optional: a missing one leaves that KiCad check alone. Lengths are millimetres. The row is chosen from the board's copper-layer count and the thicker outer copper. A weight a little above a published step still uses that step. A weight past every step uses the heaviest one. An unknown field, a name other than letters, digits, and a hyphen, or no row for this board exits with code 2 and writes nothing.

```json
{
  "name": "acme",
  "tracks": [
    { "minLayers": 1, "maxLayers": 2, "minOz": 1, "widthMm": 0.1, "spaceMm": 0.1 }
  ],
  "annular": [
    { "minLayers": 1, "maxLayers": 2, "minOz": 1, "failMm": 0.18, "warnMm": 0.25 }
  ],
  "via": { "drillFailMm": 0.15, "drillWarnMm": 0.2, "extraFailMm": 0.1, "extraWarnMm": 0.15 },
  "pth": { "drillFailMm": 0.15 },
  "npth": { "drillFailMm": 0.5, "annularWarnMm": 0.45 },
  "holeSpacing": { "viaMm": 0.2, "pthMm": 0.45 },
  "holeToCopper": {
    "viaMm": 0.2,
    "npthMm": 0.2,
    "pthOuterFailMm": 0.28,
    "pthOuterWarnMm": 0.35,
    "pthInnerMm": 0.3
  },
  "copperToEdgeMm": 0.2,
  "mask": [
    { "colors": ["green"], "bridgeMm": 0.1, "toCopperMm": 0.09 }
  ],
  "smd": [
    { "maxOz": 2, "padSpacingMm": 0.15, "padToTrackMm": 0.1 }
  ],
  "silk": { "strokeMm": 0.15, "heightMm": 1, "toPadMm": 0.15 }
}
```

`name` is written into the custom rules as `pcbcraft acme …`. Running the command again replaces every rule whose name starts with `pcbcraft `, including a previous fabricator, and leaves the other rules in `.kicad_dru` as they were.

The command writes `.kicad_pro`, `.kicad_dru`, and the solder-mask lines in the board's `(setup …)`. It is the only pcbcraft command that edits those files. After writing, it copies the three files and asks `kicad-cli pcb drc` to load the copy. Ordinary DRC violations are not a failure. If the board does not load, or KiCad ignores the project rules or the custom rules, the three files are restored from the command's own backup and the command exits with code 1. Reload the board in KiCad afterwards. An open KiCad will overwrite the files when it saves.

A second run that finds the floors already in place changes nothing.
