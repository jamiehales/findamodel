# CTB RLE reference (practical)

## Purpose

This page gives a practical decode/encode reference for CTB layer bitmap payloads based on community implementations.

Related:
- `docs/features/ctb-byte-layout-reference.md`
- `docs/features/ctb-parser-pseudocode.md`

## Important context

CTB payload encoding is not PNG or ZIP. Layer images are stored as compact run-length streams.

The exact bit-level details vary by format family and version, but CTB implementations in UVtools use CTB-specific encode/decode paths (`EncodeCtbImage`, `DecodeCtbImage`).

## Conceptual model

The stream represents alternating runs of same-valued pixels in raster order.

A control byte carries:

1. pixel value/signature bits
2. run-length mode bits

Additional bytes may follow to represent larger run lengths.

## Run length tiers seen in common implementations

Typical strategy in CTB-style encoders:

1. short run in single byte extension
2. medium run with two-byte extension
3. long run with three-byte extension
4. very long run with four-byte extension

This is why the decoder should be written as:

1. parse control byte
2. branch by run-length prefix bits
3. read N extra bytes
4. emit repeated pixel value

## Decode pseudocode

```text
function decodeCtbRle(data, width, height):
  out = new byte[width * height]
  pos = 0
  i = 0

  while i < data.length and pos < out.length:
    control = data[i]
    i += 1

    pixelValue = decodePixelValueFromControl(control)
    runLength = decodeRunLength(control, data, i)
    i += extraBytesConsumedByRunLength(control)

    if runLength <= 0:
      error("invalid run")

    if pos + runLength > out.length:
      error("run exceeds output")

    fill out[pos : pos + runLength] with pixelValue
    pos += runLength

  if pos != out.length:
    error("incomplete raster")

  return out as image(width, height)
```

## Encode pseudocode

```text
function encodeCtbRle(pixels):
  out = []
  i = 0

  while i < pixels.length:
    value = pixels[i]
    run = 1

    while i + run < pixels.length and pixels[i + run] == value:
      run += 1

    emitControlAndRun(out, value, run)
    i += run

  return out
```

## Pixel domain notes

1. Many pipelines treat layer rasters as monochrome-like masks even when AA metadata exists.
2. AA semantics in CTB are not identical to legacy multi-plane CBDDLP semantics.
3. Keep decode output normalized to one deterministic internal representation for your toolchain.

## Validation tests to include

1. all-black layer
2. all-white layer
3. checkerboard
4. single white pixel at each corner
5. long continuous run crossing tier boundaries
6. random noise layer
7. round-trip encode/decode equality test

## Corruption guards

1. reject negative/zero run lengths
2. reject runs that overflow target raster length
3. reject payloads that end before required continuation bytes
4. reject payloads that leave raster underfilled

## Encrypted CTB sequencing

For encrypted CTB:

1. read encrypted payload
2. decrypt payload
3. then perform RLE decode

Do not attempt RLE decode before successful decryption.

## Practical debugging tips

1. Log first 64 decoded runs for failing layers.
2. Dump decoded raster to PNG for visual verification.
3. Compare non-zero pixel counts against metadata when present.
4. Check layer bounding boxes if your model computes them.
