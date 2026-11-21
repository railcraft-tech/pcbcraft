# SeriesPassives

[Tests](../tests.md)

Two resistors, or two capacitors, whose middle net has only those two pins. The outer nets must differ. Resistors report the sum. Capacitors report the series equivalent. In both cases the span runs from both parts at the low end of tolerance to both at the high end.

`expect` is optional per pair. A pair with no entry is printed and passes. An `expect` key that is not such a pair fails. The key is the two references joined by `/`, sorted by letter then number: `R1/R2` or `C1/C2`. A resistor window is ohms (`4.7k`). A capacitor window is capacitance (`100n`).

Run `pcbcraft components sync` first. `DNP` parts are skipped.

`pcbcraft tests init type SeriesPassives series` writes the skeleton.

```json
{
  "type": "SeriesPassives",
  "spec": {
    "project": "../../board.kicad_pro",
    "expect": {
      "R1/R2": { "min": "1.9k", "max": "2.1k" }
    }
  }
}
```

The line starts with that key, so a skip is `SeriesPassives.R1/R2`. No series pair fails the file.
