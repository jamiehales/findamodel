# CTB double-exposure experiments (same-Z dual bitmap)

## Purpose

This page is an experimental guide for testing whether a target CTB printer workflow can emulate "double expose with different masks without lifting."

Important:
- CTB does not provide a native single-layer dual-bitmap primitive.
- This guide uses an emulation approach and should be treated as experimental.

Related:
- `docs/features/ctb-format-uniformation-gk2-gk3.md`
- `docs/features/ctb-byte-layout-reference.md`

## Emulation idea

Encode two consecutive records with:

1. identical `PositionZ`
2. bitmap A in record N
3. bitmap B in record N+1
4. minimized inter-record motion parameters

## Expected behavior classes

1. Firmware accepts and performs two exposures at same Z.
2. Firmware enforces movement despite zero-like values.
3. Firmware sanitizes or rejects unusual same-Z sequences.
4. Firmware rewrites timing behavior at print time.

## Safety warning

Do not start with critical parts.

Start with tiny calibration masks and ensure:

1. vat and release film condition are good
2. over-cure risk is acceptable
3. supports and peel forces are low

## Test matrix

Use a small 3x3 experiment matrix:

1. Z pattern: same-Z exact vs +epsilon Z
2. lift/retract: zero vs tiny non-zero
3. waits: zero vs short waits

For each cell, record:

1. printer accepted/rejected file
2. observed plate motion
3. effective exposure sequence
4. result quality

## Minimal sequence template

```text
Layer N:
  Z = Z0
  Bitmap = A
  Exposure = tA
  LiftHeight = 0 (or tiny)
  LiftSpeed = minimal safe
  RetractSpeed = minimal safe
  waits tuned low

Layer N+1:
  Z = Z0
  Bitmap = B
  Exposure = tB
  LiftHeight = 0 (or tiny)
  LiftSpeed = minimal safe
  RetractSpeed = minimal safe
  waits tuned low
```

## Measurement recommendations

1. Capture printer video at high frame rate if possible.
2. Log total print time vs predicted print time.
3. Compare cured area against expected union/intersection of A and B.
4. Inspect for unintended extra cure from light bleed and residual resin flow.

## Failure patterns and interpretation

1. Layers merged visually with no distinct effect
Likely firmware/slicer normalization or over-cure masking differences.

2. Unexpected peel between same-Z layers
Firmware enforces mechanical cycle regardless of zero-like settings.

3. Print aborted or file rejected
Validator rejects same-Z or zero-motion constraints.

4. Severe dimensional errors
Thermal/resin kinetics or over-cure dominates intended dual-mask effect.

## GK2 and GK3 notes

1. GK2
Use GK2-specific CTB variant path for file generation and testing.

2. GK3
Test as standard modern CTB first; watch for profile-level behavior overrides.

## Decision rule

Promote this technique only if all conditions hold:

1. accepted by firmware reliably
2. no hidden lift cycle observed
3. repeatable dimensional behavior on at least 10 runs
4. no increase in print failures beyond threshold

If any fail, treat as unsupported in production.
