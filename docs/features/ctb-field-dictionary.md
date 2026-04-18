# CTB field dictionary (engineering glossary)

## Purpose

This glossary maps CTB-family field names to practical behavior and units.

Related:
- `docs/features/ctb-byte-layout-reference.md`
- `docs/features/ctb-parser-pseudocode.md`

## Exposure and timing

1. `ExposureTime` (s)
Normal-layer UV on-time.

2. `BottomExposureTime` (s)
Bottom-layer UV on-time.

3. `LightOffDelay` (s)
Delay with UV off around layer cycle. In tooling, may overlap with wait fields depending on profile conventions.

4. `BottomLightOffDelay` (s)
Bottom-layer equivalent of light-off delay.

5. `WaitTimeBeforeCure` (s)
Rest after retract, before UV on.

6. `WaitTimeAfterCure` (s)
Rest after UV off, before lift.

7. `WaitTimeAfterLift` (s)
Rest after lift, before retract.

## Motion

1. `LiftHeight` (mm)
Primary peel/lift distance per layer.

2. `LiftSpeed` (mm/min)
Primary lift speed.

3. `LiftHeight2` (mm)
Secondary lift segment distance (TSMC-style 2-stage motion).

4. `LiftSpeed2` (mm/min)
Secondary lift segment speed.

5. `RetractSpeed` (mm/min)
Primary downward speed returning toward next cure position.

6. `RetractHeight2` (mm)
Secondary retract segment distance.

7. `RetractSpeed2` (mm/min)
Secondary retract segment speed.

8. `BottomLiftHeight`, `BottomLiftSpeed`, `BottomLiftHeight2`, `BottomLiftSpeed2`
Bottom-layer variants of lift parameters.

9. `BottomRetractSpeed`, `BottomRetractHeight2`, `BottomRetractSpeed2`
Bottom-layer retract variants.

## Layer geometry and payload pointers

1. `PositionZ` (mm)
Absolute build platform Z for that layer.

2. `DataAddress` or `LayerDataOffset` (u32)
Offset to encoded bitmap payload.

3. `DataSize` or `DataLength` (u32)
Encoded payload length in bytes.

4. `PageNumber` (u32)
Large-file paging component for addressing.

5. `TableSize` / `LayerTableSize` (u32)
Declared layer record size.

## Image quality and light power

1. `LightPWM` (0-255)
Normal-layer UV intensity duty setting.

2. `BottomLightPWM` (0-255)
Bottom-layer UV intensity duty setting.

3. `AntiAliasFlag`
Flag field indicating AA mode presence in CTB families.

4. `AntiAliasLevel`
Configured AA level metadata.

## Machine/profile metadata

1. `ResolutionX`, `ResolutionY` (pixels)
Panel resolution.

2. `DisplayWidth`, `DisplayHeight` (mm)
Physical display dimensions.

3. `MachineName`
Printer profile identity string.

4. `ProjectorType`
Mirror/projection orientation mode.

5. `TransitionLayerCount`
Transition ramp layer count.

6. `BottomLayerCount`
Bottom exposure layer count.

## Per-layer control flags

1. `PerLayerSettings`
Controls whether per-layer override semantics are enabled.

2. observed values in community tooling
`0x30` for v3 family, `0x40` for v4 family, `0x50` for v5 family.

## v4/v5 extra blocks

1. `PrintParametersV4Address`
Pointer to v4+ extra parameter block.

2. `DisclaimerAddress`, `DisclaimerLength`
Pointer/length for copyright disclaimer text area.

3. `ResinParametersAddress`
Pointer to resin metadata block in v5-style files.

4. `LastLayerIndex`
Consistency/checkpoint value usually equal to `LayerCount - 1`.

## Encrypted CTB terms

1. `EncryptedDataOffset`, `EncryptedDataLength`
Encrypted payload address and length.

2. `LayerXorKey` and other crypto fields
Variant-dependent decryption metadata.

## Unknown and reserved fields

You will see `Unknown*`, `Padding*`, `Reserved*` in reverse-engineered models.

Guideline:

1. preserve bytes on round-trip when possible
2. do not assume semantic meaning without sample-backed proof
3. treat structural offsets and sizes as authoritative over guessed labels
