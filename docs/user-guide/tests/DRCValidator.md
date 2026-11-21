# DRCValidator

[Tests](../tests.md)

KiCad design rules check. An error fails the file. A warning or an exclusion is printed, and the file still passes.

`pcbcraft tests init defaults` writes `drc-test.json`. The spec is only the project. The path is relative to the JSON file.

```json
{
  "type": "DRCValidator",
  "spec": { "project": "../../board.kicad_pro" }
}
```

The file is one line, named after the test file. A skip for `drc-test.json` is `DRCValidator.drc-test`.
