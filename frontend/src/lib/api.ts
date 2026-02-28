export interface Model {
  id: string
  name: string
  relativePath: string
  fileType: string
  fileSize: number
  fileUrl: string
  hasPreview: boolean
  previewUrl: string | null
  author: string | null
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

export async function fetchModels(): Promise<Model[]> {
  const r = await fetch('/api/models')
  if (!r.ok) throw new Error('Failed to fetch models')
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
