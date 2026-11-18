# Skipping tests

`pcbcraft tests run` reads `pcbcraft.yaml` from the current directory. A missing file skips nothing. A broken file stops the run with exit code 2 before any test starts.

```yaml
tests:
  skipped:
    - VoltageRating.C21
    - PartAvailability.C12345
    - Divider.R1/R2
    - ERCValidator.erc-test
```

`tests` and `skipped` may be omitted. An unknown key is an error. A blank entry is an error.

The value is the printed identity of one line. A subtest is `Type.` plus the name before the measured suffix, so `VoltageRating.C21` skips the line printed as `VoltageRating.C21 3.3V`. `Divider.R1/R2` skips the divider whose name starts with those references. A whole file is `Type.` plus the file name without `.json`, and the colon form `ERCValidator: erc-test` is accepted too. `VoltageRating.C2` does not skip `VoltageRating.C21`.

A skipped line is printed with a yellow `s` and `(skipped)`. It counts as a test and does not count as an assertion or a failure. The footer adds `, N skipped` when N is greater than zero. A skipped `PartAvailability` part is not looked up in the catalog. An entry that matches no line is reported on stderr and does not fail the run: `skipped entry 'Divider.R9' did not match a test`.
