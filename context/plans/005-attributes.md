# Attributes

`Attributes` checks schematic symbol flags. The spec requires `project` and nothing else. `pcbcraft tests init defaults` does not write this file. `pcbcraft tests init type Attributes <name>` does.

The schematic parser stores KiCad's `(in_bom yes|no)` and `(in_pos_files yes|no)` on each symbol. A missing flag means `yes`, which is KiCad's default. The net cache keeps both columns. A cache written by an older parser is stale, so a hole is not reported as being in the BOM just because the new column defaulted to yes.

A footprint is a mounting hole or a fiducial when its library or its name is `MountingHole` or `Fiducial`, or starts with those words. `MountingHole:MountingHole_3.2mm_M3` and `Fiducial:Fiducial_1mm_Mask2mm` match. An empty footprint does not.

Each such symbol is one subtest, `Attributes.H1`. It passes only when both of these hold:

- `in_bom` and `in_pos_files` are no.
- The LCSC and PCBWay fields are empty.

A failure names the broken condition, for example `in BOM`, `in position file`, `LCSC C123`, or `PCBWay PW1`. A board with no mounting holes and no fiducials prints one ordinary passing line. That is not a failure.
