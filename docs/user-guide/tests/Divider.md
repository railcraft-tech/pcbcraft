# Divider

[Tests](../tests.md)

Two resistors that meet on one net, each with its other pin on a different declared rail. The reported voltage is that output swept across both rail windows and both tolerance bands. `expect` maps the output net to a window the whole span must fit inside. A divider with no `expect` entry is printed and passes. An `expect` key that is not such an output fails.

Run `pcbcraft components sync` first. `DNP` parts are skipped. Rails need at least one net. Both `min` and `max` are strings with a unit, including the expected volts.

`pcbcraft tests init type Divider divider` writes the skeleton.

```json
{
  "type": "Divider",
  "spec": {
    "project": "../../board.kicad_pro",
    "rails": {
      "+3V3": { "min": "3.2V", "max": "3.4V" },
      "GND": { "min": "0V", "max": "0V" }
    },
    "expect": {
      "VREF": { "min": "1.6V", "max": "1.7V" }
    }
  }
}
```

The line starts with the two references, so a skip is `Divider.R1/R2`. No divider on the declared rails fails the file.
