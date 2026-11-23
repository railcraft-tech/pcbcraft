---
name: pcbcraft
description: >
  Runs the pcbcraft CLI for KiCad board exports, JLCPCB CPL, Gerbers,
  A4 drawings, ERC/DRC tests, LCSC and PCBWay alternatives, JLCPCB catalog
  search, LCSC datasheets, the project component database, net dumps, and
  KiCad board-setup profiles for JLCPCB, PCBWay, or a custom JSON file.
  Use when the user mentions BOM, CPL, Gerbers, JLCPCB, PCBWay, a board
  drawing, ERC, DRC, board setup, a fabrication profile, a part alternative,
  a part search, a datasheet, a net, or a pin dump.
---

# pcbcraft

Use the `pcbcraft` binary. Do not edit `.kicad_sch`, `.kicad_pcb`, or `.kicad_pro` to produce these outputs. `pcbcraft board setup` is the exception: it writes the project rules, `.kicad_dru`, and the solder-mask lines in the board setup, then reloads are required. Read numbers and violations from the command output. MCP tools from `pcbcraft mcp` are the same commands. Each one requires `cwd`, an absolute project directory.

## Board setup

`pcbcraft board setup jlcpcb [board.kicad_pro]` and `pcbcraft board setup pcbway` raise KiCad's design rules to that fabricator's floors. Values that are already tighter stay. `pcbcraft board setup custom profile.json` reads a JSON profile with the same fields; `tracks` is required and `name` may contain letters, digits, and a hyphen. `--mask` defaults to `green`. The MCP tool is `board_setup`. Pass `profile` as `jlcpcb`, `pcbway`, or `custom`, and pass `file` when the profile is `custom`.

The command checks the written files with `kicad-cli`. If the board or the rules do not load, it restores the three files from its own backup. Reload the board afterwards. An open KiCad overwrites the files on save.

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

`pcbcraft tests single-run ERCValidator` reads the spec object from stdin and does not write a file. `project` is relative to the current directory. The MCP tool is `tests_single_run`. Pass the object as `spec`. There is no stdin on the tool.

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

`pcbcraft parts search --supplier jlcpcb --query resistor` searches the JLCPCB assembly catalog and prints a JSON array of LCSC parts. Product attributes are a JSON object on stdin, a JSON file argument, or repeated `-p key=value`. `package` and `manufacturer` filter those fields. Any other key is a catalog attribute in the catalog's own spelling, for example `Resistance=10kΩ` and `Tolerance=±1%`. A list, or a repeated key, is OR. Different keys are AND. `--limit` defaults to 10 and is at most 50. The MCP tool is `parts_search`. Pass the object as `attributes` and the `-p` entries as `param`. There is no stdin on the tool.

`pcbcraft parts download-model C2040` saves the EasyEDA footprint under `.pcbcraft/EasyEDA.pretty` and the STEP model, when EasyEDA has one, under `.pcbcraft/EasyEDA.3dshapes`. A missing model still saves the footprint. `--library` changes the parent directory. `--project board.kicad_pro` registers the library in that project's `fp-lib-table`. The MCP tool is `parts_download_model`, with `lcsc` as the list of numbers.

`pcbcraft parts download-datasheet --supplier lcsc C55266 --output-dir=datasheets/C55266` writes `datasheet.md`, `datasheet.pdf`, figures under `images/`, and the other LCSC files under `extra/`. The markdown links figures as `images/…`. PyMuPDF 2.1 is enough (`pip install 'pymupdf==2.1'`). PyMuPDF 1.x needs `pymupdf4llm` older than 2 (`pip install 'pymupdf4llm<2'`). The MCP tool is `parts_download_datasheet`.

`pcbcraft components sync <project.kicad_pro>` writes used LCSC and PCBWay parts, including electrical parameters, to `.pcbcraft/components.sqlite3`. It uses the same supplier catalog as the alternative search. `--refresh` fetches every number again. `pcbcraft components show C12345` prints that card, including every stored attribute, as JSON.

## Nets

`pcbcraft net GND dump` prints track widths, a text graph, and the pin-to-pin list. The graph is cached in `.pcbcraft/net.sqlite` and rebuilt when a schematic or the board changes. The same net name can appear as more than one star.
