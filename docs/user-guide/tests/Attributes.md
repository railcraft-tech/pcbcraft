# Attributes

[Tests](../tests.md)

A mounting hole or a fiducial must stay out of the BOM and the position file, and its LCSC and PCBWay fields must be empty. The footprint library or the footprint name has to be `MountingHole` or `Fiducial`, or start with that word. `MountingHole:MountingHole_3.2mm_M3` and `Fiducial:Fiducial_1mm_Mask2mm` both match.

`pcbcraft tests init type Attributes holes` writes the file. The spec is only the project.

```json
{
  "type": "Attributes",
  "spec": { "project": "../../board.kicad_pro" }
}
```

One line per such part, named by its reference. A part that is in the BOM, in the position file, or carries a catalog number fails. A board with none of these footprints passes, and the report says so. A skip is `Attributes.H1`.
