import { useQuery, useSuspenseQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import {
  fetchModels,
  fetchModelsByIds,
  fetchModel,
  fetchModelMetadata,
  updateModelMetadata,
  fetchGeometry,
  fetchSplitGeometry,
  createAutoSupportJob,
  fetchAutoSupportJob,
  fetchAutoSupportGeometry,
  createAutoSupportSettingsPreview,
  fetchAutoSupportSettingsPreviewGeometry,
  fetchOtherParts,
  fetchExplorer,
  fetchDirectoryConfig,
  fetchExplorerFileText,
  updateDirectoryConfig,
  fetchIndexerStatus,
  fetchIndexerRuns,
  fetchIndexerRun,
  cancelIndexerRun,
  enqueueIndex,
  IndexFlags,
  type IndexRunFilesView,
  fetchPrintingLists,
  fetchActivePrintingList,
  fetchPrintingList,
  createPrintingList,
  renamePrintingList,
  deletePrintingList,
  activatePrintingList,
  updatePrintingListSettings,
  updatePrintingListPrinter,
  upsertPrintingListItem,
  clearPrintingListItems,
  fetchPrinters,
  createPrinter,
  updatePrinter,
  deletePrinter,
  setDefaultPrinter,
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
  fetchSetupStatus,
  fetchInitialSetupDefaults,
  completeInitialSetup,
  fetchApplicationLogs,
  fetchInstanceStats,
  type ApplicationLogsResponse,
  type SetupStatus,
  type InitialSetupDefaults,
  type InitialSetupRequest,
  type InstanceStats,
  type UpdateAppConfigRequest,
  type AutoSupportSettingsPreviewTuningRequest,
  type UpdateModelMetadataRequest,
} from './api';

export const queryKeys = {
  models: (limit?: number) =>
    limit !== undefined ? (['models', limit] as const) : (['models'] as const),
  modelsByIds: (ids: string[]) => ['models', 'by-ids', [...ids].sort()] as const,
  model: (id: string) => ['model', id] as const,
  modelMetadata: (id: string) => ['model', id, 'metadata'] as const,
  modelOtherParts: (id: string) => ['model', id, 'other-parts'] as const,
  geometry: (id: string) => ['geometry', id] as const,
  splitGeometry: (id: string) => ['split-geometry', id] as const,
  autoSupportJob: (id: string, jobId: string) => ['auto-support-job', id, jobId] as const,
  autoSupportGeometry: (id: string, jobId: string) => ['auto-support-geometry', id, jobId] as const,
  autoSupportSettingsPreviewGeometry: (previewId: string, scenarioId: string) =>
    ['auto-support-settings-preview-geometry', previewId, scenarioId] as const,
  explorerDir: (path: string) => ['explorer', 'dir', path] as const,
  explorerConfig: (path: string) => ['explorer', 'config', path] as const,
  explorerFileText: (path: string) => ['explorer', 'file-text', path] as const,
  indexerStatus: ['indexer', 'status'] as const,
  indexerRuns: (days: number) => ['indexer', 'runs', days] as const,
  indexerRun: (
    runId: string,
    filesPage: number,
    filesPageSize: number,
    filesView: IndexRunFilesView,
    eventsPage: number,
    eventsPageSize: number,
  ) =>
    [
      'indexer',
      'run',
      runId,
      filesPage,
      filesPageSize,
      filesView,
      eventsPage,
      eventsPageSize,
    ] as const,
  printingLists: ['printing-lists'] as const,
  activePrintingList: ['printing-lists', 'active'] as const,
  printingList: (id: string) => ['printing-lists', id] as const,
  queryModels: (filter: ModelFilter, limit: number, offset: number, modelName?: string) =>
    ['query', 'models', filter, limit, offset, modelName ?? ''] as const,
  modelNameOptions: (filter: ModelFilter, limit: number, modelNameInput?: string) =>
    ['query', 'model-name-options', filter, limit, modelNameInput ?? ''] as const,
  filterOptions: (filter: ModelFilter, modelName?: string) =>
    ['query', 'options', filter, modelName ?? ''] as const,
  metadataDictionaryOverview: ['settings', 'metadata-dictionary'] as const,
  appConfig: ['settings', 'config'] as const,
  setupStatus: ['settings', 'setup-status'] as const,
  setupDefaults: ['settings', 'setup-defaults'] as const,
  instanceStats: ['settings', 'stats'] as const,
  applicationLogs: (channel: string, severity: string, limit: number) =>
    ['settings', 'logs', channel, severity, limit] as const,
  printers: ['settings', 'printers'] as const,
};

export function useModels(limit?: number) {
  return useQuery({
    queryKey: queryKeys.models(limit),
    queryFn: () => fetchModels(limit),
  });
}

export function useModelsByIds(ids: string[]) {
  return useQuery({
    queryKey: queryKeys.modelsByIds(ids),
    queryFn: () => fetchModelsByIds(ids),
    enabled: ids.length > 0,
  });
}

export function useModel(id: string, enabled: boolean = true) {
  return useQuery({
    queryKey: queryKeys.model(id),
    queryFn: () => fetchModel(id),
    enabled,
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

export function useGenerateAutoSupportJob(id: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => createAutoSupportJob(id),
    onSuccess: (job) => {
      queryClient.setQueryData(queryKeys.autoSupportJob(id, job.jobId), job);
    },
  });
}

export function useAutoSupportJob(id: string, jobId: string | null, enabled: boolean = true) {
  return useQuery({
    queryKey: queryKeys.autoSupportJob(id, jobId ?? 'pending'),
    queryFn: () => fetchAutoSupportJob(id, jobId!),
    enabled: !!id && !!jobId && enabled,
    refetchInterval: (query) => {
      const status = query.state.data?.status;
      return status === 'completed' || status === 'failed' ? false : 1000;
    },
  });
}

export function useAutoSupportGeometry(id: string, jobId: string | null, enabled: boolean = true) {
  return useQuery({
    queryKey: queryKeys.autoSupportGeometry(id, jobId ?? 'pending'),
    queryFn: () => fetchAutoSupportGeometry(id, jobId!),
    enabled: !!id && !!jobId && enabled,
  });
}

export function useCreateAutoSupportSettingsPreview() {
  return useMutation({
    mutationFn: (request: {
      tuning: AutoSupportSettingsPreviewTuningRequest;
      scenarioId?: string;
    }) => createAutoSupportSettingsPreview(request),
  });
}

export function useAutoSupportSettingsPreviewGeometry(
  previewId: string | null,
  scenarioId: string,
  enabled: boolean = true,
) {
  return useQuery({
    queryKey: queryKeys.autoSupportSettingsPreviewGeometry(previewId ?? 'pending', scenarioId),
    queryFn: () => fetchAutoSupportSettingsPreviewGeometry(previewId!, scenarioId),
    enabled: !!previewId && !!scenarioId && enabled,
  });
}

export function useModelOtherParts(id: string) {
  return useQuery({
    queryKey: queryKeys.modelOtherParts(id),
    queryFn: () => fetchOtherParts(id),
    enabled: !!id,
  });
}

export function useQueryModels(
  filter: ModelFilter,
  limit: number,
  offset: number,
  modelName?: string,
) {
  return useQuery({
    queryKey: queryKeys.queryModels(filter, limit, offset, modelName),
    queryFn: () => fetchQueryModels(filter, limit, offset, modelName),
    placeholderData: (previousData) => previousData,
  });
}

export function useModelNameOptions(
  filter: ModelFilter,
  limit: number = 50,
  modelNameInput?: string,
) {
  return useQuery({
    queryKey: queryKeys.modelNameOptions(filter, limit, modelNameInput),
    queryFn: async () => {
      const result = await fetchQueryModels(filter, limit, 0, modelNameInput);
      return Array.from(new Set(result.models.map((model) => model.name)));
    },
    staleTime: 30 * 1000,
    placeholderData: (previousData) => previousData,
  });
}

export function useFilterOptions(filter: ModelFilter, modelName?: string) {
  return useQuery({
    queryKey: queryKeys.filterOptions(filter, modelName),
    queryFn: () => fetchFilterOptions(filter, modelName),
    staleTime: 5 * 60 * 1000,
    placeholderData: (previousData) => previousData,
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

export function useSetupStatus() {
  return useQuery<SetupStatus>({
    queryKey: queryKeys.setupStatus,
    queryFn: fetchSetupStatus,
    refetchOnWindowFocus: false,
  });
}

export function useInitialSetupDefaults(enabled: boolean) {
  return useQuery<InitialSetupDefaults>({
    queryKey: queryKeys.setupDefaults,
    queryFn: fetchInitialSetupDefaults,
    enabled,
  });
}

export function useApplicationLogs(channel: string, severity: string, limit: number) {
  return useQuery<ApplicationLogsResponse>({
    queryKey: queryKeys.applicationLogs(channel, severity, limit),
    queryFn: () =>
      fetchApplicationLogs({
        channel: channel || undefined,
        severity: severity || undefined,
        limit,
      }),
    refetchInterval: 5000,
    placeholderData: (previousData) => previousData,
  });
}

export function useInstanceStats() {
  return useQuery<InstanceStats>({
    queryKey: queryKeys.instanceStats,
    queryFn: fetchInstanceStats,
    staleTime: 60 * 1000,
  });
}

export function useUpdateAppConfig() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (request: UpdateAppConfigRequest) => updateAppConfig(request),
    onSuccess: (updated) => {
      queryClient.setQueryData(queryKeys.appConfig, updated);
      queryClient.invalidateQueries({ queryKey: ['models'] });
      queryClient.invalidateQueries({ queryKey: ['model'] });
      queryClient.invalidateQueries({ queryKey: ['query'] });
      queryClient.invalidateQueries({ queryKey: ['explorer'] });
    },
  });
}

export function useCompleteInitialSetup() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (request: InitialSetupRequest) => completeInitialSetup(request),
    onSuccess: (updated) => {
      queryClient.setQueryData(queryKeys.appConfig, updated);
      queryClient.invalidateQueries({ queryKey: queryKeys.setupStatus });
      queryClient.invalidateQueries({ queryKey: ['models'] });
      queryClient.invalidateQueries({ queryKey: ['model'] });
      queryClient.invalidateQueries({ queryKey: ['query'] });
      queryClient.invalidateQueries({ queryKey: ['explorer'] });
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

type IndexerPollingOptions = {
  adaptivePolling?: boolean;
};

function adaptiveIndexerInterval(options?: IndexerPollingOptions): number {
  if (!options?.adaptivePolling) return 5000;
  if (typeof document === 'undefined') return 3000;
  return document.visibilityState === 'visible' ? 3000 : 30000;
}

export function useIndexerStatus(options?: IndexerPollingOptions) {
  return useQuery({
    queryKey: queryKeys.indexerStatus,
    queryFn: fetchIndexerStatus,
    refetchInterval: () => adaptiveIndexerInterval(options),
    refetchIntervalInBackground: true,
  });
}

export function useIndexerRuns(days: number = 7, options?: IndexerPollingOptions) {
  return useQuery({
    queryKey: queryKeys.indexerRuns(days),
    queryFn: () => fetchIndexerRuns(days),
    refetchInterval: () => adaptiveIndexerInterval(options),
    refetchIntervalInBackground: true,
  });
}

export function useIndexerRun(
  runId: string | null | undefined,
  paging: {
    filesPage: number;
    filesPageSize: number;
    filesView: IndexRunFilesView;
    eventsPage: number;
    eventsPageSize: number;
  },
  options?: IndexerPollingOptions,
) {
  return useQuery({
    queryKey: queryKeys.indexerRun(
      runId ?? 'none',
      paging.filesPage,
      paging.filesPageSize,
      paging.filesView,
      paging.eventsPage,
      paging.eventsPageSize,
    ),
    queryFn: () =>
      fetchIndexerRun(
        runId ?? '',
        paging.filesPage,
        paging.filesPageSize,
        paging.filesView,
        paging.eventsPage,
        paging.eventsPageSize,
      ),
    enabled: !!runId,
    refetchInterval: () => adaptiveIndexerInterval(options),
    refetchIntervalInBackground: true,
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

export function useCancelIndexerRun(days: number = 7) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (runId: string) => cancelIndexerRun(runId),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.indexerStatus });
      queryClient.invalidateQueries({ queryKey: queryKeys.indexerRuns(days) });
      queryClient.invalidateQueries({ queryKey: ['indexer', 'run'] });
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

export function useUpdatePrintingListPrinter() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ id, printerConfigId }: { id: string; printerConfigId: string | null }) =>
      updatePrintingListPrinter(id, printerConfigId),
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

export function usePrinters() {
  return useQuery({
    queryKey: queryKeys.printers,
    queryFn: fetchPrinters,
  });
}

export function useCreatePrinter() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: createPrinter,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.printers });
    },
  });
}

export function useUpdatePrinter() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      id,
      name,
      bedWidthMm,
      bedDepthMm,
      pixelWidth,
      pixelHeight,
    }: {
      id: string;
      name: string;
      bedWidthMm: number;
      bedDepthMm: number;
      pixelWidth: number;
      pixelHeight: number;
    }) => updatePrinter(id, { name, bedWidthMm, bedDepthMm, pixelWidth, pixelHeight }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.printers });
      queryClient.invalidateQueries({ queryKey: queryKeys.printingLists });
      queryClient.invalidateQueries({ queryKey: queryKeys.activePrintingList });
    },
  });
}

export function useDeletePrinter() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => deletePrinter(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.printers });
      queryClient.invalidateQueries({ queryKey: queryKeys.printingLists });
      queryClient.invalidateQueries({ queryKey: queryKeys.activePrintingList });
    },
  });
}

export function useSetDefaultPrinter() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (id: string) => setDefaultPrinter(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: queryKeys.printers });
      queryClient.invalidateQueries({ queryKey: queryKeys.printingLists });
      queryClient.invalidateQueries({ queryKey: queryKeys.activePrintingList });
    },
  });
}
