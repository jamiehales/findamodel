import { useQuery, useSuspenseQuery } from '@tanstack/react-query'
import { fetchModels, fetchGeometry } from './api'

export const queryKeys = {
  models: ['models'] as const,
  geometry: (id: string) => ['geometry', id] as const,
}

export function useModels() {
  return useQuery({
    queryKey: queryKeys.models,
    queryFn: fetchModels,
  })
}

export function useModel(id: string) {
  return useQuery({
    queryKey: queryKeys.models,
    queryFn: fetchModels,
    select: models => models.find(m => m.id === id) ?? null,
  })
}

export function useGeometry(id: string) {
  return useSuspenseQuery({
    queryKey: queryKeys.geometry(id),
    queryFn: () => fetchGeometry(id),
  })
}
