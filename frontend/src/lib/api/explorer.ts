import type { CommonMetadataFields } from './metadata';
import { apiFetch, apiUrl } from '../http';

export interface MetadataFields extends CommonMetadataFields {
  /** When provided, fully replaces the rule set for this directory. Maps YAML field name
   *  (e.g. "creator", "model_name") to inner rule YAML (e.g. "rule: filename\nindex: -2"). */
  fieldRules?: Record<string, string> | null;
}

export interface ExplorerFolder {
  name: string;
  path: string;
  subdirectoryCount: number;
  modelCount: number;
  resolvedValues: MetadataFields;
  ruleConfigs: Record<string, string> | null;
  localValues: MetadataFields | null;
  localRuleFields: string[] | null;
}

export interface ExplorerModel {
  id: string | null;
  fileName: string;
  relativePath: string;
  fileType: string;
  fileSize: number | null;
  hasPreview: boolean;
  previewUrl: string | null;
  resolvedMetadata: MetadataFields | null;
  ruleConfigs: Record<string, string> | null;
}

export interface ExplorerFile {
  fileName: string;
  relativePath: string;
  fileType: string;
  fileSize: number;
}

export interface ExplorerResponse {
  currentPath: string;
  parentPath: string | null;
  folders: ExplorerFolder[];
  models: ExplorerModel[];
  files: ExplorerFile[];
}

function mapExplorerModelUrls(model: ExplorerModel): ExplorerModel {
  return {
    ...model,
    previewUrl: model.previewUrl ? apiUrl(model.previewUrl) : null,
  };
}

export function explorerFileUrl(relativePath: string): string {
  return apiUrl(`/api/explorer/file?path=${encodeURIComponent(relativePath)}`);
}

export async function fetchExplorerFileText(relativePath: string): Promise<string> {
  const r = await apiFetch(`/api/explorer/file?path=${encodeURIComponent(relativePath)}`);
  if (!r.ok) throw new Error('Failed to load text preview');
  return r.text();
}

export interface DirectoryConfigDetail {
  directoryPath: string;
  localValues: MetadataFields;
  parentResolvedValues: MetadataFields | null;
  parentPath: string | null;
  localRuleFields: string[] | null;
  /** Maps each rule field name to its inner YAML content for editing
   *  (e.g. "rule: filename\nindex: -2", without the outer field key wrapper). */
  localRuleContents: Record<string, string> | null;
  /** Maps inherited rule field names to their YAML snippets from parent. */
  parentResolvedRules: Record<string, string> | null;
}

export interface MetadataDictionaryValue {
  id: string;
  value: string;
}

export interface MetadataDictionaryField {
  configured: MetadataDictionaryValue[];
  observed: string[];
}

export interface MetadataDictionaryOverview {
  category: MetadataDictionaryField;
  type: MetadataDictionaryField;
  material: MetadataDictionaryField;
  tags: MetadataDictionaryField;
}

export interface AppConfig {
  defaultRaftHeightMm: number;
  theme: string;
  tagGenerationEnabled: boolean;
  tagGenerationProvider: string;
  tagGenerationEndpoint: string;
  tagGenerationModel: string;
  tagGenerationTimeoutMs: number;
  tagGenerationMaxTags: number;
  tagGenerationMinConfidence: number;
}

export interface UpdateAppConfigRequest {
  defaultRaftHeightMm: number;
  theme: string;
  tagGenerationEnabled: boolean;
  tagGenerationProvider: string;
  tagGenerationEndpoint: string;
  tagGenerationModel: string;
  tagGenerationTimeoutMs: number;
  tagGenerationMaxTags: number;
  tagGenerationMinConfidence: number;
}

export async function fetchExplorer(path: string): Promise<ExplorerResponse> {
  const url = `/api/explorer?path=${encodeURIComponent(path)}`;
  const r = await apiFetch(url);
  if (!r.ok) throw new Error(`Failed to fetch explorer at path: ${path}`);
  const response = (await r.json()) as ExplorerResponse;
  return {
    ...response,
    models: response.models.map(mapExplorerModelUrls),
  };
}

export async function fetchDirectoryConfig(path: string): Promise<DirectoryConfigDetail> {
  const r = await apiFetch(`/api/explorer/config?path=${encodeURIComponent(path)}`);
  if (!r.ok) throw new Error(`Failed to fetch config for: ${path}`);
  return r.json();
}

export class ConfigValidationError extends Error {
  constructor(public readonly fieldErrors: Record<string, string>) {
    super('Config validation failed');
  }
}

export async function updateDirectoryConfig(
  path: string,
  fields: MetadataFields,
): Promise<DirectoryConfigDetail> {
  const r = await apiFetch(`/api/explorer/config?path=${encodeURIComponent(path)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(fields),
  });
  if (r.status === 422) {
    const body = await r.json();
    throw new ConfigValidationError(body.fieldErrors ?? {});
  }
  if (!r.ok) throw new Error(`Failed to update config for: ${path}`);
  return r.json();
}

export async function fetchMetadataDictionaryOverview(): Promise<MetadataDictionaryOverview> {
  const r = await apiFetch('/api/settings/metadata-dictionary');
  if (!r.ok) throw new Error('Failed to fetch metadata dictionary settings');
  return r.json();
}

export async function fetchAppConfig(): Promise<AppConfig> {
  const r = await apiFetch('/api/settings/config');
  if (!r.ok) throw new Error('Failed to fetch app settings');
  return r.json();
}

export async function updateAppConfig(request: UpdateAppConfigRequest): Promise<AppConfig> {
  const r = await apiFetch('/api/settings/config', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  });
  if (!r.ok) throw new Error('Failed to update app settings');
  return r.json();
}

export async function createMetadataDictionaryValue(
  field: 'category' | 'type' | 'material' | 'tags',
  value: string,
): Promise<MetadataDictionaryValue> {
  const r = await apiFetch('/api/settings/metadata-dictionary', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ field, value }),
  });
  if (!r.ok) throw new Error('Failed to create metadata dictionary value');
  return r.json();
}

export async function updateMetadataDictionaryValue(
  id: string,
  value: string,
): Promise<MetadataDictionaryValue> {
  const r = await apiFetch(`/api/settings/metadata-dictionary/${encodeURIComponent(id)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ value }),
  });
  if (!r.ok) throw new Error('Failed to update metadata dictionary value');
  return r.json();
}

export async function deleteMetadataDictionaryValue(id: string): Promise<void> {
  const r = await apiFetch(`/api/settings/metadata-dictionary/${encodeURIComponent(id)}`, {
    method: 'DELETE',
  });
  if (!r.ok) throw new Error('Failed to delete metadata dictionary value');
}
