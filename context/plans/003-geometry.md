# Geometry

`Geometry` reads a `.kicad_pcb` file and returns board-space outlines in millimetres, Y up, the same axes as pcbnew. It does not call `kicad-cli` and it does not write KiCad files. The s-expression reader is the one in `crates/kicad`. The parser does not depend on the CLI. `dump`, `is-occupied`, and `find-symbol` are consumers of the same shapes. `export 2d` reads the same shapes when it frames one footprint.

Footprint graphics are stored in the footprint's local coordinates. A footprint on `B.Cu` is mirrored in X before its rotation is applied. Pad outlines are built at the pad, then transformed with the footprint. An arc stays an arc: three points still define it after that transform.

## Outlines

A blocking shape has two closed areas. Neither is a centreline.

- `occupied` is the whole footprint of the object. A track is a stadium of its copper width. A via is the disk of its copper size. A hole is a circle. A round-rect pad is lines plus corner arcs from `roundrect_rratio` (`radius = ratio * min(width, height)`). A courtyard is the interior of its outline, grown by half the stroke width.
- `clearance` is `occupied` grown by a clearance. The band between them is empty space where another object must not be placed. `clearanceMm` is that distance. An outward offset keeps arcs as arcs: the radius grows by the clearance, and a sharp corner gains an arc of that radius.

Clearance numbers come from the sibling `.kicad_pro`:

- Copper (pad, track, via, filled zone) uses the net class `clearance` in `net_settings`. A net that matches no pattern uses the `Default` class, then `board.design_settings.rules.min_clearance`.
- A hole uses `min_hole_clearance`.
- `Edge.Cuts`, when it forms a closed board outline, uses `min_copper_edge_clearance` inward. The safe interior is the inset polygon. A point in that band, or outside the board, is blocked by the edge.
- A courtyard already is the keep-out for another footprint, so its `clearanceMm` is 0 and `clearance` equals `occupied`. Its pads still carry copper clearance.
- A keepout polygon whose flags disallow tracks, vias, pads, copper pour, or footprints is blocking and is not grown further. A keepout with only `allowed` is omitted unless `--all`.

`--board` without a project looks for `<stem>.kicad_pro` beside the board. Missing clearance rules are an error.

Graphics that block nothing (silk, fab, mask, paste, user drawings, text) appear only with `--all`. They have `occupied` and no clearance. Text is the font box, not glyph outlines. 3D models are never emitted.

`*.Cu` and `*.Mask` expand to the copper or mask layers declared on the board. A via from `F.Cu` to `B.Cu` is drawn on every copper layer between them. A drill is also a single shape on the logical layer `drill`.

An unknown pad shape, or a primitive that would otherwise be skipped, fails the parse and names the object.

## `geometry dump`

`pcbcraft geometry dump` writes every blocking shape on the board. `pcbcraft geometry dump 0-50x0-40` keeps shapes whose clearance bounds meet the closed window. A negative number is written with its sign, so `-10--5x0-40` is x from -10 to -5. The JSON contains the full outlines, including points outside the window. A small window on a copper pour therefore returns the whole `filled_polygon` when the bounds meet.

`--all` adds the non-blocking graphics. `--detailed` adds `points` beside `segments` on every outline: strings `"XxY"`, for example `["0.1x5.5"]`. `--detailed-precision` is the step in millimetres and defaults to `0.01`. Coordinates are rounded to that step and trailing zeros are dropped. An arc is sampled so the polyline stays within one step of the curve. A closed outline does not repeat its first point. The occupancy test does not use these samples.

`--format` is `json` or `png`. The default is `json`, printed to stdout. `--output` writes that JSON to a file instead. `-` stays on stdout. `png` draws the same shapes the JSON would list: occupied areas, clearance halos, outline vertices, and `--detailed` samples. Arcs are sampled at 0.05 mm and filled as polylines. Zones are painted first, then tracks, pads, vias, keepouts, courtyards, and other graphics. Drills and `Edge.Cuts` are last. `Edge.Cuts`, courtyards, and keepouts are stroked; the rest are filled. `F.Cu` is red, `B.Cu` is green, other copper is darker, silk is ochre, mask is teal, drills are black, and the board edge is yellow. The picture uses pcbnew axes with Y flipped, a 1 mm margin, and 20 px/mm, with the longer side capped at 4096 px. Without `--output` the file is `pcb-geometry.png`. `-` is rejected for PNG.

## `geometry is-occupied`

`pcbcraft geometry is-occupied --x 0 --y 10` builds a square window centred on the point. The side defaults to 10 mm (`--size`). The shapes are the same blocking set as `dump` without `--all`.

The point is occupied when it lies in any `clearance` area (or, for the board edge, outside the safe inset). Free space is measured along pcbnew axes: left is decreasing X, right increasing X, up increasing Y, down decreasing Y. A direction that steps further into a blocking area is `0`. Otherwise the distance is the first entry into a blocking area, capped by the window. If the ray hits nothing inside the window, the distance is the distance to the window edge and `limitedByWindow` is true.

The JSON carries the verdict, the four distances with the layer and source that stopped them, and `nearby`, which is the `dump` value for that square. `--detailed` fills `points` inside `nearby` only.

`is-occupied` prints JSON to stdout. `dump` does too, unless `--format png` or `--output` says otherwise. Both resolve the project the same way as the other commands.

## `geometry find-symbol`

`pcbcraft geometry find-symbol J1` keeps the shapes whose reference is `J1`: pads, courtyard, holes, and graphics of that footprint. A name that matches nothing is the error `no symbol 'J1'`.

The JSON matches `dump`: `unit`, `window`, and `layers`, plus `reference`. `window` is the occupied outline of those shapes. The clearance halo is not part of the window.

## `export 2d`

`pcbcraft export 2d` renders an orthographic view through `kicad-cli pcb render`, without perspective, as a square PNG. `--side` is `front` or `back` and defaults to `front`. KiCad receives that as `--side top` or `--side bottom`. `--width` is the image side in pixels and defaults to 1024. The file is `<stem>-2d.png`, or `<stem>-back-2d.png` for the back. With `--symbol J1` it is `<stem>-J1-2d.png`. `-o` sets the path.

`--symbol J1 --around-space 2mm` frames a square around that footprint. The same lookup as `find-symbol` supplies the occupied window. The margin is added on every side. The square side is the longer side after that expansion, and the centre stays the centre of the symbol. `2` and `2mm` both mean 2 mm. Another unit is an error. `--around-space` without `--symbol` is an error. A symbol without a margin uses 5 mm. The face comes from that footprint: a `B.CrtYd` courtyard selects the back, and so does a footprint whose layers are only `B.*`. Pads on both copper sides stay on the front unless the courtyard says otherwise. `--side` does not override that choice.

KiCad has no millimetre crop, so the camera is aimed instead. `--pivot` is the offset from the board centre to the square centre, in centimetres. `--zoom` is `max(board width, board height) / square side`. A zoom of 1 is the whole board. The board centre comes from `Edge.Cuts`, or from every shape when the outline is missing. Without `--symbol` the command renders the whole board.

## Cache

Geometry is always built for the whole board, then `dump`, `is-occupied`, and `find-symbol` filter that result. The filter is the same one as a direct parse: a window keeps shapes whose occupied or clearance bounds meet it, and `--all` is what adds the non-blocking graphics. `--detailed` samples `points` at print time, so the cache does not store them.

The cache lives next to the project:

- `.pcbcraft/geometry/{name}.json` is the full-board dump, including silk and the other non-blocking graphics, with exact `segments`.
- `.pcbcraft/{name}-hash` is the SHA-256 hex digest of the `.kicad_pcb` bytes.

`{name}` is the pcb file name. Any character that is not an ASCII letter, digit, `.`, `_`, or `-` becomes `-`, so `My Board.kicad_pcb` is `My-Board.kicad_pcb`.

A command reads the hash file first. When it equals the current file hash and the JSON loads, the board is not parsed again. A different hash, a missing file, or JSON that does not load causes a fresh parse. The JSON is replaced first, then the hash file, so a crash cannot leave a new hash pointing at a half-written dump. Clearance still comes from the `.kicad_pro` at the moment the cache is built.
