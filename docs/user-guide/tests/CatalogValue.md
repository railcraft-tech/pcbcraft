# CatalogValue

[Tests](../tests.md)

The schematic value of each `R`, `C`, and `L` must fall inside the LCSC card: nominal ± the catalog tolerance. Run `pcbcraft components sync` first. A part with no LCSC number, no card in `.pcbcraft/components.sqlite3`, no catalog value, or no tolerance fails on its own line. `DNP` parts are skipped. A reference such as `RN2` is not a two-terminal part and is ignored. A bare resistor value such as `22` is ohms.

`pcbcraft tests init type CatalogValue values` writes the file. The spec is only the project.

```json
{
  "type": "CatalogValue",
  "spec": { "project": "../../board.kicad_pro" }
}
```

One line per part, named by its reference. A board with no such parts fails. A skip is `CatalogValue.R12`.
