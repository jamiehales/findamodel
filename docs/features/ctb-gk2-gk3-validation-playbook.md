# CTB GK2/GK3 validation playbook

## Purpose

This playbook gives a repeatable validation process for CTB parsing/writing compatibility on Uniformation GK2 and GK3 workflows.

Related:
- `docs/features/ctb-format-uniformation-gk2-gk3.md`
- `docs/features/ctb-byte-layout-reference.md`
- `docs/features/ctb-parser-pseudocode.md`

## Goals

1. Confirm files are accepted by target printer workflow.
2. Confirm global and per-layer parameters are preserved.
3. Confirm bitmap payload decode/encode integrity.
4. Catch regressions when parser/writer changes.

## Sample set design

Build a corpus with at least:

1. tiny single-layer file
2. standard 100-layer file
3. file with transition layers
4. file with per-layer speed overrides
5. file with 2-stage lift/retract values
6. file with non-default PWM
7. GK2-specific variant sample
8. v5-style sample with resin parameters

## Static checks (no printing)

For each sample:

1. magic/version recognized
2. all offsets in-bounds
3. layer count matches parsed tables
4. last-layer index consistency (v4+)
5. preview blocks parse correctly
6. all layer payloads decode to expected raster size
7. no run-length overflow during decode

## Round-trip checks

For each sample:

1. parse original
2. write file back
3. parse rewritten
4. compare key semantic fields

Compare at minimum:

1. layer count, resolution, machine dimensions
2. bottom/normal exposure and waits
3. lift/retract speeds and stage-2 values
4. per-layer overrides where present
5. per-layer non-zero pixel counts

Allow differences only for known non-semantic metadata (timestamps, preserved unknown bytes policy, etc.).

## Printer acceptance checks

On device or vendor viewer workflow, verify:

1. file appears in file browser
2. preview renders
3. estimated print time is plausible
4. slice preview scroll works without corruption
5. print can be started

## Motion behavior checks

Use a tiny diagnostic part and verify:

1. bottom layers follow configured parameters
2. transition zone behavior matches expectations
3. per-layer override zone changes are observable
4. 2-stage motion actually applies when set

## GK2-specific checks

1. accept GK2 magic variant path
2. validate file opens/prints in GK2 workflow
3. verify no corruption from generic-v4 assumptions

## GK3-specific checks

1. validate standard modern CTB path first
2. confirm no hidden incompatibilities with profile machine name
3. verify per-layer override behavior on at least one test print

## Regression checklist for code changes

When parser/writer code changes:

1. rerun static corpus checks
2. rerun round-trip semantic diff checks
3. rerun at least one physical acceptance check per target family
4. archive pass/fail matrix with commit hash

## Suggested pass criteria

1. 100 percent static parse success on corpus
2. 100 percent semantic round-trip on required fields
3. no new printer-side rejection on GK2/GK3 acceptance tests
4. no new visible layer corruption in visual diff checks

## Failure triage order

1. magic/variant routing issue
2. offset or table-size calculation error
3. RLE decode/encode mismatch
4. per-layer extension mapping issue
5. writer normalization or sanitization side-effect

## Artifacts to keep

For each run archive:

1. original file
2. rewritten file
3. semantic diff report
4. layer bitmap diff summary
5. printer acceptance notes
