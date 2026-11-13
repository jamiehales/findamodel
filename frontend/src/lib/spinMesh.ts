import { MeshoptDecoder } from 'three/examples/jsm/libs/meshopt_decoder.module.js';

// Decoder for the spin mesh files the backend writes (backend/src/preview/spin.rs): a header,
// then meshoptimizer-encoded 12-bit positions and indices. Keep in step with that module.

const MAGIC = 'FSPN';
const SPIN_MESH_VERSION = 1;
const HEADER_BYTES = 84;
const QUANT_MAX = 4095;

export interface Bounds {
  min: [number, number, number];
  max: [number, number, number];
}

export interface SpinMesh {
  /** Y-up, mm: the coordinates the previews are drawn in. */
  positions: Float32Array;
  indices: Uint16Array | Uint32Array;
  /** The supports-removed preview's framing box. */
  bodyBounds: Bounds;
  /** The with-supports preview's framing box (the whole model). */
  modelBounds: Bounds;
}

export async function decodeSpinMesh(buffer: ArrayBuffer): Promise<SpinMesh> {
  await MeshoptDecoder.ready;
  const view = new DataView(buffer);
  const bytes = new Uint8Array(buffer);
  if (
    buffer.byteLength < HEADER_BYTES ||
    String.fromCharCode(...bytes.subarray(0, 4)) !== MAGIC ||
    view.getUint32(4, true) !== SPIN_MESH_VERSION
  ) {
    throw new Error('Unsupported spin mesh');
  }
  const vertexCount = view.getUint32(8, true);
  const indexCount = view.getUint32(12, true);
  const vertexBytes = view.getUint32(16, true);
  const f = (offset: number) => view.getFloat32(offset, true);
  const v3 = (offset: number): [number, number, number] => [
    f(offset),
    f(offset + 4),
    f(offset + 8),
  ];
  const offset = v3(20);
  const scale = f(32) / QUANT_MAX;

  const quantised = new Uint16Array(vertexCount * 4);
  MeshoptDecoder.decodeVertexBuffer(
    new Uint8Array(quantised.buffer),
    vertexCount,
    8,
    bytes.subarray(HEADER_BYTES, HEADER_BYTES + vertexBytes),
  );
  const indices = vertexCount <= 0xffff ? new Uint16Array(indexCount) : new Uint32Array(indexCount);
  MeshoptDecoder.decodeIndexBuffer(
    new Uint8Array(indices.buffer),
    indexCount,
    indices.BYTES_PER_ELEMENT,
    bytes.subarray(HEADER_BYTES + vertexBytes),
  );

  const positions = new Float32Array(vertexCount * 3);
  for (let i = 0; i < vertexCount; i++) {
    for (let k = 0; k < 3; k++) {
      positions[i * 3 + k] = offset[k] + quantised[i * 4 + k] * scale;
    }
  }
  return {
    positions,
    indices,
    bodyBounds: { min: v3(36), max: v3(48) },
    modelBounds: { min: v3(60), max: v3(72) },
  };
}
