# Nets

`pcbcraft net GND dump` prints the track widths on that net, a text star of the parts, and the pin-to-pin list (`U1.3 -> R1.2`).

The graph is cached in `.pcbcraft/net.sqlite` and rebuilt when the schematic or the board changes. The same net name can come out as more than one star. `--project board.kicad_pro` picks the project.
