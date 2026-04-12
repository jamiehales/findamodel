import type { CommonMetadataFields } from './metadata';

export interface Model {
  id: string;
  name: string;
  partName: string | null;
  relativePath: string;
  fileType: string;
  canExportToPlate: boolean;
  fileSize: number;
  fileUrl: string;
  hasPreview: boolean;
  previewUrl: string | null;
  creator: string | null;
  collection: string | null;
  subcollection: string | null;
  tags: string[];
  category: string | null;
  type: string | null;
  material: string | null;
  supported: boolean | null;
  convexHull: string | null;
  concaveHull: string | null;
  convexSansRaftHull: string | null;
  raftHeightMm: number;
  dimensionXMm: number | null;
  dimensionYMm: number | null;
  dimensionZMm: number | null;
  sphereCentreX: number | null;
  sphereCentreY: number | null;
  sphereCentreZ: number | null;
  sphereRadius: number | null;
}

export async function fetchModels(limit?: number): Promise<Model[]> {
  const url = limit !== undefined ? `/api/models?limit=${limit}` : '/api/models';
  const r = await fetch(url);
  if (!r.ok) throw new Error('Failed to fetch models');
  return r.json();
}

export interface ModelFilter {
  search: string;
  creator: string[];
  collection: string[];
  subcollection: string[];
  tags: string[];
  category: string[];
  type: string[];
  material: string[];
  fileType: string[];
  supported: boolean | null;
}

export const emptyFilter: ModelFilter = {
  search: '',
  creator: [],
  collection: [],
  subcollection: [],
  tags: [],
  category: [],
  type: [],
  material: [],
  fileType: [],
  supported: null,
};

export interface ModelQueryResult {
  models: Model[];
  totalCount: number;
  hasMore: boolean;
}

export interface FilterOptions {
  creators: string[];
  collections: string[];
  subcollections: string[];
  tags: string[];
  categories: string[];
  types: string[];
  materials: string[];
  fileTypes: string[];
}

export async function fetchQueryModels(
  filter: ModelFilter,
  limit: number,
  offset: number = 0,
): Promise<ModelQueryResult> {
  const params = new URLSearchParams();
  params.set('limit', String(limit));
  params.set('offset', String(offset));
  if (filter.search) params.set('search', filter.search);
  for (const v of filter.creator) params.append('creator', v);
  for (const v of filter.collection) params.append('collection', v);
  for (const v of filter.subcollection) params.append('subcollection', v);
  for (const v of filter.tags) params.append('tags', v);
  for (const v of filter.category) params.append('category', v);
  for (const v of filter.type) params.append('type', v);
  for (const v of filter.material) params.append('material', v);
  for (const v of filter.fileType) params.append('fileType', v);
  if (filter.supported !== null) params.set('supported', String(filter.supported));
  const r = await fetch(`/api/query?${params}`);
  if (!r.ok) throw new Error('Failed to query models');
  return r.json();
}

export async function fetchFilterOptions(): Promise<FilterOptions> {
  const r = await fetch('/api/query/options');
  if (!r.ok) throw new Error('Failed to fetch filter options');
  return r.json();
}

export async function fetchModel(id: string): Promise<Model> {
  const r = await fetch(`/api/models/${id}`);
  if (!r.ok) throw new Error(`Failed to fetch model ${id}`);
  return r.json();
}

export interface GeometryResponse {
  positions: Float32Array;
  indices: Uint16Array | Uint32Array;
  triangleCount: number;
  sphereRadius: number;
  sphereCentre: { x: number; y: number; z: number };
  dimensionXMm: number;
  dimensionYMm: number;
  dimensionZMm: number;
}

export interface RelatedModel {
  id: string;
  name: string;
  relativePath: string;
  fileType: string;
  fileSize: number;
  previewUrl: string | null;
}

export async function fetchGeometry(id: string): Promise<GeometryResponse> {
  const r = await fetch(`/api/models/${id}/geometry`, {
    headers: {
      Accept: 'application/vnd.findamodel.mesh',
    },
  });
  if (!r.ok) throw new Error('Failed to fetch geometry');

  const contentType = r.headers.get('content-type') ?? '';
  if (contentType.includes('application/vnd.findamodel.mesh')) {
    return decodeBinaryGeometry(await r.arrayBuffer());
  }

  const legacy = (await r.json()) as {
    positions: number[];
    normals: number[];
    triangleCount: number;
    sphereRadius: number;
    sphereCentre: { x: number; y: number; z: number };
    dimensionXMm: number;
    dimensionYMm: number;
    dimensionZMm: number;
  };

  const triangleIndices = new Uint32Array(legacy.triangleCount * 3);
  for (let i = 0; i < triangleIndices.length; i++) triangleIndices[i] = i;

  return {
    positions: new Float32Array(legacy.positions),
    indices: triangleIndices,
    triangleCount: legacy.triangleCount,
    sphereRadius: legacy.sphereRadius,
    sphereCentre: legacy.sphereCentre,
    dimensionXMm: legacy.dimensionXMm,
    dimensionYMm: legacy.dimensionYMm,
    dimensionZMm: legacy.dimensionZMm,
  };
}

const BINARY_MESH_MAGIC = 0x48534d46;
const BINARY_MESH_HEADER_SIZE = 56;

function decodeBinaryGeometry(buffer: ArrayBuffer): GeometryResponse {
  if (buffer.byteLength < BINARY_MESH_HEADER_SIZE) {
    throw new Error('Binary mesh payload too small');
  }

  const view = new DataView(buffer);
  const magic = view.getUint32(0, true);
  if (magic !== BINARY_MESH_MAGIC) {
    throw new Error('Unexpected binary mesh format');
  }

  const version = view.getUint8(4);
  if (version !== 1) {
    throw new Error(`Unsupported binary mesh version ${version}`);
  }

  const indexElementSize = view.getUint8(5);
  const quantizationBits = view.getUint8(6);
  if (quantizationBits !== 16) {
    throw new Error(`Unsupported mesh quantization ${quantizationBits}`);
  }

  const vertexCount = view.getUint32(8, true);
  const triangleCount = view.getUint32(12, true);
  const dimensionXMm = view.getFloat32(16, true);
  const dimensionYMm = view.getFloat32(20, true);
  const dimensionZMm = view.getFloat32(24, true);
  const sphereCentre = {
    x: view.getFloat32(28, true),
    y: view.getFloat32(32, true),
    z: view.getFloat32(36, true),
  };
  const sphereRadius = view.getFloat32(40, true);
  const positionsByteLength = view.getUint32(44, true);
  const indexOffset = view.getUint32(48, true);
  const indicesByteLength = view.getUint32(52, true);

  const expectedPositionsByteLength = vertexCount * 3 * Uint16Array.BYTES_PER_ELEMENT;
  if (positionsByteLength !== expectedPositionsByteLength) {
    throw new Error('Corrupt binary mesh positions section');
  }

  if (indexOffset + indicesByteLength > buffer.byteLength) {
    throw new Error('Corrupt binary mesh index section');
  }

  const quantized = new Uint16Array(buffer, BINARY_MESH_HEADER_SIZE, vertexCount * 3);
  const positions = new Float32Array(vertexCount * 3);
  const xScale = dimensionXMm === 0 ? 0 : dimensionXMm / 65535;
  const yScale = dimensionYMm === 0 ? 0 : dimensionYMm / 65535;
  const zScale = dimensionZMm === 0 ? 0 : dimensionZMm / 65535;
  const minX = -dimensionXMm / 2;
  const minZ = -dimensionZMm / 2;

  for (let i = 0; i < quantized.length; i += 3) {
    positions[i] = minX + quantized[i] * xScale;
    positions[i + 1] = quantized[i + 1] * yScale;
    positions[i + 2] = minZ + quantized[i + 2] * zScale;
  }

  const indexCount = triangleCount * 3;
  let indices: Uint16Array | Uint32Array;
  if (indexElementSize === Uint16Array.BYTES_PER_ELEMENT) {
    if (indicesByteLength !== indexCount * Uint16Array.BYTES_PER_ELEMENT) {
      throw new Error('Corrupt Uint16 mesh index data');
    }
    indices = new Uint16Array(buffer, indexOffset, indexCount);
  } else if (indexElementSize === Uint32Array.BYTES_PER_ELEMENT) {
    if (indicesByteLength !== indexCount * Uint32Array.BYTES_PER_ELEMENT) {
      throw new Error('Corrupt Uint32 mesh index data');
    }
    indices = new Uint32Array(buffer, indexOffset, indexCount);
  } else {
    throw new Error(`Unsupported mesh index size ${indexElementSize}`);
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

export interface SplitGeometryResponse {
  body: GeometryResponse;
  supports: GeometryResponse | null;
}

export async function fetchSplitGeometry(id: string): Promise<SplitGeometryResponse | null> {
  const r = await fetch(`/api/models/${id}/geometry/split`, {
    headers: { Accept: 'application/vnd.findamodel.mesh-split' },
  });
  if (r.status === 204) return null;
  if (!r.ok) throw new Error('Failed to fetch split geometry');

  const buf = await r.arrayBuffer();
  const view = new DataView(buf);
  let offset = 0;

  const bodyLength = view.getUint32(offset, true);
  offset += 4;
  const body = decodeBinaryGeometry(buf.slice(offset, offset + bodyLength));
  offset += bodyLength;

  const supportLength = view.getUint32(offset, true);
  offset += 4;
  const supports =
    supportLength > 0 ? decodeBinaryGeometry(buf.slice(offset, offset + supportLength)) : null;

  return { body, supports };
}

export async function fetchOtherParts(id: string): Promise<RelatedModel[]> {
  const r = await fetch(`/api/models/${id}/other-parts`);
  if (!r.ok) throw new Error('Failed to fetch other parts');
  return r.json();
}

export interface UpdateModelMetadataRequest {
  name: string | null;
  partName: string | null;
  creator: CommonMetadataFields['creator'];
  collection: CommonMetadataFields['collection'];
  subcollection: CommonMetadataFields['subcollection'];
  tags: CommonMetadataFields['tags'];
  category: CommonMetadataFields['category'];
  type: CommonMetadataFields['type'];
  material: CommonMetadataFields['material'];
  supported: CommonMetadataFields['supported'];
  raftHeightMm: CommonMetadataFields['raftHeightMm'];
}

export interface ModelMetadata {
  name: string | null;
  partName: string | null;
  creator: CommonMetadataFields['creator'];
  collection: CommonMetadataFields['collection'];
  subcollection: CommonMetadataFields['subcollection'];
  tags: CommonMetadataFields['tags'];
  category: CommonMetadataFields['category'];
  type: CommonMetadataFields['type'];
  material: CommonMetadataFields['material'];
  supported: CommonMetadataFields['supported'];
  raftHeightMm: CommonMetadataFields['raftHeightMm'];
}

export interface ModelMetadataDetail {
  localValues: ModelMetadata;
  inheritedValues: ModelMetadata | null;
}

export async function fetchModelMetadata(id: string): Promise<ModelMetadataDetail> {
  const r = await fetch(`/api/models/${id}/metadata`);
  if (!r.ok) throw new Error('Failed to fetch model metadata');
  return r.json();
}

export async function updateModelMetadata(
  id: string,
  request: UpdateModelMetadataRequest,
): Promise<Model> {
  const r = await fetch(`/api/models/${id}/metadata`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  });
  if (!r.ok) throw new Error('Failed to update model metadata');
  return r.json();
}

export interface PlatePlacement {
  modelId: string;
  instanceIndex: number;
  xMm: number;
  yMm: number;
  angleRad: number;
}

export interface PlateGenerationResult {
  blob: Blob;
  warning: string | null;
  skippedModels: string[];
}

export async function generatePlate(
  placements: PlatePlacement[],
  format: '3mf' | 'stl' | 'glb' = '3mf',
): Promise<PlateGenerationResult> {
  const r = await fetch('/api/plate/generate', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ placements, format }),
  });
  if (!r.ok) {
    const message = await r.text();
    throw new Error(message || 'Failed to generate plate');
  }

  const warning = r.headers.get('X-Plate-Warning');
  const skippedRaw = r.headers.get('X-Plate-Skipped-Models');
  const skippedModels = skippedRaw
    ? skippedRaw
        .split(',')
        .map((m) => m.trim())
        .filter((m) => m.length > 0)
    : [];

  return {
    blob: await r.blob(),
    warning,
    skippedModels,
  };
}
