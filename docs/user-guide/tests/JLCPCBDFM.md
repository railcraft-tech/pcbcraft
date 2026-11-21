# JLCPCBDFM

[Tests](../tests.md)

Local check of the published JLCPCB process limits. `pcbcraft tests init type JLCPCBDFM jlcpcb-dfm` writes the file. Run it with the others, or alone with `pcbcraft tests run --name jlcpcb-dfm`.

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

Each check is its own line. A skip is `JLCPCBDFM.annular-ring`, `JLCPCBDFM.body-to-edge`, `JLCPCBDFM.silk-width`, and the same form for the other lines: `different-net-spacing`, `same-net-spacing`, `smd-pad-to-track`, `smd-pad-spacing`, `mask-bridge`, `mask-to-trace`, `via-diameter`, `plated-hole`, `via-hole-to-copper`, `pth-to-copper`, `npth-to-copper`, `inner-via-hole-to-copper`, `inner-pth-to-copper`, `copper-to-edge`, `npth-diameter`, `npth-annular`, `plated-slot`, `npth-slot`, `via-hole-spacing`, `pth-hole-spacing`, `silk-height`, `silk-to-pad`, `smd-pad-size`, `hole-in-smd-pad`, `body-spacing`.
