# Parts

`pcbcraft parts alternatives spec.yaml` searches LCSC or PCBWay. Every item needs `supplier`. The two suppliers use different parameter keys.

```yaml
items:
  - name: R12
    supplier: lcsc
    parameters:
      category: resistor
      package: "0805"
      resistance: { min: "9.9k", max: "10.1k" }
      tolerance: { min: "0.1%", max: "1%" }
  - name: U1
    supplier: pcbway
    parameters:
      mpn: ["STM32F103C8T6"]
      package: ["LQFP-48"]
```

LCSC requires `category`. Any other LCSC parameter is a `min`/`max` pair or a list of accepted strings. A bare number is rejected. PCBWay requires an `mpn` list. `manufacturer`, `package`, and `description` are optional lists.

`--limit` defaults to 10. `--include-out-of-stock` keeps empty stock. `--basic` keeps LCSC basic parts only. `--format yaml` prints the matches as YAML. The default is one line per hit: supplier, id, mpn, package, stock.

## Search

`pcbcraft parts search --supplier jlcpcb --query resistor` searches the JLCPCB assembly catalog. Part numbers in the JSON array are LCSC codes.

Product attributes are a JSON object. Pass the file as the last argument, pipe it on stdin, or repeat `-p key=value`. A list of values for one key is OR. Different keys are AND. `-p` adds to the object.

```text
pcbcraft parts search --supplier jlcpcb --query resistor < params.json
pcbcraft parts search --supplier jlcpcb --query resistor params.json
pcbcraft parts search --supplier jlcpcb --query resistor -p package=0805 -p Resistance=10kΩ -p Tolerance=±1%
```

```json
{
  "package": "0805",
  "manufacturer": "UNI-ROYAL(Uniroyal Elec)",
  "Resistance": "10kΩ",
  "Tolerance": ["±1%", "±5%"]
}
```

`package` and `manufacturer` filter those catalog fields. Every other key is a product attribute. The name and the value are the catalog's own spelling, the same strings `components show` prints. `10kΩ` is not `10k` or `10000`.

`--limit` defaults to 10 and must be from 1 to 50. `--include-out-of-stock` keeps empty stock. `--basic` keeps basic parts only.

`pcbcraft mcp` exposes this command as `parts_search`. There is no stdin on that tool. Pass the object as `attributes`, the file as `params`, and each `-p` value in `param`.

## Download a footprint

`pcbcraft parts download-model C2040` saves the EasyEDA footprint to `.pcbcraft/EasyEDA.pretty` and, when EasyEDA publishes one, the STEP model to `.pcbcraft/EasyEDA.3dshapes`. Several LCSC numbers can be given. A missing 3D model still saves the footprint. The report uses one word per number: `model` when both files were written, `footprint` when only the footprint was written, and `failed` when nothing was saved. One failure does not stop the rest. The command exits 0 when every number has a footprint.

`--library` chooses the parent directory of the two library folders. `--project board.kicad_pro` registers the footprint library in that project's `fp-lib-table`. Without `--project`, the report prints the library entry to paste into the table. EasyEDA Pro footprints are not converted.

`pcbcraft mcp` exposes this command as `parts_download_model`. `lcsc` is the list of numbers. `library` and `project` are optional paths relative to `cwd`.

## Download a datasheet

`pcbcraft parts download-datasheet --supplier lcsc C55266 --output-dir=datasheets/C55266` reads the LCSC product page and writes into that directory. It does not add a part-number subdirectory. `--supplier` accepts `lcsc`. The part number is `C` plus digits; letter case does not matter.

```text
datasheets/C55266/
  datasheet.pdf
  datasheet.md
  images/
  extra/
```

`datasheet.md` is the datasheet with a short header (LCSC number, manufacturer, source URL, SHA-256, page count). Figures from the PDF are PNG files in `images/`, and the markdown links them as `images/…`. Product photos, RoHS files, and the EasyEDA symbol and footprint drawings go in `extra/` and are not converted. The blank LCSC photo is skipped. A second run replaces `datasheet.pdf`, `datasheet.md`, `images/`, and `extra/`, and leaves every other file in the directory alone.

The conversion runs PyMuPDF in a separate Python process. PyMuPDF 2.1 is enough (`pip install 'pymupdf==2.1'`). PyMuPDF 1.x also works with `pymupdf4llm` older than 2 (`pip install 'pymupdf4llm<2'`); the 2.x line of that package does not serve a 1.x install. A scanned datasheet with no text layer still saves its figures and prints a warning. An image inside a table cell is placed under the table; the words and numbers stay in the markdown.

`pcbcraft mcp` exposes this command as `parts_download_datasheet`. `output_dir` is relative to `cwd`.
