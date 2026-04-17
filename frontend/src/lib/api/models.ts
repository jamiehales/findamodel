import type { CommonMetadataFields } from './metadata';
import { apiFetch, apiUrl } from '../http';

function mapModelUrls(model: Model): Model {
  return {
    ...model,
    fileUrl: apiUrl(model.fileUrl),
    previewUrl: model.previewUrl ? apiUrl(model.previewUrl) : null,
  };
}

function mapRelatedModelUrls(model: RelatedModel): RelatedModel {
  return {
    ...model,
    previewUrl: model.previewUrl ? apiUrl(model.previewUrl) : null,
  };
}

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
  generatedTags: string[];
  generatedTagConfidence: Record<string, number>;
  generatedTagsStatus: string;
  generatedTagsAt: string | null;
  generatedTagsError: string | null;
  generatedTagsModel: string | null;
  generatedDescription: string | null;
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
  const r = await apiFetch(url);
  if (!r.ok) throw new Error('Failed to fetch models');
  const models = (await r.json()) as Model[];
  return models.map(mapModelUrls);
}

export async function fetchModelsByIds(ids: string[]): Promise<Model[]> {
  if (ids.length === 0) return [];

  const r = await apiFetch('/api/models/by-ids', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ids }),
  });
  if (!r.ok) throw new Error('Failed to fetch models by ids');
  const models = (await r.json()) as Model[];
  return models.map(mapModelUrls);
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

function appendFilterParams(
  params: URLSearchParams,
  filter: ModelFilter,
  modelName?: string,
): void {
  if (filter.search) params.set('search', filter.search);
  if (modelName) params.set('modelName', modelName);
  for (const v of filter.creator) params.append('creator', v);
  for (const v of filter.collection) params.append('collection', v);
  for (const v of filter.subcollection) params.append('subcollection', v);
  for (const v of filter.tags) params.append('tags', v);
  for (const v of filter.category) params.append('category', v);
  for (const v of filter.type) params.append('type', v);
  for (const v of filter.material) params.append('material', v);
  for (const v of filter.fileType) params.append('fileType', v);
  if (filter.supported !== null) params.set('supported', String(filter.supported));
}

export async function fetchQueryModels(
  filter: ModelFilter,
  limit: number,
  offset: number = 0,
  modelName?: string,
): Promise<ModelQueryResult> {
  const params = new URLSearchParams();
  params.set('limit', String(limit));
  params.set('offset', String(offset));
  appendFilterParams(params, filter, modelName);
  const r = await apiFetch(`/api/query?${params}`);
  if (!r.ok) throw new Error('Failed to query models');
  const result = (await r.json()) as ModelQueryResult;
  return {
    ...result,
    models: result.models.map(mapModelUrls),
  };
}

export async function fetchFilterOptions(
  filter: ModelFilter,
  modelName?: string,
): Promise<FilterOptions> {
  const params = new URLSearchParams();
  appendFilterParams(params, filter, modelName);
  const r = await apiFetch(`/api/query/options?${params}`);
  if (!r.ok) throw new Error('Failed to fetch filter options');
  return r.json();
}

export async function fetchModel(id: string): Promise<Model> {
  const r = await apiFetch(`/api/models/${id}`);
  if (!r.ok) throw new Error(`Failed to fetch model ${id}`);
  const model = (await r.json()) as Model;
  return mapModelUrls(model);
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
  const r = await apiFetch(`/api/models/${id}/geometry`, {
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

export interface AutoSupportVector {
  x: number;
  y: number;
  z: number;
}

export interface AutoSupportPoint {
  x: number;
  y: number;
  z: number;
  radiusMm: number;
  pullForce: AutoSupportVector;
}

export interface AutoSupportJob {
  jobId: string;
  status: 'queued' | 'running' | 'completed' | 'failed';
  progressPercent: number;
  supportCount: number;
  errorMessage: string | null;
  supportPoints: AutoSupportPoint[] | null;
}

export async function fetchSplitGeometry(id: string): Promise<SplitGeometryResponse | null> {
  const r = await apiFetch(`/api/models/${id}/geometry/split`, {
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

export async function createAutoSupportJob(id: string): Promise<AutoSupportJob> {
  const r = await apiFetch(`/api/models/${id}/auto-support/jobs`, {
    method: 'POST',
  });
  if (!r.ok) throw new Error('Failed to start support generation');
  return r.json();
}

export async function fetchAutoSupportJob(id: string, jobId: string): Promise<AutoSupportJob> {
  const r = await apiFetch(`/api/models/${id}/auto-support/jobs/${jobId}`);
  if (!r.ok) throw new Error('Failed to fetch support generation status');
  return r.json();
}

export async function fetchAutoSupportGeometry(
  id: string,
  jobId: string,
): Promise<SplitGeometryResponse | null> {
  const r = await apiFetch(`/api/models/${id}/auto-support/jobs/${jobId}/geometry`, {
    headers: { Accept: 'application/vnd.findamodel.mesh-split' },
  });
  if (r.status === 204 || r.status === 404) return null;
  if (!r.ok) throw new Error('Failed to fetch generated support geometry');

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
  const r = await apiFetch(`/api/models/${id}/other-parts`);
  if (!r.ok) throw new Error('Failed to fetch other parts');
  const parts = (await r.json()) as RelatedModel[];
  return parts.map(mapRelatedModelUrls);
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
  const r = await apiFetch(`/api/models/${id}/metadata`);
  if (!r.ok) throw new Error('Failed to fetch model metadata');
  return r.json();
}

export async function updateModelMetadata(
  id: string,
  request: UpdateModelMetadataRequest,
): Promise<Model> {
  const r = await apiFetch(`/api/models/${id}/metadata`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  });
  if (!r.ok) throw new Error('Failed to update model metadata');
  const model = (await r.json()) as Model;
  return mapModelUrls(model);
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

export interface PlateGenerationJob {
  jobId: string;
  fileName: string;
  format: '3mf' | 'stl' | 'glb' | 'pngzip' | 'pngzip_mesh' | 'pngzip_orthographic';
  status: 'queued' | 'running' | 'completed' | 'failed';
  totalEntries: number;
  completedEntries: number;
  progressPercent: number;
  currentEntryName: string | null;
  errorMessage: string | null;
  warning: string | null;
  skippedModels: string[];
}

export async function generatePlate(
  placements: PlatePlacement[],
  format: '3mf' | 'stl' | 'glb' | 'pngzip' | 'pngzip_mesh' | 'pngzip_orthographic' = '3mf',
  printerConfigId?: string | null,
): Promise<PlateGenerationResult> {
  const r = await apiFetch('/api/plate/generate', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ placements, format, printerConfigId }),
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

export async function createPlateGenerationJob(
  placements: PlatePlacement[],
  format: '3mf' | 'stl' | 'glb' | 'pngzip' | 'pngzip_mesh' | 'pngzip_orthographic' = '3mf',
  printerConfigId?: string | null,
): Promise<PlateGenerationJob> {
  const r = await apiFetch('/api/plate/jobs', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ placements, format, printerConfigId }),
  });
  if (!r.ok) {
    const message = await r.text();
    throw new Error(message || 'Failed to start plate generation');
  }

  return r.json();
}

export async function fetchPlateGenerationJob(jobId: string): Promise<PlateGenerationJob> {
  const r = await apiFetch(`/api/plate/jobs/${jobId}`);
  if (!r.ok) throw new Error('Failed to fetch plate generation status');
  return r.json();
}

export function getPlateGenerationDownloadUrl(jobId: string): string {
  return apiUrl(`/api/plate/jobs/${jobId}/file`);
}
