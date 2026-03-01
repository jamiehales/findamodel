export interface Model {
  id: string
  name: string
  relativePath: string
  fileType: string
  fileSize: number
  fileUrl: string
  hasPreview: boolean
  previewUrl: string | null
  creator: string | null
  collection: string | null
  subcollection: string | null
  category: string | null
  type: string | null
  supported: boolean | null
  convexHull: string | null
  concaveHull: string | null
  convexSansRaftHull: string | null
  dimensionXMm: number | null
  dimensionYMm: number | null
  dimensionZMm: number | null
  sphereCentreX: number | null
  sphereCentreY: number | null
  sphereCentreZ: number | null
  sphereRadius: number | null
}

export async function fetchModels(limit?: number): Promise<Model[]> {
  const url = limit !== undefined ? `/api/models?limit=${limit}` : '/api/models'
  const r = await fetch(url)
  if (!r.ok) throw new Error('Failed to fetch models')
  return r.json()
}

export async function fetchModel(id: string): Promise<Model> {
  const r = await fetch(`/api/models/${id}`)
  if (!r.ok) throw new Error(`Failed to fetch model ${id}`)
  return r.json()
}

export interface GeometryResponse {
  positions: number[]
  normals: number[]
  triangleCount: number
  sphereRadius: number
  sphereCentre: { x: number; y: number; z: number }
  dimensionXMm: number
  dimensionYMm: number
  dimensionZMm: number
}

export async function fetchGeometry(id: string): Promise<GeometryResponse> {
  const r = await fetch(`/api/models/${id}/geometry`)
  if (!r.ok) throw new Error('Failed to fetch geometry')
  return r.json()
}

export interface PlatePlacement {
  modelId: string
  instanceIndex: number
  xMm: number
  yMm: number
  angleRad: number
}

// ---- Explorer ----

export interface MetadataFields {
  creator: string | null
  collection: string | null
  subcollection: string | null
  category: string | null
  type: string | null
  supported: boolean | null
}

export interface ExplorerFolder {
  name: string
  path: string
  subdirectoryCount: number
  modelCount: number
  resolvedValues: MetadataFields
}

export interface ExplorerModel {
  id: string | null
  fileName: string
  relativePath: string
  fileType: string
  fileSize: number | null
  hasPreview: boolean
  previewUrl: string | null
}

export interface ExplorerResponse {
  currentPath: string
  parentPath: string | null
  folders: ExplorerFolder[]
  models: ExplorerModel[]
}

export interface DirectoryConfigDetail {
  directoryPath: string
  localValues: MetadataFields
  parentResolvedValues: MetadataFields | null
  parentPath: string | null
}

export async function fetchExplorer(path: string): Promise<ExplorerResponse> {
  const url = `/api/explorer?path=${encodeURIComponent(path)}`
  const r = await fetch(url)
  if (!r.ok) throw new Error(`Failed to fetch explorer at path: ${path}`)
  return r.json()
}

export async function fetchDirectoryConfig(path: string): Promise<DirectoryConfigDetail> {
  const r = await fetch(`/api/explorer/config?path=${encodeURIComponent(path)}`)
  if (!r.ok) throw new Error(`Failed to fetch config for: ${path}`)
  return r.json()
}

export async function updateDirectoryConfig(
  path: string,
  fields: MetadataFields
): Promise<DirectoryConfigDetail> {
  const r = await fetch(`/api/explorer/config?path=${encodeURIComponent(path)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(fields),
  })
  if (!r.ok) throw new Error(`Failed to update config for: ${path}`)
  return r.json()
}

// ---- Indexer ----

export const IndexFlags = { Directories: 1, Models: 2, Hulls: 4, All: 7 } as const

export interface IndexRequest {
  id: string
  directoryFilter: string | null
  flags: number
  requestedAt: string
  status: 'queued' | 'running'
}

export interface IndexerStatus {
  isRunning: boolean
  currentRequest: IndexRequest | null
  queue: IndexRequest[]
}

export async function fetchIndexerStatus(): Promise<IndexerStatus> {
  const r = await fetch('/api/indexer')
  if (!r.ok) throw new Error('Failed to fetch indexer status')
  return r.json()
}

export async function enqueueIndex(directoryFilter: string | null, flags: number): Promise<IndexRequest> {
  const r = await fetch('/api/indexer', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ directoryFilter, flags }),
  })
  if (!r.ok) throw new Error('Failed to enqueue index request')
  return r.json()
}

export async function generatePlate(placements: PlatePlacement[]): Promise<Blob> {
  const r = await fetch('/api/plate/generate', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ placements }),
  })
  if (!r.ok) throw new Error('Failed to generate plate')
  return r.blob()
}

// ---- Printing Lists ----

export interface PrintingListSummary {
  id: string
  name: string
  isActive: boolean
  isDefault: boolean
  createdAt: string
  ownerUsername: string | null
  itemCount: number
}

export interface PrintingListItem {
  id: string
  modelId: string
  quantity: number
}

export interface PrintingListDetail {
  id: string
  name: string
  isActive: boolean
  isDefault: boolean
  createdAt: string
  ownerUsername: string | null
  items: PrintingListItem[]
}

export async function fetchPrintingLists(): Promise<PrintingListSummary[]> {
  const r = await fetch('/api/printing-lists')
  if (!r.ok) throw new Error('Failed to fetch printing lists')
  return r.json()
}

export async function fetchActivePrintingList(): Promise<PrintingListDetail | null> {
  const r = await fetch('/api/printing-lists/active')
  if (r.status === 204) return null
  if (!r.ok) throw new Error('Failed to fetch active printing list')
  return r.json()
}

export async function fetchPrintingList(id: string): Promise<PrintingListDetail> {
  const r = await fetch(`/api/printing-lists/${id}`)
  if (!r.ok) throw new Error(`Failed to fetch printing list ${id}`)
  return r.json()
}

export async function createPrintingList(name: string): Promise<PrintingListSummary> {
  const r = await fetch('/api/printing-lists', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name }),
  })
  if (!r.ok) throw new Error('Failed to create printing list')
  return r.json()
}

export async function renamePrintingList(id: string, name: string): Promise<PrintingListSummary> {
  const r = await fetch(`/api/printing-lists/${id}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name }),
  })
  if (!r.ok) throw new Error('Failed to rename printing list')
  return r.json()
}

export async function deletePrintingList(id: string): Promise<void> {
  const r = await fetch(`/api/printing-lists/${id}`, { method: 'DELETE' })
  if (!r.ok) throw new Error('Failed to delete printing list')
}

export async function activatePrintingList(id: string): Promise<void> {
  const r = await fetch(`/api/printing-lists/${id}/activate`, { method: 'POST' })
  if (!r.ok) throw new Error('Failed to activate printing list')
}

export async function upsertPrintingListItem(listId: string, modelId: string, quantity: number): Promise<PrintingListDetail> {
  const r = await fetch(`/api/printing-lists/${listId}/items/${modelId}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ quantity }),
  })
  if (!r.ok) throw new Error('Failed to update printing list item')
  return r.json()
}

export async function clearPrintingListItems(listId: string): Promise<PrintingListDetail> {
  const r = await fetch(`/api/printing-lists/${listId}/items`, { method: 'DELETE' })
  if (!r.ok) throw new Error('Failed to clear printing list items')
  return r.json()
}
