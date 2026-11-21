# ResistorPower

[Tests](../tests.md)

A resistor whose two pins sit on declared rails. The used power is `Vmax² / Rmin`: the widest gap between the two rail windows, and the low end of the catalog tolerance. It fails when that power is above `rating / powerHeadroom`. `powerHeadroom` defaults to 2 and must be at least 1.

Run `pcbcraft components sync` first. The card must carry resistance, tolerance, and a power rating. `DNP` parts are skipped. A resistor that does not touch two declared rails is not a line.

`pcbcraft tests init type ResistorPower power` writes a skeleton with an empty `rails` object. Name at least one rail. Both `min` and `max` are strings with a unit.

```json
{
  "type": "ResistorPower",
  "spec": {
    "project": "../../board.kicad_pro",
    "rails": {
      "+3V3": { "min": "3.0V", "max": "3.6V" },
      "GND": { "min": "0V", "max": "0V" }
    },
    "powerHeadroom": 2
  }
}
```

The line starts with the reference, so a skip is `ResistorPower.R1`. No resistor between the declared rails fails the file.
