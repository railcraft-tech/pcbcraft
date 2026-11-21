# Parts

`pcbcraft parts alternatives spec.yaml` searches LCSC or PCBWay. Every item needs `supplier`. The two suppliers use different parameter keys.

```yaml
items:
  - name: R12
    supplier: lcsc
    parameters:
      category: resistor
      package: "0805"
      resistance: { min: "9.9k", max: "10.1k" }
      tolerance: { min: "0.1%", max: "1%" }
  - name: U1
    supplier: pcbway
    parameters:
      mpn: ["STM32F103C8T6"]
      package: ["LQFP-48"]
```

LCSC requires `category`. Any other LCSC parameter is a `min`/`max` pair or a list of accepted strings. A bare number is rejected. PCBWay requires an `mpn` list. `manufacturer`, `package`, and `description` are optional lists.

`--limit` defaults to 10. `--include-out-of-stock` keeps empty stock. `--basic` keeps LCSC basic parts only. `--format yaml` prints the matches as YAML. The default is one line per hit: supplier, id, mpn, package, stock.

## Search

`pcbcraft parts search --supplier jlcpcb --query resistor` searches the JLCPCB assembly catalog. Part numbers in the JSON array are LCSC codes.

Product attributes are a JSON object. Pass the file as the last argument, pipe it on stdin, or repeat `-p key=value`. A list of values for one key is OR. Different keys are AND. `-p` adds to the object.

```text
pcbcraft parts search --supplier jlcpcb --query resistor < params.json
pcbcraft parts search --supplier jlcpcb --query resistor params.json
pcbcraft parts search --supplier jlcpcb --query resistor -p package=0805 -p Resistance=10kΩ -p Tolerance=±1%
```

```json
{
  "package": "0805",
  "manufacturer": "UNI-ROYAL(Uniroyal Elec)",
  "Resistance": "10kΩ",
  "Tolerance": ["±1%", "±5%"]
}
```

`package` and `manufacturer` filter those catalog fields. Every other key is a product attribute. The name and the value are the catalog's own spelling, the same strings `components show` prints. `10kΩ` is not `10k` or `10000`.

`--limit` defaults to 10 and must be from 1 to 50. `--include-out-of-stock` keeps empty stock. `--basic` keeps basic parts only.

`pcbcraft mcp` exposes this command as `parts_search`. There is no stdin on that tool. Pass the object as `attributes`, the file as `params`, and each `-p` value in `param`.
