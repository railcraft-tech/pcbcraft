# VoltageRating

[Tests](../tests.md)

A part between two declared rails against its voltage rating. Every capacitor on two rails is checked. A resistor or an inductor is checked when the LCSC card publishes a voltage rating. The applied voltage is the widest gap between the two rail windows. It fails when that voltage is above `rating / voltageHeadroom`. `voltageHeadroom` defaults to 2 and must be at least 1.

Run `pcbcraft components sync` first. `DNP` parts are skipped.

`pcbcraft tests init type VoltageRating voltage` writes a skeleton with an empty `rails` object. Name at least one rail. Both `min` and `max` are strings with a unit.

```json
{
  "type": "VoltageRating",
  "spec": {
    "project": "../../board.kicad_pro",
    "rails": {
      "+3V3": { "min": "3.0V", "max": "3.6V" },
      "GND": { "min": "0V", "max": "0V" }
    },
    "voltageHeadroom": 2
  }
}
```

The line starts with the reference, so a skip is `VoltageRating.C21`. No rated part between the declared rails fails the file.
