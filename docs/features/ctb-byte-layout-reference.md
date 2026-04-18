# CTB byte layout reference (implementation page)

## Purpose

This page is the implementation companion to the GK2/GK3 overview page.

Use it when building or validating a CTB parser/writer.

Related overview:
- see `docs/features/ctb-format-uniformation-gk2-gk3.md`

## Sources and trust level

This layout is derived from reverse-engineering in UVtools (`ChituboxFile` and `CTBEncryptedFile`).

Notes:
- CTB is proprietary; unknown/reserved fields still exist.
- Field names here match community names where possible.
- Treat this as "known-good engineering map," not an official vendor spec.

## Endianness and addressing

- Endianness: Little-endian in common CTB handling.
- Addressing: Most offsets are absolute file offsets, with some `(page, offset)` pairs used for large files.
- Large-file paging: page size is 4,294,967,296 bytes (4 GiB).

## Magic values and variants

Common values seen in UVtools:

1. `0x12FD0019` - CBDDLP legacy family
2. `0x12FD0086` - CTB classic
3. `0x12FD0106` - CTB v4/v5 family
4. `0xFF220810` - Uniformation GK2 CTB variant (`gktwo.ctb` path)
5. `0x12FD0107` - encrypted CTB variant in dedicated UVtools handler

## High-level physical layout

Typical file flow:

1. Header
2. Print parameter block(s)
3. Optional slicer info / v4+ blocks
4. Optional disclaimer text block (v4+; GK2 has variant text sizing)
5. Optional resin parameters block (v5-style)
6. Preview table(s) + preview payload(s)
7. Layer table(s)
8. Layer image payloads (RLE, optionally encrypted depending on family)

## Classic CTB structural blocks (ChituboxFile model)

## Header (major fields)

The classic CTB header carries:

1. `Magic` (u32)
2. `Version` (u32)
3. Build volume / display dimensions (`BedSizeX`, `BedSizeY`, `BedSizeZ`)
4. Model/global print settings (`LayerHeight`, `Exposure`, `BottomExposure`, `LightOffDelay`, etc.)
5. Resolution (`ResolutionX`, `ResolutionY`)
6. Layer count
7. Preview offsets
8. Print time
9. Projector/mirror flag
10. Print parameter table offset + size
11. Anti-alias and light PWM fields
12. Encryption key (classic CTB can carry non-zero key)
13. Slicer info offset + size

## PrintParameters block (global motion/exposure defaults)

Well-known fields include:

1. `BottomLiftHeight`
2. `BottomLiftSpeed`
3. `LiftHeight`
4. `LiftSpeed`
5. `RetractSpeed`
6. Material estimates (`VolumeMl`, `WeightG`, `CostDollars`)
7. `BottomLightOffDelay`
8. `LightOffDelay`
9. `BottomLayerCount`
10. Padding/reserved fields

## SlicerInfo block (v3+ important extras)

Observed key fields:

1. `BottomLiftHeight2`
2. `BottomLiftSpeed2`
3. `LiftHeight2`
4. `LiftSpeed2`
5. `RetractHeight2`
6. `RetractSpeed2`
7. `RestTimeAfterLift`
8. Machine name pointer/size
9. `AntiAliasFlag`
10. `PerLayerSettings` flag
11. Modified timestamp (minutes since epoch)
12. AA level value
13. Additional rest fields
14. Transition layer count
15. `PrintParametersV4Address` (v4+)

## PrintParametersV4 block (v4/v5 extras)

Observed key fields:

1. `BottomRetractSpeed`
2. `BottomRetractSpeed2`
3. `RestTimeAfterRetract`
4. `RestTimeAfterLift`
5. `RestTimeBeforeLift`
6. `BottomRetractHeight2`
7. `LastLayerIndex`
8. Disclaimer address + length
9. Resin parameters address (v5-style)
10. Reserved bytes

## ResinParameters block (v5-style metadata)

Observed key fields:

1. Resin color channels
2. Machine name address/length
3. Resin type address/length
4. Resin name address/length
5. Resin density

## Preview records

Preview table entries generally include:

1. `ResolutionX`
2. `ResolutionY`
3. `ImageOffset`
4. `ImageLength`
5. Reserved/unknown words

Two previews are commonly present in CTB workflows.

## Layer records (classic CTB)

## Base layer record (`LayerDef`)

Known table size:
- `TABLE_SIZE = 36` bytes (base)

Core fields:

1. `PositionZ` (f32)
2. `ExposureTime` (f32)
3. `LightOffSeconds` (f32)
4. `DataAddress` (u32)
5. `DataSize` (u32)
6. `PageNumber` (u32)
7. `TableSize` (u32)
8. unknown/reserved words

## Extended per-layer record (`LayerDefEx`, v3+)

Known table size:
- `TABLE_SIZE = 48` bytes (extension)

Core fields:

1. embedded/copy base layer info handle
2. `TotalSize`
3. `LiftHeight`
4. `LiftSpeed`
5. `LiftHeight2`
6. `LiftSpeed2`
7. `RetractSpeed`
8. `RetractHeight2`
9. `RetractSpeed2`
10. `RestTimeBeforeLift`
11. `RestTimeAfterLift`
12. `RestTimeAfterRetract`
13. `LightPWM`

Implementation note:
- UVtools seeks to `DataAddress - 84` for these records in one update path.
- `84 = 36 + 48`, matching base + extension combined metadata footprint.

## Encrypted CTB structural blocks (CTBEncryptedFile model)

This is a separate handler in UVtools, not just a flag on classic CTB.

## Slicer settings table

Known table size:
- `TABLE_SIZE = 288` bytes

Carries many global fields (display/machine/layer settings, offsets, AA, motion defaults, waits, per-layer flag, disclaimer/resin pointers, last-layer index, and more).

## Layer pointer table

Known entry size / shape:

1. `LayerOffset` (u32)
2. `PageNumber` (u32)
3. `LayerTableSize` (u32), commonly `0x58`
4. padding (u32)

## Encrypted layer definition

Known table size:
- `TABLE_SIZE = 88` bytes (`0x58`)

Observed fields include:

1. `PositionZ`
2. `ExposureTime`
3. `LightOffDelay`
4. plain layer data pointer/size
5. encrypted layer data pointer/size
6. lift/retract and wait fields
7. `LightPWM`
8. unknown/reserved fields

## Per-layer settings capability map

For CTB-capable paths, per-layer modifiers in UVtools include:

1. `PositionZ`
2. `LightOffDelay`
3. `WaitTimeBeforeCure`
4. `ExposureTime`
5. `WaitTimeAfterCure`
6. `LiftHeight`
7. `LiftSpeed`
8. `LiftHeight2`
9. `LiftSpeed2`
10. `WaitTimeAfterLift`
11. `RetractSpeed`
12. `RetractHeight2`
13. `RetractSpeed2`
14. `LightPWM`

Important:
- support depends on version and per-layer flag state.
- transition and bottom behavior can be represented through global + software-expanded layer values.

## What is not in CTB as a native primitive

No dedicated "two separate bitmap masks for one layer record with two exposure pulses and no peel in between" structure is defined in these models.

One layer record maps to one bitmap payload.

## GK2-specific notes

1. GK2 variant uses dedicated magic (`0xFF220810`) in UVtools.
2. GK2 path writes a variant disclaimer size/content branch.
3. Treat GK2 as CTB-family but not always byte-identical to generic v4 defaults.

## GK3-specific notes

No dedicated GK3 magic constant or dedicated GK3 CTB class branch was found in the same UVtools code paths.

Practical parser/writer stance:
- parse as standard modern CTB family first.
- allow machine-name/profile-based handling for quirks.

## Parser checklist (recommended)

1. Read header and validate magic/version.
2. Branch by magic family (classic, GK2 variant, encrypted variant).
3. Resolve block offsets safely with bounds checks.
4. Read parameter blocks (global motion/exposure first).
5. Read preview descriptors and payloads.
6. Read layer table base entries.
7. If version/family supports it, read per-layer extension entries.
8. Decode RLE payloads (and decrypt if encrypted family).
9. Normalize per-layer values against global defaults.
10. Validate layer count, last layer index, and table sizes for consistency.

## Writer checklist (recommended)

1. Build layers and deduplicate payloads only if safe.
2. Encode RLE payloads.
3. Emit header with temporary offsets.
4. Emit parameter blocks.
5. Emit previews.
6. Emit layer metadata blocks.
7. Emit layer payloads.
8. Patch all offsets/sizes/page references.
9. Set per-layer capability flag only when needed.
10. Re-open and self-parse output as a verification pass.

## Validation scenarios

Minimum validation set:

1. Single-layer tiny model
2. Transition layers enabled
3. Per-layer override only on one middle layer
4. Two-stage lift/retract values non-zero
5. Large layer data crossing high offsets
6. GK2 target profile sample
7. Round-trip parse-write-parse equality on timing/motion fields

## Cross-page link

For compatibility and workflow interpretation, read:
- `docs/features/ctb-format-uniformation-gk2-gk3.md`
