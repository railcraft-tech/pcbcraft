# Electrical tests

Seven JSON test types read the net graph and `.pcbcraft/components.sqlite3`. Each type is its own file. `pcbcraft tests init defaults` does not create them. `pcbcraft tests init type ERCValidator my-check` does. Run `pcbcraft components sync` first. A missing database, a missing LCSC number, a missing catalog attribute, or a missing tolerance fails that part's line.

References are `R`, `C`, and `L` followed by digits. `RN2` is not included. The catalog attribute is matched with the same aliases as LCSC search. `1%` is stored as `1.0`, so the fraction is `si / 100`.

Rails are never guessed from the net name. A rail entry has string `min` and `max`, both required, and both must be voltages. A bare number is a schema error. An empty `rails` object is valid JSON and fails the test with `no rails in spec`.

`powerHeadroom` and `voltageHeadroom` default to 2. The rating must be at least that multiple of the worst case. The worst case uses the corners of the rail ranges and the low end of the resistance tolerance.

- `CatalogValue`: the schematic `Value` lies inside the catalog nominal ± tolerance. One line per part, `CatalogValue.R12`.
- `ResistorPower`: a resistor between two rails dissipates less than `rating / powerHeadroom`. The line name includes the worst-case power.
- `VoltageRating`: a capacitor between two rails, or a resistor that publishes a voltage rating, stays under `rating / voltageHeadroom`.
- `Divider`: exactly two resistors share a non-rail net and their other ends sit on two different rails. Other pins on the output are allowed. A third resistor on that net drops the divider. The line name includes the output span. `expect` keys the output net; the whole span must lie inside the window.
- `TimeConstant`: one resistor and one capacitor bridge the same two nets. The line name includes `τ`. `expect` keys the sorted references and uses times such as `1ms`.
- `ParallelPassives`: two or more resistors share both nets. The line name includes the equivalent resistance.
- `SeriesPassives`: two resistors, or two capacitors, meet on a net that has only those two pins. The line name includes the equivalent.

A file that matches nothing produces one ordinary failure line, not a subtest, so a mistyped rail does not look like success. An `expect` key with no network fails too.

Groups run after `PartAvailability`, in the order listed above. The suite header count is the number of lines. The lines are printed when that group finishes. These checks do not call the network.

## Track current

Not implemented. The board file stores copper thickness per layer under `(setup (stackup ...))`. On longfred-v1b, `F.Cu` and `B.Cu` are 0.035 mm and the inner copper layers are 0.0152 mm. A segment also stores its layer, which the width parser currently drops. Net classes store track width and pair geometry, not current. Operating current is not a KiCad field. Zone copper, which carries power-net current, is not parsed. An IPC-2221 check still needs a declared current and a temperature rise.
