# PartAvailability

[Tests](../tests.md)

Stock of every LCSC and PCBWay number on the schematic. A quantity below the minimum fails. A number with no stock figure fails. A lookup error fails. A schematic with no catalog numbers fails the file.

`pcbcraft tests init defaults` writes `availability-test.json` with `defaultMinimum` set to 10. Omitting that field later means 100. `part` maps one catalog number to its own minimum.

```json
{
  "type": "PartAvailability",
  "spec": {
    "project": "../../board.kicad_pro",
    "defaultMinimum": 10,
    "part": { "C12345": 50 }
  }
}
```

One line per catalog number. The same number on both suppliers is printed as `lcsc:C12345` and `pcbway:C12345`. A skip is `PartAvailability.C12345`.
