---
name: pcbcraft
description: >
  Runs the pcbcraft CLI for KiCad board exports, JLCPCB CPL, Gerbers,
  A4 drawings, ERC/DRC tests, LCSC and PCBWay alternatives, the project
  component database, and net dumps. Use when the user mentions BOM, CPL,
  Gerbers, JLCPCB, PCBWay, a board drawing, ERC, DRC, a part alternative,
  a net, or a pin dump.
---

# pcbcraft

Use the `pcbcraft` binary. Do not edit `.kicad_sch`, `.kicad_pcb`, or `.kicad_pro` to produce these outputs. Read numbers and violations from the command output.

## Export

- `pcbcraft export bom <project.kicad_pro>` writes a JLCPCB BOM.
- `pcbcraft export pos <project.kicad_pro>` writes the raw KiCad position CSV.
- `pcbcraft export cpl <project.kicad_pro> --local corrections.yaml` writes the five-column JLCPCB CPL. A second run on an already corrected file mirrors the bottom again. Pass a fresh KiCad position file, not the previous CPL.
- `pcbcraft export gerbers <project.kicad_pro> -o board.zip`
- `pcbcraft export drawing <project.kicad_pro>` puts side above back on one A4 page. `--side side` or `--side back` selects one face. `--full` asks for color.
- `pcbcraft export 2d <project.kicad_pro>` writes a square PNG of the orthographic 3D view. `--side front` or `--side back` selects the face and defaults to `front`. `--symbol J1` frames that footprint and picks the face from its geometry. `--around-space` defaults to 5 mm.
- `pcbcraft geometry find-symbol J1` prints the shapes of that one footprint.

`--local` points at a YAML file of the same shape as `longfred-v1a.yaml`.

## Tests

`pcbcraft tests run` reads `tests/pcbcraft/**/*.json`. `pcbcraft tests run <directory>` selects another directory. `pcbcraft tests run --name erc-test` runs that one file, including its subtests. The name has no extension and is the path relative to the test directory. `pcbcraft tests init defaults` creates the directory and the default `erc-test.json`, `drc-test.json`, and `availability-test.json` when they are missing. The generated availability file sets `defaultMinimum` to 10. `pcbcraft tests init type ERCValidator my-erc` writes `my-erc.json`. The type and the file name, without an extension, are both required. `pcbcraft tests list` prints the tests in that directory, then the types the program can run. `pcbcraft.yaml` in the current directory may set `tests.skipped` to line ids such as `VoltageRating.C21`. A skipped line does not fail the run. `PartAvailability` fails a part when its stock is below `defaultMinimum`, which defaults to 100 when the field is omitted. `part` maps a catalog number to its own minimum. Each part is printed as `PartAvailability.{part number}`. Each file is:

```json
{
  "type": "ERCValidator",
  "spec": { "project": "../board.kicad_pro" }
}
```

`DRCValidator` uses the same `spec`. Paths are relative to the JSON file. Exit code 2 means the schema blocked the run. Exit code 1 means a test failed.

These types are not created by `tests init defaults`. `tests init type` writes one of them. `Attributes` checks mounting holes and fiducials: `in_bom` and `in_pos_files` must be no, and the LCSC and PCBWay fields must be empty. Each one is its own file:

- `CatalogValue` compares each `R`, `C`, and `L` value on the schematic with the LCSC card, inside the catalog tolerance.
- `ResistorPower` checks a resistor between two declared rails against its power rating. `powerHeadroom` defaults to 2.
- `VoltageRating` checks a capacitor, or a resistor that publishes a voltage rating, between two declared rails. `voltageHeadroom` defaults to 2.
- `Divider` reports the output of two resistors between two rails. `expect` maps the output net to a voltage window.
- `TimeConstant` reports `τ` for one resistor and one capacitor across the same two nets. `expect` uses times such as `1ms`.
- `ParallelPassives` reports the equivalent of resistors that share both nets.
- `SeriesPassives` reports the equivalent of two resistors or two capacitors whose middle net has only those two pins.

Rails are objects with string `min` and `max`, both required, for example `"3.3V"`. A bare number is a schema error. Run `pcbcraft components sync` before these tests.

## Parts

`supplier` is required on every item. LCSC parameters and PCBWay parameters are different structs. A key from one supplier is an error on the other. See `reference.md`.

`pcbcraft parts alternatives spec.yaml`

`pcbcraft components sync <project.kicad_pro>` writes used LCSC and PCBWay parts, including electrical parameters, to `.pcbcraft/components.sqlite3`. It uses the same supplier catalog as the alternative search. `--refresh` fetches every number again. `pcbcraft components show C12345` prints that card, including every stored attribute, as JSON.

## Nets

`pcbcraft net GND dump` prints track widths, a text graph, and the pin-to-pin list. The graph is cached in `.pcbcraft/net.sqlite` and rebuilt when a schematic or the board changes. The same net name can appear as more than one star.
