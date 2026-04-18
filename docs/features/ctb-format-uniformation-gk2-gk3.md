# CTB file format notes for Uniformation GK2 and GK3

## Scope and confidence

This document is a practical reverse-engineering spec for CTB, focused on what matters for Uniformation GK2 and GK3 workflows.

Primary source used: UVtools source code (`ChituboxFile`, `CTBEncryptedFile`, and related layer/parameter models).

The CTB format is proprietary. Some fields are still not fully named in community tooling, but the motion/exposure fields below are well established.

## Short answers first

1. Is CTB a ZIP file?
No. CTB is a binary container with fixed-structure headers and pointer tables. It is not a ZIP archive.

2. Which parts are per-layer configurable (lift speed and related)?
CTB v3+ supports per-layer overrides for exposure, light-off/delays, lift/retract speeds, 2-stage lift/retract values, and light PWM (details below).

3. Can I double expose one layer with different bitmaps without lifting?
Not as a single layer command in CTB. A CTB layer record references one bitmap payload. There is no native "two masks in one layer record" feature.

Possible workaround: encode two consecutive layer records at the same Z with different bitmaps, while setting lift/retract movement to zero or near zero. This depends on firmware behavior and may not be reliable on every printer profile.

## Uniformation compatibility focus

## GK2

- UVtools explicitly models a special GK2 CTB variant:
  - magic: `MAGIC_CTBv4_GKtwo = 0xFF220810`
  - extension path supported by UVtools: `.gktwo.ctb`
- UVtools also includes a GK2-specific disclaimer block size and writes a GK2-specific disclaimer text branch while encoding v4+ CTB.

Interpretation: GK2 support in community tooling is not just "generic CTB". There is a GK2-flavored CTB variant in the wild.

## GK3

- In the same UVtools code, there is no dedicated GK3 magic constant or distinct GK3 container branch equivalent to GK2.
- Practical implication: current community behavior strongly suggests GK3 uses standard modern CTB (v4/v5 family), not a separate GK3-only container signature.

## CTB is binary, not archive

CTB structure is pointer-based binary data, roughly:

1. File header
2. Print parameter blocks (global settings)
3. Optional extended parameter blocks (v4/v5 style)
4. Preview table and preview image payloads
5. Layer definition table(s)
6. Layer bitmap payloads (RLE-compressed, optionally encrypted depending on variant)

Unlike ZIP formats, entries are not named files in a directory table. They are offsets/sizes into one binary stream.

## Core CTB layer model

From UVtools CTB handling:

- Base `LayerDef` includes:
  - `PositionZ`
  - `ExposureTime`
  - `LightOffSeconds` / `LightOffDelay`
  - `DataAddress` / `LayerDataOffset`
  - `DataSize` / `DataLength`
  - `PageNumber`
  - table size and unknown padding fields

- Extended layer record (`LayerDefEx` or equivalent, version-dependent) includes per-layer motion/wait/light overrides such as:
  - `LiftHeight`
  - `LiftSpeed`
  - `LiftHeight2`
  - `LiftSpeed2`
  - `RetractSpeed`
  - `RetractHeight2`
  - `RetractSpeed2`
  - `RestTimeBeforeLift` (maps to wait after cure)
  - `RestTimeAfterLift`
  - `RestTimeAfterRetract` (maps to wait before cure)
  - `LightPWM`

The exact block shape varies across CTB generations, but these semantics are consistent in mainstream tooling.

## Global settings vs per-layer overrides

CTB carries both:

- Global defaults (header/parameter blocks)
- Optional per-layer overrides (layer extended records)

A per-layer enable flag is present in slicer info (`PerLayerSettings`, with observed values aligned to version family like `0x30`, `0x40`, `0x50` in community tooling).

If per-layer settings are not enabled, firmware generally follows global timing/motion defaults.

## Per-layer options that matter for tuning

The following are the important knobs you can vary by layer in CTB-capable toolchains:

1. `PositionZ`
2. `ExposureTime`
3. `LightOffDelay` (or equivalent wait field mapping)
4. `WaitTimeBeforeCure` (rest after retract)
5. `WaitTimeAfterCure` (rest before lift)
6. `LiftHeight`
7. `LiftSpeed`
8. `LiftHeight2` (second lift stage distance)
9. `LiftSpeed2` (second lift stage speed)
10. `WaitTimeAfterLift`
11. `RetractSpeed`
12. `RetractHeight2` (second retract stage distance)
13. `RetractSpeed2` (second retract stage speed)
14. `LightPWM`

Plus bottom-layer specific global variants:

- Bottom exposure, bottom lift/retract speeds, bottom lift/retract second-stage values, bottom waits, bottom light PWM.

## Anti-aliasing note

CTB handling differs from legacy CBDDLP anti-aliasing replication. In UVtools, CTB branch uses CTB-specific image encoding (`EncodeCtbImage` / `DecodeCtbImage`) and separate anti-alias metadata flags/levels.

This is relevant because AA in CTB is not equivalent to "store N independent masks per layer as in older split-level schemes".

## Double exposure with different bitmaps: exact answer

### What CTB natively provides

- One layer record points to one image payload.
- One exposure value is attached to that layer record.

So CTB does not define a native "layer has two different bitmaps exposed sequentially before peel" primitive.

### What can be emulated

You can emulate dual exposure by writing:

1. Layer N at Z = Z0 with bitmap A
2. Layer N+1 at Z = Z0 with bitmap B

And forcing motion between them to zero/near-zero (lift/retract heights and delays tuned accordingly).

### Practical risk

- Firmware may still enforce parts of a peel cycle or timing guardrails.
- Slicers/printers may sanitize "odd" parameter combinations.
- Mechanical reality: even with no commanded lift, resin flow and cure history can differ from true intra-layer multi-pulse features in other ecosystems.

Conclusion: possible as a hack in some pipelines, not guaranteed as a portable CTB feature.

## Practical recommendations for GK2 and GK3

1. GK2
Use GK2-aware CTB generation path (special magic/signature handling), not just generic CTB writer defaults.

2. GK3
Start with standard CTB v5-compatible path unless proven otherwise by device-side failures.

3. Per-layer tuning
Prefer explicit per-layer overrides only where needed (burn-in, transition, and known problem-height zones). Keep global defaults sane.

4. Dual-exposure experiments
If you test same-Z dual records, validate on printer with small calibration jobs and inspect actual motion logs/behavior before production use.

## Implementation checklist for parser/writer work

1. Validate CTB magic and version early.
2. Parse global parameter blocks and slicer info.
3. Respect per-layer flag semantics.
4. Decode layer table and extended per-layer blocks (v3+).
5. Decode/encode CTB RLE payload correctly.
6. Keep preview offsets/sizes consistent.
7. Recompute all addresses/sizes after write.
8. For GK2, support GK2 magic variant path.

## Known unknowns

- Several reserved/unknown fields remain named as unknown in public tooling.
- Vendor firmware behavior may diverge from file-level capabilities, especially for unusual timing/motion combinations.
- GK3 may still have model-specific quirks outside currently exposed community constants.
