# Verification Summary Report Proposal

## Motivation

Kani verifies individual proof harnesses, but CI users often need a concise
summary of the verification run. A machine-readable summary would make it
easier to track verification health over time and integrate Kani with CI
dashboards.

## Proposed interface

Add an optional output format to the Kani command:

```text
cargo kani --output-format human
cargo kani --output-format json
```

The JSON report should include:

- Total harnesses discovered
- Passed, failed, and skipped harness counts
- Number of assertions checked
- Number of panics and arithmetic overflows detected
- Verification duration for each harness and the complete run
- A stable exit status that remains non-zero when verification fails

Example:

```json
{
  "harnesses": {
    "total": 2,
    "passed": 1,
    "failed": 1,
    "skipped": 0
  },
  "duration_ms": 1842,
  "results": [
    {"name": "check_addition", "status": "passed", "duration_ms": 731},
    {"name": "check_index", "status": "failed", "duration_ms": 1111}
  ]
}
```

## Implementation notes

The report should reuse Kani's existing verification result types and output
pipeline. Human-readable output should remain the default so this change does
not break existing users. Unit tests should cover aggregation, serialization,
empty runs, and failed harnesses. An integration test should verify the CLI
output and exit status.

## Benefits

This would improve CI/CD integrations, make regressions easier to identify,
and give users a quick overview of verification coverage without parsing
individual harness logs.
