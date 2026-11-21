# TimeConstant

[Tests](../tests.md)

One resistor and one capacitor that share both nets, with nothing else across that pair. τ is `R × C`, from the low ends of both tolerances to the high ends. `expect` maps the pair to a time window the whole span must fit inside. A pair with no `expect` entry is printed and passes. An `expect` key that is not such a pair fails.

The key is the two references joined by `/`, sorted by letter then number: `C1` and `R12` is `C1/R12`. A time is `1s`, `1ms`, `1us`, or `1ns`.

Run `pcbcraft components sync` first. `DNP` parts are skipped.

`pcbcraft tests init type TimeConstant tau` writes the skeleton.

```json
{
  "type": "TimeConstant",
  "spec": {
    "project": "../../board.kicad_pro",
    "expect": {
      "C1/R12": { "min": "0.9ms", "max": "1.1ms" }
    }
  }
}
```

The line starts with that same key, so a skip is `TimeConstant.C1/R12`. No such pair fails the file.
