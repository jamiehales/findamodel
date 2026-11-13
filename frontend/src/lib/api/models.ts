import type { CommonMetadataFields } from './metadata';
import { apiFetch, apiUrl } from '../http';
import { decodeMesh } from '../meshTransfer';
import { decodeSpinMesh, type SpinMesh } from '../spinMesh';

function mapModelUrls(model: Model): Model {
  return {
    ...model,
    fileUrl: apiUrl(model.fileUrl),
    previewUrl: model.previewUrl ? apiUrl(model.previewUrl) : null,
    spinUrl: model.spinUrl ? apiUrl(model.spinUrl) : null,
  };
}

function mapRelatedModelUrls(model: RelatedModel): RelatedModel {
  return {
    ...model,
    previewUrl: model.previewUrl ? apiUrl(model.previewUrl) : null,
    spinUrl: model.spinUrl ? apiUrl(model.spinUrl) : null,
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
  /** The reduced body turned in 3D while the tile is hovered; null until generated. */
  spinUrl: string | null;
  creator: string | null;
  collection: string | null;
  subcollection: string | null;
  tags: string[];
  category: string | null;
  type: string | null;
  material: string | null;
  supported: boolean | null;
  /** The supports can be split from the model; null until it is indexed or split. */
  supportSplit: boolean | null;
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

export type ModelSortField = 'name' | 'fileName' | 'fileDate' | 'fileType' | 'path';
export type SortOrder = 'asc' | 'desc';

export interface ModelSort {
  field: ModelSortField;
  order: SortOrder;
}

export const defaultSortOrder = (field: ModelSortField): SortOrder =>
  field === 'fileDate' ? 'desc' : 'asc';

export const defaultSort: ModelSort = { field: 'fileDate', order: 'desc' };

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
  sort?: ModelSort,
): Promise<ModelQueryResult> {
  const params = new URLSearchParams();
  params.set('limit', String(limit));
  params.set('offset', String(offset));
  appendFilterParams(params, filter, modelName);
  if (sort) {
    params.set('sort', sort.field);
    params.set('order', sort.order);
  }
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
  /** The reduced body turned in 3D while the tile is hovered; null until generated. */
  spinUrl: string | null;
}

export async function fetchGeometry(id: string): Promise<GeometryResponse> {
  const r = await apiFetch(`/api/models/${id}/geometry`);
  if (!r.ok) throw new Error('Failed to fetch geometry');
  return decodeMesh(new Uint8Array(await r.arrayBuffer()));
}

export interface SplitGeometryResponse {
  body: GeometryResponse;
  supports: GeometryResponse | null;
}

export async function fetchSplitGeometry(id: string): Promise<SplitGeometryResponse | null> {
  const r = await apiFetch(`/api/models/${id}/geometry/split`);
  if (r.status === 204) return null;
  if (!r.ok) throw new Error('Failed to fetch split geometry');

  const bytes = new Uint8Array(await r.arrayBuffer());
  const view = new DataView(bytes.buffer);
  const bodyLength = view.getUint32(0, true);
  const body = await decodeMesh(bytes.subarray(4, 4 + bodyLength));
  const supportOffset = 4 + bodyLength;
  const supportLength = view.getUint32(supportOffset, true);
  const supports =
    supportLength > 0
      ? await decodeMesh(bytes.subarray(supportOffset + 4, supportOffset + 4 + supportLength))
      : null;

  return { body, supports };
}

/** The supports as findamodel rebuilds them for the VOXL export; null when it has none. */
export async function fetchResupportedGeometry(id: string): Promise<GeometryResponse | null> {
  const r = await apiFetch(`/api/models/${id}/geometry/resupported`);
  if (r.status === 204) return null;
  if (!r.ok) throw new Error('Failed to fetch rebuilt supports');
  return decodeMesh(new Uint8Array(await r.arrayBuffer()));
}

export interface RepairedGeometryResponse {
  geometry: GeometryResponse;
  /** One flag per triangle: the repair made it (a mended spot or a filled void). */
  changed: Uint8Array;
}

/**
 * The model body made watertight, with the faces the repair made flagged (sent after the
 * others).
 */
export async function fetchRepairedGeometry(id: string): Promise<RepairedGeometryResponse> {
  const r = await apiFetch(`/api/models/${id}/geometry/repaired`);
  if (!r.ok) throw new Error('Failed to fetch repaired geometry');

  const bytes = new Uint8Array(await r.arrayBuffer());
  const view = new DataView(bytes.buffer);
  const meshLength = view.getUint32(0, true);
  const geometry = await decodeMesh(bytes.subarray(4, 4 + meshLength));
  const flagOffset = 4 + meshLength;
  const faceCount = view.getUint32(flagOffset, true);
  if (faceCount !== geometry.triangleCount) throw new Error('Corrupt repaired mesh flags');
  const bits = bytes.subarray(flagOffset + 4, flagOffset + 4 + Math.ceil(faceCount / 8));
  const changed = new Uint8Array(faceCount);
  for (let i = 0; i < faceCount; i++) changed[i] = (bits[i >> 3] >> (i & 7)) & 1;
  return { geometry, changed };
}

/** A model's spin mesh, from the `spinUrl` its model carries (already an API URL). */
export async function fetchSpinMesh(spinUrl: string): Promise<SpinMesh> {
  const r = await fetch(spinUrl);
  if (!r.ok) throw new Error('Failed to fetch spin mesh');
  return decodeSpinMesh(await r.arrayBuffer());
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

/**
 * `voxl`: the model meshes as their files hold them. `voxl-resupported`: pre-supported models
 * without their supports, rebuilt as DragonFruit supports. `voxl-remeshed`: the resupported plate
 * with the model meshes made watertight.
 */
export type PlateFormat = '3mf' | 'stl' | 'glb' | 'voxl' | 'voxl-resupported' | 'voxl-remeshed';

/** Plate context for formats that carry it (VOXL centres on the bed and names the plate). */
export interface PlateOptions {
  bedWidthMm?: number;
  bedDepthMm?: number;
  plateName?: string;
}

export interface PlateGenerationJob {
  jobId: string;
  fileName: string;
  format: PlateFormat;
  status: 'queued' | 'running' | 'completed' | 'failed';
  totalEntries: number;
  completedEntries: number;
  progressPercent: number;
  currentEntryName: string | null;
  errorMessage: string | null;
  warning: string | null;
  skippedModels: string[];
  /** Pre-supported models whose supports a resupported VOXL plate reads (0 until it reaches them). */
  supportsTotal: number;
  supportsRead: number;
  /** Model meshes a remeshed, resupported VOXL plate repairs (0 until it reaches them). */
  repairsTotal: number;
  repairsDone: number;
}

export async function generatePlate(
  placements: PlatePlacement[],
  format: PlateFormat = 'voxl',
  options: PlateOptions = {},
): Promise<PlateGenerationResult> {
  const r = await apiFetch('/api/plate/generate', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ placements, format, ...options }),
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
  format: PlateFormat = 'voxl',
  options: PlateOptions = {},
): Promise<PlateGenerationJob> {
  const r = await apiFetch('/api/plate/jobs', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ placements, format, ...options }),
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

/**
 * `original`: the model's mesh as its file holds it. `resupported`: the model without its
 * supports, with their layout as DragonFruit supports. `remeshed`: the resupported VOXL with the
 * model mesh made watertight (the supports are the same).
 */
export type ModelVoxlVariant = 'original' | 'resupported' | 'remeshed';

export function getModelVoxlUrl(id: string, variant: ModelVoxlVariant = 'original'): string {
  return apiUrl(`/api/models/${id}/voxl${variant === 'original' ? '' : `?variant=${variant}`}`);
}

export function getPlateGenerationDownloadUrl(jobId: string): string {
  return apiUrl(`/api/plate/jobs/${jobId}/file`);
}
