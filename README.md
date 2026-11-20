# pcbcraft

Command-line helper for KiCad boards: manufacturing files, checks, supplier lookup, and net inspection. It talks to KiCad only through `kicad-cli` and does not rewrite schematic or board files.

## Commands

```text
pcbcraft export bom <project.kicad_pro>
pcbcraft export pos <project.kicad_pro>
pcbcraft export cpl <project.kicad_pro> --local corrections.yaml
pcbcraft export gerbers <project.kicad_pro> -o board.zip
pcbcraft export drawing <project.kicad_pro>
pcbcraft export 2d <project.kicad_pro> --symbol J1 --around-space 2mm
pcbcraft geometry find-symbol J1
pcbcraft tests run
pcbcraft parts alternatives spec.yaml
pcbcraft components sync <project.kicad_pro>
pcbcraft components show C12345
pcbcraft net GND dump
pcbcraft mcp
```

`export bom` writes a JLCPCB-shaped bill of materials. `export pos` writes the raw KiCad position CSV. `export cpl` corrects rotation and offset, then writes the five JLCPCB columns. Run CPL on a fresh position file: a second pass mirrors the bottom again.

`export drawing` puts the front above the back on one landscape A4 page at 1:1. `--side side` or `--side back` selects one face. `--full` adds color.

`export 2d` writes a square PNG of the orthographic 3D view. `--side` is `front` or `back` and defaults to `front`. `--symbol J1` frames that footprint and picks the face from its geometry. `--around-space` defaults to 5 mm. `geometry find-symbol J1` prints the shapes of that one footprint.

`tests run` reads `tests/pcbcraft/**/*.json`. Each file names a validator (`ERCValidator`, `DRCValidator`, or `PartAvailability`) and a `spec`. Pass another directory as `pcbcraft tests run <dir>`. `pcbcraft tests run --name erc-test` runs that one file and still runs its subtests. The name is the file path relative to the test directory, without `.json`. `pcbcraft tests init defaults` creates that directory, if it is missing, with `erc-test.json`, `drc-test.json`, and `availability-test.json`. The generated availability test sets `defaultMinimum` to 10. `pcbcraft tests init type ERCValidator my-check` writes `my-check.json` for that type. Both the type and the file name are required. `pcbcraft tests list` prints the tests in the directory, then the types the program can run. `pcbcraft.yaml` in the current directory can list lines to skip under `tests.skipped`, for example `VoltageRating.C21`. A skipped line is printed and does not fail the run.

Further checks are separate JSON types and are not created by `tests init defaults`: `Attributes`, `CatalogValue`, `ResistorPower`, `VoltageRating`, `Divider`, `TimeConstant`, `ParallelPassives`, and `SeriesPassives`. `tests init type` can write them. `Attributes` requires a mounting hole or fiducial to stay out of the BOM and the position file, and to have no LCSC or PCBWay number. The electrical checks read the net graph and `.pcbcraft/components.sqlite3`. Rail voltages are declared in the test file as a min/max pair.

`parts alternatives` searches LCSC or PCBWay from a YAML file. Each item sets `supplier`, and the parameters for one supplier are not accepted by the other. `components sync` stores the parts used on the schematic, including electrical parameters, in `.pcbcraft/components.sqlite3`. `components show C12345` prints that card as JSON.

`net GND dump` prints track widths, a text graph, and the pin-to-pin list. The graph is cached in `.pcbcraft/net.sqlite` and rebuilt when a source file changes.

`pcbcraft mcp` serves these commands over MCP. See [docs/MCP.md](docs/MCP.md).

Set `KICAD_CLI` if `kicad-cli` is not on `PATH`.

## Build and test

```text
make
make test
```

`make` builds the workspace. `make test` runs the unit tests in every crate. Pushes and pull requests run the same tests in GitHub Actions. A tag `v*` also publishes release binaries for Linux (x86_64 and aarch64), macOS (aarch64), and Windows (x86_64).

A skill for agents that call this tool lives in `dist/.skills/pcbcraft`. Copy that directory into another project's `.cursor/skills/pcbcraft`.
