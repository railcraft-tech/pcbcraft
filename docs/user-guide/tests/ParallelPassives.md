# ParallelPassives

[Tests](../tests.md)

Two or more resistors that share both nets. The line reports their parallel equivalent, from all of them at the low end of tolerance to all of them at the high end. `expect` maps that group to an ohm window the whole span must fit inside. A group with no `expect` entry is printed and passes. An `expect` key that is not such a group fails.

The key is the references joined by `/`, sorted by letter then number: `R1/R2/R10`. Both `min` and `max` are resistances, such as `4.7k`.

Run `pcbcraft components sync` first. `DNP` parts are skipped.

`pcbcraft tests init type ParallelPassives parallel` writes the skeleton.

```json
{
  "type": "ParallelPassives",
  "spec": {
    "project": "../../board.kicad_pro",
    "expect": {
      "R1/R2": { "min": "4.7k", "max": "5.3k" }
    }
  }
}
```

The line starts with that key, so a skip is `ParallelPassives.R1/R2`. No parallel group fails the file.
