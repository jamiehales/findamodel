import { useQuery, useSuspenseQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  fetchModels,
  fetchModel,
  fetchModelMetadata,
  updateModelMetadata,
  fetchGeometry,
  fetchSplitGeometry,
  fetchOtherParts,
  fetchExplorer,
  fetchDirectoryConfig,
  fetchExplorerFileText,
  updateDirectoryConfig,
  fetchIndexerStatus,
  enqueueIndex,
  IndexFlags,
  fetchPrintingLists,
  fetchActivePrintingList,
  fetchPrintingList,
  createPrintingList,
  renamePrintingList,
  deletePrintingList,
  activatePrintingList,
  updatePrintingListSettings,
  upsertPrintingListItem,
  clearPrintingListItems,
  type MetadataFields,
  type PrintingListDetail,
  type ModelFilter,
  type SpawnType,
  type HullMode,
  fetchQueryModels,
  fetchFilterOptions,
  fetchMetadataDictionaryOverview,
  createMetadataDictionaryValue,
  updateMetadataDictionaryValue,
  deleteMetadataDictionaryValue,
  fetchAppConfig,
  updateAppConfig,
  type UpdateAppConfigRequest,
  type UpdateModelMetadataRequest,
} from './api';

export const queryKeys = {
  models: (limit?: number) =>
    limit !== undefined ? (['models', limit] as const) : (['models'] as const),
  model: (id: string) => ['model', id] as const,
  modelMetadata: (id: string) => ['model', id, 'metadata'] as const,
  modelOtherParts: (id: string) => ['model', id, 'other-parts'] as const,
  geometry: (id: string) => ['geometry', id] as const,
  splitGeometry: (id: string) => ['split-geometry', id] as const,
  explorerDir: (path: string) => ['explorer', 'dir', path] as const,
  explorerConfig: (path: string) => ['explorer', 'config', path] as const,
  explorerFileText: (path: string) => ['explorer', 'file-text', path] as const,
  indexerStatus: ['indexer', 'status'] as const,
  printingLists: ['printing-lists'] as const,
  activePrintingList: ['printing-lists', 'active'] as const,
  printingList: (id: string) => ['printing-lists', id] as const,
  queryModels: (filter: ModelFilter, limit: number) => ['query', 'models', filter, limit] as const,
  filterOptions: ['query', 'options'] as const,
  metadataDictionaryOverview: ['settings', 'metadata-dictionary'] as const,
  appConfig: ['settings', 'config'] as const,
};

export function useModels(limit?: number) {
  return useQuery({
    queryKey: queryKeys.models(limit),
    queryFn: () => fetchModels(limit),
  });
}

export function useModel(id: string) {
  return useQuery({
    queryKey: queryKeys.model(id),
    queryFn: () => fetchModel(id),
  });
}

export function useUpdateModelMetadata(id: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (request: UpdateModelMetadataRequest) => updateModelMetadata(id, request),
    onSuccess: (updated) => {
      queryClient.setQueryData(queryKeys.model(id), updated);
      queryClient.invalidateQueries({ queryKey: queryKeys.modelMetadata(id) });
      queryClient.invalidateQueries({ queryKey: queryKeys.modelOtherParts(id) });
      queryClient.invalidateQueries({ queryKey: queryKeys.models() });
      queryClient.invalidateQueries({ queryKey: ['query', 'models'] });
    },
  });
}

export function useModelMetadata(id: string) {
  return useQuery({
    queryKey: queryKeys.modelMetadata(id),
    queryFn: () => fetchModelMetadata(id),
    enabled: !!id,
  });
}

export function useSuspenseModel(id: string) {
  return useSuspenseQuery({
    queryKey: queryKeys.model(id),
    queryFn: () => fetchModel(id),
  });
}

export function useGeometry(id: string, enabled: boolean = true) {
  return useQuery({
    queryKey: queryKeys.geometry(id),
    queryFn: () => fetchGeometry(id),
    enabled: !!id && enabled,
  });
}

export function useSplitGeometry(id: string, enabled: boolean = true) {
  return useQuery({
    queryKey: queryKeys.splitGeometry(id),
    queryFn: () => fetchSplitGeometry(id),
    enabled: !!id && enabled,
  });
}

export function useModelOtherParts(id: string) {
  return useQuery({
    queryKey: queryKeys.modelOtherParts(id),
    queryFn: () => fetchOtherParts(id),
    enabled: !!id,
  });
}

export function useQueryModels(filter: ModelFilter, limit: number) {
  return useQuery({
    queryKey: queryKeys.queryModels(filter, limit),
    queryFn: () => fetchQueryModels(filter, limit),
  });
}

export function useFilterOptions() {
  return useQuery({
    queryKey: queryKeys.filterOptions,
    queryFn: fetchFilterOptions,
    staleTime: 5 * 60 * 1000,
  });
}

export function useMetadataDictionaryOverview() {
  return useQuery({
    queryKey: queryKeys.metadataDictionaryOverview,
    queryFn: fetchMetadataDictionaryOverview,
  });
}

export function useAppConfig() {
  return useQuery({
    queryKey: queryKeys.appConfig,
    queryFn: fetchAppConfig,
  });
}

export function useUpdateAppConfig() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (request: UpdateAppConfigRequest) => updateAppConfig(request),
    onSuccess: (updated) => {
      queryClient.setQueryData(queryKeys.appConfig, updated);
    },
  });
}

export function useCreateMetadataDictionaryValue() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      field,
      value,
    }: {
      field: 'category' | 'type' | 'material' | 'tags';
      value: string;
    }) => createMetadataDictionaryValue(field, value),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.metadataDictionaryOverview });
    },
  });
}

export function useUpdateMetadataDictionaryValue() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, value }: { id: string; value: string }) =>
      updateMetadataDictionaryValue(id, value),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.metadataDictionaryOverview });
    },
  });
}

export function useDeleteMetadataDictionaryValue() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => deleteMetadataDictionaryValue(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.metadataDictionaryOverview });
    },
  });
}

export function useExplorer(path: string) {
  return useQuery({
    queryKey: queryKeys.explorerDir(path),
    queryFn: () => fetchExplorer(path),
  });
}

export function useDirectoryConfig(path: string) {
  return useQuery({
    queryKey: queryKeys.explorerConfig(path),
    queryFn: () => fetchDirectoryConfig(path),
  });
}

export function useExplorerFileText(relativePath: string, enabled: boolean) {
  return useQuery({
    queryKey: queryKeys.explorerFileText(relativePath),
    queryFn: () => fetchExplorerFileText(relativePath),
    enabled,
    staleTime: 5 * 60 * 1000,
  });
}

export function useUpdateDirectoryConfig(path: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (fields: MetadataFields) => updateDirectoryConfig(path, fields),
    onSuccess: (updated) => {
      // Update the config cache with the fresh response
      queryClient.setQueryData(queryKeys.explorerConfig(path), updated);
      // Invalidate the explorer dir (resolved values on folder cards may have changed)
      const parentPath = updated.parentPath ?? '';
      queryClient.invalidateQueries({ queryKey: queryKeys.explorerDir(parentPath) });
      // Also invalidate the dir we're in, so folder cards of its children update
      queryClient.invalidateQueries({ queryKey: queryKeys.explorerDir(path) });
    },
  });
}

// ---- Indexer ----

export function useIndexerStatus() {
  return useQuery({
    queryKey: queryKeys.indexerStatus,
    queryFn: fetchIndexerStatus,
    refetchInterval: 5000,
    refetchIntervalInBackground: false,
  });
}

export function useEnqueueIndex() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      directoryFilter,
      flags,
      relativeModelPath,
    }: {
      directoryFilter: string | null;
      flags: number;
      relativeModelPath?: string | null;
    }) => enqueueIndex(directoryFilter, flags, relativeModelPath ?? null),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.indexerStatus });
    },
  });
}

export function useIndexFolder(path: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => enqueueIndex(path || null, IndexFlags.Models, null),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.indexerStatus });
    },
  });
}

/**
 * Returns the indexing state for a specific folder path by reading from the live
 * indexer status. 'running' means it is the current active request; 'queued' means
 * it is waiting in the queue; null means it is not being indexed.
 */
export function useIsFolderIndexing(path: string): 'running' | 'queued' | null {
  const { data: status } = useIndexerStatus();
  if (!status) return null;

  const filter = path || null;
  if (
    status.currentRequest?.relativeModelPath == null &&
    status.currentRequest?.directoryFilter === filter
  )
    return 'running';
  if (status.queue.some((r) => r.relativeModelPath == null && r.directoryFilter === filter))
    return 'queued';
  return null;
}

export function useIndexModel(relativePath: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => enqueueIndex(null, IndexFlags.Models, relativePath),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.indexerStatus });
    },
  });
}

export function useIsModelIndexing(relativePath: string): 'running' | 'queued' | null {
  const { data: status } = useIndexerStatus();
  if (!status) return null;

  if (status.currentRequest?.relativeModelPath === relativePath) return 'running';
  if (status.queue.some((r) => r.relativeModelPath === relativePath)) return 'queued';
  return null;
}

// ---- Printing Lists ----

export function usePrintingLists() {
  return useQuery({
    queryKey: queryKeys.printingLists,
    queryFn: fetchPrintingLists,
  });
}

export function useActivePrintingList() {
  return useQuery({
    queryKey: queryKeys.activePrintingList,
    queryFn: fetchActivePrintingList,
  });
}

export function usePrintingListDetail(id: string) {
  return useQuery({
    queryKey: id === 'active' ? queryKeys.activePrintingList : queryKeys.printingList(id),
    queryFn: () => (id === 'active' ? fetchActivePrintingList() : fetchPrintingList(id)),
    enabled: !!id,
  });
}

function syncList(queryClient: ReturnType<typeof useQueryClient>, updated: PrintingListDetail) {
  queryClient.setQueryData(queryKeys.printingList(updated.id), updated);
  if (updated.isActive) queryClient.setQueryData(queryKeys.activePrintingList, updated);
}

export function useCreatePrintingList() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => createPrintingList(name),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.printingLists });
      queryClient.invalidateQueries({ queryKey: queryKeys.activePrintingList });
    },
  });
}

export function useRenamePrintingList() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, name }: { id: string; name: string }) => renamePrintingList(id, name),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.printingLists });
    },
  });
}

export function useDeletePrintingList() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => deletePrintingList(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.printingLists });
      queryClient.invalidateQueries({ queryKey: queryKeys.activePrintingList });
    },
  });
}

export function useActivatePrintingList() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => activatePrintingList(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['printing-lists'] });
    },
  });
}

export function useUpdatePrintingListSettings() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      id,
      spawnType,
      hullMode,
    }: {
      id: string;
      spawnType: SpawnType;
      hullMode: HullMode;
    }) => updatePrintingListSettings(id, { spawnType, hullMode }),
    onSuccess: (updated) => syncList(queryClient, updated),
  });
}

export function useUpsertPrintingListItem() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      listId,
      modelId,
      quantity,
    }: {
      listId: string;
      modelId: string;
      quantity: number;
    }) => upsertPrintingListItem(listId, modelId, quantity),
    onSuccess: (updated) => syncList(queryClient, updated),
  });
}

export function useClearPrintingListItems() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (listId: string) => clearPrintingListItems(listId),
    onSuccess: (updated) => syncList(queryClient, updated),
  });
}
