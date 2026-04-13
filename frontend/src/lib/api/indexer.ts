import { apiFetch } from '../http';

export const IndexFlags = { Directories: 1, Models: 2, Hulls: 4, All: 7 } as const;

export interface IndexRequest {
  id: string;
  runId: string | null;
  directoryFilter: string | null;
  relativeModelPath: string | null;
  flags: number;
  requestedAt: string;
  totalFiles: number | null;
  processedFiles: number;
  status: 'queued' | 'running';
}

export interface CompletedIndexRequest {
  id: string;
  runId: string | null;
  directoryFilter: string | null;
  relativeModelPath: string | null;
  flags: number;
  requestedAt: string;
  startedAt: string;
  completedAt: string;
  durationMs: number;
  outcome: 'success' | 'failed' | 'cancelled';
  error: string | null;
}

export interface IndexerStatus {
  isRunning: boolean;
  currentRequest: IndexRequest | null;
  queue: IndexRequest[];
  recent: CompletedIndexRequest[];
}

export interface IndexRunSummary {
  id: string;
  directoryFilter: string | null;
  relativeModelPath: string | null;
  flags: number;
  requestedAt: string;
  startedAt: string | null;
  completedAt: string | null;
  totalFiles: number | null;
  processedFiles: number;
  status: 'queued' | 'running' | 'success' | 'failed' | 'cancelled';
  outcome: 'success' | 'failed' | 'cancelled' | null;
  error: string | null;
}

export interface PagedResult<T> {
  items: T[];
  page: number;
  pageSize: number;
  totalCount: number;
}

export interface IndexRunFile {
  relativePath: string;
  fileType: string;
  status: 'pending' | 'processed' | 'skipped' | 'failed';
  isNew: boolean;
  wasUpdated: boolean;
  generatedPreview: boolean;
  generatedHull: boolean;
  generatedAiTags: boolean;
  generatedAiDescription: boolean;
  aiGenerationReason: string | null;
  message: string | null;
  durationMs: number | null;
  processedAt: string | null;
}

export interface IndexRunEvent {
  createdAt: string;
  level: string;
  message: string;
  relativePath: string | null;
}

export interface IndexRunDetail {
  run: IndexRunSummary;
  files: PagedResult<IndexRunFile>;
  events: PagedResult<IndexRunEvent>;
}

export type IndexRunFilesView = 'all' | 'pending' | 'processed';

export async function fetchIndexerStatus(): Promise<IndexerStatus> {
  const r = await apiFetch('/api/indexer');
  if (!r.ok) throw new Error('Failed to fetch indexer status');
  return r.json();
}

export async function enqueueIndex(
  directoryFilter: string | null,
  flags: number,
  relativeModelPath: string | null = null,
): Promise<IndexRequest> {
  const r = await apiFetch('/api/indexer', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ directoryFilter, relativeModelPath, flags }),
  });
  if (!r.ok) throw new Error('Failed to enqueue index request');
  return r.json();
}

export async function fetchIndexerRuns(days: number = 7): Promise<IndexRunSummary[]> {
  const r = await apiFetch(`/api/indexer/runs?days=${days}`);
  if (!r.ok) throw new Error('Failed to fetch index run history');
  return r.json();
}

export async function fetchIndexerRun(
  runId: string,
  filesPage: number = 1,
  filesPageSize: number = 200,
  filesView: IndexRunFilesView = 'all',
  eventsPage: number = 1,
  eventsPageSize: number = 200,
): Promise<IndexRunDetail> {
  const params = new URLSearchParams({
    filesPage: String(filesPage),
    filesPageSize: String(filesPageSize),
    filesView,
    eventsPage: String(eventsPage),
    eventsPageSize: String(eventsPageSize),
  });
  const r = await apiFetch(`/api/indexer/runs/${encodeURIComponent(runId)}?${params.toString()}`);
  if (!r.ok) throw new Error('Failed to fetch index run detail');
  return r.json();
}

export async function cancelIndexerRun(runId: string): Promise<void> {
  const r = await apiFetch(`/api/indexer/runs/${encodeURIComponent(runId)}`, {
    method: 'DELETE',
  });

  if (!r.ok) throw new Error('Failed to cancel index run');
}
