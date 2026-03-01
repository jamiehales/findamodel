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

export async function generatePlate(placements: PlatePlacement[]): Promise<Blob> {
  const r = await fetch('/api/plate/generate', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ placements }),
  })
  if (!r.ok) throw new Error('Failed to generate plate')
  return r.blob()
}
