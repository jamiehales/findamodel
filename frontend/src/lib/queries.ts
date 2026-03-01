import { useQuery, useSuspenseQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import {
  fetchModels, fetchModel, fetchGeometry,
  fetchExplorer, fetchDirectoryConfig, updateDirectoryConfig,
  fetchPrintingLists, fetchActivePrintingList, fetchPrintingList,
  createPrintingList, renamePrintingList, deletePrintingList, activatePrintingList,
  upsertPrintingListItem, clearPrintingListItems,
  type MetadataFields, type PrintingListDetail,
} from './api'

export const queryKeys = {
  models: (limit?: number) => limit !== undefined ? ['models', limit] as const : ['models'] as const,
  model: (id: string) => ['model', id] as const,
  geometry: (id: string) => ['geometry', id] as const,
  explorerDir: (path: string) => ['explorer', 'dir', path] as const,
  explorerConfig: (path: string) => ['explorer', 'config', path] as const,
  printingLists: ['printing-lists'] as const,
  activePrintingList: ['printing-lists', 'active'] as const,
  printingList: (id: string) => ['printing-lists', id] as const,
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

// ---- Printing Lists ----

export function usePrintingLists() {
  return useQuery({
    queryKey: queryKeys.printingLists,
    queryFn: fetchPrintingLists,
  })
}

export function useActivePrintingList() {
  return useQuery({
    queryKey: queryKeys.activePrintingList,
    queryFn: fetchActivePrintingList,
  })
}

export function usePrintingListDetail(id: string) {
  return useQuery({
    queryKey: queryKeys.printingList(id),
    queryFn: () => fetchPrintingList(id),
    enabled: !!id,
  })
}

function syncActiveList(queryClient: ReturnType<typeof useQueryClient>, updated: PrintingListDetail) {
  queryClient.setQueryData(queryKeys.activePrintingList, updated)
  queryClient.setQueryData(queryKeys.printingList(updated.id), updated)
}

export function useCreatePrintingList() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (name: string) => createPrintingList(name),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.printingLists })
      queryClient.invalidateQueries({ queryKey: queryKeys.activePrintingList })
    },
  })
}

export function useRenamePrintingList() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) => renamePrintingList(id, name),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.printingLists })
    },
  })
}

export function useDeletePrintingList() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => deletePrintingList(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.printingLists })
      queryClient.invalidateQueries({ queryKey: queryKeys.activePrintingList })
    },
  })
}

export function useActivatePrintingList() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (id: string) => activatePrintingList(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.printingLists })
      queryClient.invalidateQueries({ queryKey: queryKeys.activePrintingList })
    },
  })
}

export function useUpsertPrintingListItem(listId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ modelId, quantity }: { modelId: string; quantity: number }) =>
      upsertPrintingListItem(listId, modelId, quantity),
    onSuccess: (updated) => syncActiveList(queryClient, updated),
  })
}

export function useClearPrintingListItems(listId: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => clearPrintingListItems(listId),
    onSuccess: (updated) => syncActiveList(queryClient, updated),
  })
}
