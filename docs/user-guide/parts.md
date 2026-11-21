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
