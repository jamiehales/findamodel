export interface Model {
  id: string
  name: string
  relativePath: string
  fileType: string
  fileSize: number
  fileUrl: string
  hasPreview: boolean
  previewUrl: string | null
}

export async function fetchModels(): Promise<Model[]> {
  const r = await fetch('/api/models')
  if (!r.ok) throw new Error('Failed to fetch models')
  return r.json()
}
