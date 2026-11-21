# ERCValidator

[Tests](../tests.md)

KiCad electrical rules check. An error fails the file. A warning or an exclusion is printed, and the file still passes.

`pcbcraft tests init defaults` writes `erc-test.json`. The spec is only the project. The path is relative to the JSON file.

```json
{
  "type": "ERCValidator",
  "spec": { "project": "../../board.kicad_pro" }
}
```

The file is one line, named after the test file. A skip for `erc-test.json` is `ERCValidator.erc-test`.
