# JLCPCB DFM

`pcbcraft tests init type JLCPCBDFM jlcpcb-dfm` writes `tests/pcbcraft/jlcpcb-dfm.json`. Run it with the other tests, or alone with `pcbcraft tests run --name jlcpcb-dfm`.

```json
{
  "type": "JLCPCBDFM",
  "spec": {
    "project": "../../board.kicad_pro",
    "maskColor": "green",
    "rails": ["bottom"],
    "allowOverhang": ["J1"]
  }
}
```

`maskColor` is `green`, `red`, `yellow`, `blue`, `white`, `black`, or `purple`. Black and white raise the solder-mask bridge from 0.10 mm to 0.13 mm. `rails` names edges (`top`, `bottom`, `left`, `right`) where a body closer than 2.5 mm is a warning. The same gap on another edge fails. `allowOverhang` lists references that may cross the outline. `outerCopperOz` and `innerCopperOz` override the copper weight read from the board. Outer copper of 1.5 oz and above uses a 0.20 mm mask bridge and raises the track and annular minimums.

A value under the published minimum fails: copper spacing, annular ring, holes, slots, copper-to-edge, and two bodies that overlap. A value between that minimum and the recommendation is a warning. Silkscreen and package-to-package spacing are warnings. The board is still fabricated.
