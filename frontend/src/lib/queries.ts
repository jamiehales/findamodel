import { useQuery, useSuspenseQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import {
  fetchModels, fetchModel, fetchGeometry,
  fetchExplorer, fetchDirectoryConfig, updateDirectoryConfig,
  fetchIndexerStatus, enqueueIndex, IndexFlags,
  fetchPrintingLists, fetchActivePrintingList, fetchPrintingList,
  createPrintingList, renamePrintingList, deletePrintingList, activatePrintingList,
  upsertPrintingListItem, clearPrintingListItems,
  fetchQueryModels, fetchFilterOptions,
  type MetadataFields, type PrintingListDetail, type ModelFilter,
} from './api'

export const queryKeys = {
  models: (limit?: number) => limit !== undefined ? ['models', limit] as const : ['models'] as const,
  model: (id: string) => ['model', id] as const,
  geometry: (id: string) => ['geometry', id] as const,
  explorerDir: (path: string) => ['explorer', 'dir', path] as const,
  explorerConfig: (path: string) => ['explorer', 'config', path] as const,
  indexerStatus: ['indexer', 'status'] as const,
  printingLists: ['printing-lists'] as const,
  activePrintingList: ['printing-lists', 'active'] as const,
  printingList: (id: string) => ['printing-lists', id] as const,
  queryModels: (filter: ModelFilter, limit: number) => ['query', 'models', filter, limit] as const,
  filterOptions: ['query', 'options'] as const,
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

export function useQueryModels(filter: ModelFilter, limit: number) {
  return useQuery({
    queryKey: queryKeys.queryModels(filter, limit),
    queryFn: () => fetchQueryModels(filter, limit),
  })
}

export function useFilterOptions() {
  return useQuery({
    queryKey: queryKeys.filterOptions,
    queryFn: fetchFilterOptions,
    staleTime: 5 * 60 * 1000,
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

// ---- Indexer ----

export function useIndexerStatus() {
  return useQuery({
    queryKey: queryKeys.indexerStatus,
    queryFn: fetchIndexerStatus,
    refetchInterval: 30000,
    refetchIntervalInBackground: false,
  })
}

export function useEnqueueIndex() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ directoryFilter, flags }: { directoryFilter: string | null; flags: number }) =>
      enqueueIndex(directoryFilter, flags),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.indexerStatus })
    },
  })
}

export function useIndexFolder(path: string) {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: () => enqueueIndex(path || null, IndexFlags.Models),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.indexerStatus })
    },
  })
}

/**
 * Returns the indexing state for a specific folder path by reading from the live
 * indexer status. 'running' means it is the current active request; 'queued' means
 * it is waiting in the queue; null means it is not being indexed.
 */
export function useIsFolderIndexing(path: string): 'running' | 'queued' | null {
  const { data: status } = useIndexerStatus()
  if (!status) return null

  const filter = path || null
  if (status.currentRequest?.directoryFilter === filter) return 'running'
  if (status.queue.some(r => r.directoryFilter === filter)) return 'queued'
  return null
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
    queryKey: id === 'active' ? queryKeys.activePrintingList : queryKeys.printingList(id),
    queryFn: () => id === 'active' ? fetchActivePrintingList() : fetchPrintingList(id),
    enabled: !!id,
  })
}

function syncList(queryClient: ReturnType<typeof useQueryClient>, updated: PrintingListDetail) {
  queryClient.setQueryData(queryKeys.printingList(updated.id), updated)
  if (updated.isActive) queryClient.setQueryData(queryKeys.activePrintingList, updated)
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
      queryClient.invalidateQueries({ queryKey: ['printing-lists'] })
    },
  })
}

export function useUpsertPrintingListItem() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: ({ listId, modelId, quantity }: { listId: string; modelId: string; quantity: number }) =>
      upsertPrintingListItem(listId, modelId, quantity),
    onSuccess: (updated) => syncList(queryClient, updated),
  })
}

export function useClearPrintingListItems() {
  const queryClient = useQueryClient()
  return useMutation({
    mutationFn: (listId: string) => clearPrintingListItems(listId),
    onSuccess: (updated) => syncList(queryClient, updated),
  })
}
