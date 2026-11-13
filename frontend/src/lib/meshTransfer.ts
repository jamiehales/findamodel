import { MeshoptDecoder } from 'three/examples/jsm/libs/meshopt_decoder.module.js';
import type { GeometryResponse } from './api/models';

// Decoder for the FMSH meshes the backend sends (backend/src/geometry/mesh_transfer.rs): a
// 56-byte header, then meshoptimizer-encoded 16-bit positions (x, y, z, 0 per vertex) and
// triangle indices. Keep in step with that module.

const MAGIC = 0x48534d46; // "FMSH"
const VERSION = 2;
const HEADER_BYTES = 56;
const QUANT_MAX = 65535;

/** Decodes one FMSH mesh. */
export async function decodeMesh(bytes: Uint8Array): Promise<GeometryResponse> {
  await MeshoptDecoder.ready;
  if (bytes.byteLength < HEADER_BYTES) throw new Error('Mesh payload too small');
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (view.getUint32(0, true) !== MAGIC) throw new Error('Unexpected mesh format');
  const version = view.getUint8(4);
  if (version !== VERSION) throw new Error(`Unsupported mesh version ${version}`);
  const indexElementSize = view.getUint8(5);
  if (indexElementSize !== 2 && indexElementSize !== 4) {
    throw new Error(`Unsupported mesh index size ${indexElementSize}`);
  }
  if (view.getUint8(6) !== 16) throw new Error('Unsupported mesh quantisation');

  const vertexCount = view.getUint32(8, true);
  const triangleCount = view.getUint32(12, true);
  const f = (offset: number) => view.getFloat32(offset, true);
  const [dimensionXMm, dimensionYMm, dimensionZMm] = [f(16), f(20), f(24)];
  const sphereCentre = { x: f(28), y: f(32), z: f(36) };
  const sphereRadius = f(40);
  const vertexBytes = view.getUint32(44, true);
  const indexBytes = view.getUint32(48, true);
  if (HEADER_BYTES + vertexBytes + indexBytes !== bytes.byteLength) {
    throw new Error('Corrupt mesh payload');
  }

  const quantised = new Uint16Array(vertexCount * 4);
  if (vertexCount > 0) {
    MeshoptDecoder.decodeVertexBuffer(
      new Uint8Array(quantised.buffer),
      vertexCount,
      8,
      bytes.subarray(HEADER_BYTES, HEADER_BYTES + vertexBytes),
    );
  }
  const indexCount = triangleCount * 3;
  const indices =
    indexElementSize === 2 ? new Uint16Array(indexCount) : new Uint32Array(indexCount);
  if (indexCount > 0) {
    MeshoptDecoder.decodeIndexBuffer(
      new Uint8Array(indices.buffer),
      indexCount,
      indexElementSize,
      bytes.subarray(HEADER_BYTES + vertexBytes),
    );
  }

  const positions = new Float32Array(vertexCount * 3);
  const scale = [dimensionXMm, dimensionYMm, dimensionZMm].map((d) => d / QUANT_MAX);
  const min = [-dimensionXMm / 2, 0, -dimensionZMm / 2];
  for (let i = 0; i < vertexCount; i++) {
    for (let k = 0; k < 3; k++) {
      positions[i * 3 + k] = min[k] + quantised[i * 4 + k] * scale[k];
    }
  }

  return {
    positions,
    indices,
    triangleCount,
    sphereRadius,
    sphereCentre,
    dimensionXMm,
    dimensionYMm,
    dimensionZMm,
  };
}
