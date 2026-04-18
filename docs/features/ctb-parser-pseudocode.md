# CTB parser pseudocode (GK2/GK3 oriented)

## Purpose

This page gives an implementation-ready parser flow for CTB-family files with explicit handling for:

1. Classic CTB
2. GK2 CTB variant
3. Encrypted CTB branch

Related references:
- `docs/features/ctb-format-uniformation-gk2-gk3.md`
- `docs/features/ctb-byte-layout-reference.md`
- `docs/features/ctb-rle-reference.md`

## Data model sketch

```text
CtbFile
  Header
  GlobalSettings
  SlicerInfo
  PrintParametersV4
  ResinParameters
  Previews[]
  Layers[]

Layer
  Index
  Z
  Exposure
  LightOffDelay
  WaitBeforeCure
  WaitAfterCure
  LiftHeight
  LiftSpeed
  LiftHeight2
  LiftSpeed2
  WaitAfterLift
  RetractSpeed
  RetractHeight2
  RetractSpeed2
  LightPWM
  Bitmap
```

## Parser flow

```text
function parseCtb(stream):
  assert stream.length >= minimumHeaderSize

  header = readHeader(stream)

  variant = detectVariant(header.magic)
  if variant == UNKNOWN:
    error("unsupported magic")

  version = header.version
  if version < 1 or version > 5:
    warn("unexpected version")

  global = readGlobalBlocks(stream, header, variant, version)

  previews = readPreviews(stream, header, global, variant, version)

  layerMeta = readLayerMetadata(stream, header, global, variant, version)

  layers = []
  for i in 0 .. layerMeta.count-1:
    meta = layerMeta[i]
    raw = readLayerPayload(stream, meta, variant)
    decoded = decodeLayerPayload(raw, meta, global, variant, i)
    layer = buildLayerObject(meta, decoded, global, i, version, variant)
    layers.append(layer)

  normalizeAndValidate(layers, global, header, version, variant)

  return CtbFile(...)
```

## Variant detection

```text
function detectVariant(magic):
  if magic == 0xFF220810: return GK2_CTB
  if magic == 0x12FD0107: return ENCRYPTED_CTB
  if magic == 0x12FD0106: return CTB_V4V5
  if magic == 0x12FD0086: return CTB_CLASSIC
  if magic == 0x12FD0019: return CBDDLP
  return UNKNOWN
```

## Global block parsing

```text
function readGlobalBlocks(stream, header, variant, version):
  seek to header.printParametersOffset
  printParams = readPrintParameters(stream)

  slicerInfo = null
  if header.slicerOffset > 0 and header.slicerSize > 0:
    seek to header.slicerOffset
    slicerInfo = readSlicerInfo(stream)

  ppV4 = null
  if version >= 4 and slicerInfo.printParametersV4Address > 0:
    seek to slicerInfo.printParametersV4Address
    ppV4 = readPrintParametersV4(stream)

  resin = null
  if version >= 5 and ppV4.resinParametersAddress > 0:
    seek to ppV4.resinParametersAddress
    resin = readResinParameters(stream)

  return {printParams, slicerInfo, ppV4, resin}
```

## Layer metadata parsing

```text
function readLayerMetadata(stream, header, global, variant, version):
  count = header.layerCount
  meta = new array[count]

  if variant == ENCRYPTED_CTB:
    pointers = readEncryptedLayerPointers(stream, global.settings.layerPointersOffset, count)
    for i in 0 .. count-1:
      meta[i] = readEncryptedLayerDef(stream, pointers[i])
    return meta

  # classic CTB path
  baseTableOffset = header.layersDefinitionOffset
  seek baseTableOffset
  for i in 0 .. count-1:
    base = readLayerDef(stream)
    meta[i] = base

  if version >= 3:
    for i in 0 .. count-1:
      exOffset = computeLayerDefExOffset(meta[i])
      seek exOffset
      ex = readLayerDefEx(stream)
      merge(meta[i], ex)

  return meta
```

## Payload parsing and decoding

```text
function readLayerPayload(stream, meta, variant):
  offset = resolvePayloadOffset(meta)
  length = resolvePayloadLength(meta)
  assertBounds(offset, length, stream.length)
  seek offset
  return stream.read(length)

function decodeLayerPayload(raw, meta, global, variant, layerIndex):
  data = raw

  if variant == ENCRYPTED_CTB:
    data = decryptEncryptedLayerPayload(data, meta, global, layerIndex)

  bitmap = decodeCtbRle(data, global.resolutionX, global.resolutionY)
  return bitmap
```

## Per-layer parameter mapping

```text
function buildLayerObject(meta, bitmap, global, index, version, variant):
  layer = new Layer()

  layer.Index = index
  layer.Z = meta.positionZ
  layer.Exposure = meta.exposureTime
  layer.LightOffDelay = meta.lightOffDelay

  # Defaults from global
  layer.WaitBeforeCure = global.waitBeforeCure
  layer.WaitAfterCure = global.waitAfterCure
  layer.LiftHeight = global.liftHeight
  layer.LiftSpeed = global.liftSpeed
  layer.LiftHeight2 = global.liftHeight2
  layer.LiftSpeed2 = global.liftSpeed2
  layer.WaitAfterLift = global.waitAfterLift
  layer.RetractSpeed = global.retractSpeed
  layer.RetractHeight2 = global.retractHeight2
  layer.RetractSpeed2 = global.retractSpeed2
  layer.LightPWM = global.lightPWM

  # Override from per-layer extension when present
  if meta.hasExtendedParameters:
    layer.LiftHeight = meta.liftHeight - meta.liftHeight2
    layer.LiftSpeed = meta.liftSpeed
    layer.LiftHeight2 = meta.liftHeight2
    layer.LiftSpeed2 = meta.liftSpeed2
    layer.RetractSpeed = meta.retractSpeed
    layer.RetractHeight2 = meta.retractHeight2
    layer.RetractSpeed2 = meta.retractSpeed2
    layer.WaitAfterCure = meta.restTimeBeforeLift
    layer.WaitAfterLift = meta.restTimeAfterLift
    layer.WaitBeforeCure = meta.restTimeAfterRetract
    layer.LightPWM = meta.lightPWM

  layer.Bitmap = bitmap
  return layer
```

## Safety and consistency checks

```text
function normalizeAndValidate(layers, global, header, version, variant):
  assert layers.length == header.layerCount

  for each layer in layers:
    assert layer.Z >= 0
    assert layer.Exposure >= 0
    assert layer.LightPWM in [0, 255]
    clamp all speeds/heights/waits to sane ranges

  ensure Z is monotonic or known-safe non-monotonic per policy

  if version >= 4 and global.ppV4.lastLayerIndex is present:
    assert global.ppV4.lastLayerIndex == header.layerCount - 1
```

## GK2-specific branch hints

1. Accept GK2 magic explicitly.
2. Do not hard-reject when disclaimer lengths differ from generic CTB v4 defaults.
3. Keep parser tolerant for unknown reserved values in v4+ blocks.

## GK3 implementation posture

No dedicated GK3 magic branch is represented in the same UVtools code paths. Implement GK3 as standard modern CTB unless real samples require a special branch.

## Recommended parser API

```text
parse(filePath) -> CtbFile
readMetadata(filePath) -> CtbMetadataOnly
decodeLayer(filePath, layerIndex) -> Bitmap
validate(filePath) -> ValidationReport
```

## Recommended writer API

```text
write(ctbFile, options) -> bytes
writeLayerPreview(ctbFile, options) -> preview-only artifact
roundTripValidate(original, rewritten) -> diff report
```
