import { useQuery, useSuspenseQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import {
  fetchModels, fetchModel, fetchGeometry,
  fetchExplorer, fetchDirectoryConfig, updateDirectoryConfig,
  type MetadataFields,
} from './api'

export const queryKeys = {
  models: (limit?: number) => limit !== undefined ? ['models', limit] as const : ['models'] as const,
  model: (id: string) => ['model', id] as const,
  geometry: (id: string) => ['geometry', id] as const,
  explorerDir: (path: string) => ['explorer', 'dir', path] as const,
  explorerConfig: (path: string) => ['explorer', 'config', path] as const,
}

export function useModels(limit?: number) {
  return useQuery({
    queryKey: queryKeys.models(limit),
    queryFn: () => fetchModels(limit),
  })
}

export function useModel(id: string) {
  return useQuery({
    queryKey: queryKeys.model(id),
    queryFn: () => fetchModel(id),
  })
}

export function useSuspenseModel(id: string) {
  return useSuspenseQuery({
    queryKey: queryKeys.model(id),
    queryFn: () => fetchModel(id),
  })
}

export function useGeometry(id: string) {
  return useSuspenseQuery({
    queryKey: queryKeys.geometry(id),
    queryFn: () => fetchGeometry(id),
  })
}

export function useExplorer(path: string) {
  return useQuery({
    queryKey: queryKeys.explorerDir(path),
    queryFn: () => fetchExplorer(path),
  })
}

export function useDirectoryConfig(path: string) {
  return useQuery({
    queryKey: queryKeys.explorerConfig(path),
    queryFn: () => fetchDirectoryConfig(path),
  })
}

export function useUpdateDirectoryConfig(path: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (fields: MetadataFields) => updateDirectoryConfig(path, fields),
    onSuccess: (updated) => {
      // Update the config cache with the fresh response
      queryClient.setQueryData(queryKeys.explorerConfig(path), updated)
      // Invalidate the explorer dir (resolved values on folder cards may have changed)
      const parentPath = updated.parentPath ?? ''
      queryClient.invalidateQueries({ queryKey: queryKeys.explorerDir(parentPath) })
      // Also invalidate the dir we're in, so folder cards of its children update
      queryClient.invalidateQueries({ queryKey: queryKeys.explorerDir(path) })
    },
  })
}
