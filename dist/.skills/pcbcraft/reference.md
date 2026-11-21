# pcbcraft reference

## Alternative spec

```yaml
items:
  - name: R12
    supplier: lcsc
    parameters:
      category: resistor
      package: "0805"
      resistance: { min: "9.9k", max: "10.1k" }
      tolerance: { min: "0.1%", max: "1%" }
      power: { min: "0.0625W", max: "0.25W" }
      dielectric: ["X7R", "X5R"]
  - name: U1
    supplier: pcbway
    parameters:
      mpn: ["STM32F103C8T6"]
      manufacturer: ["STMicroelectronics", "ST"]
      package: ["LQFP-48"]
      description: ["Cortex-M3"]
```

An LCSC electrical parameter is a `min`/`max` pair or a list. A bare number is rejected. PCBWay `mpn` is a list of strings.

## Catalog search

`pcbcraft parts search --supplier jlcpcb --query resistor params.json`

```json
{
  "package": "0805",
  "manufacturer": "UNI-ROYAL(Uniroyal Elec)",
  "Resistance": "10kΩ",
  "Tolerance": ["±1%", "±5%"]
}
```

`package` and `manufacturer` filter those fields. Every other key is a product attribute, spelled the way the catalog prints it. A bare number is rejected. The same object is the `attributes` argument of `parts_search`.

## Report JSON

`pcbcraft tests run` prints a text summary. A validator `Report` contains:

- `symbols`: reference, value, footprint, library, properties
- `nets`: name and pins (`reference`, `pin`)
- `violations`: `check` (`erc` or `drc`), `severity` (`error`, `warning`, `exclusion`), `kind`, `description`, `items`
