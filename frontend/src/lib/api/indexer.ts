export const IndexFlags = { Directories: 1, Models: 2, Hulls: 4, All: 7 } as const;

export interface IndexRequest {
  id: string;
  directoryFilter: string | null;
  relativeModelPath: string | null;
  flags: number;
  requestedAt: string;
  status: 'queued' | 'running';
}

export interface IndexerStatus {
  isRunning: boolean;
  currentRequest: IndexRequest | null;
  queue: IndexRequest[];
}

export async function fetchIndexerStatus(): Promise<IndexerStatus> {
  const r = await fetch('/api/indexer');
  if (!r.ok) throw new Error('Failed to fetch indexer status');
  return r.json();
}

export async function enqueueIndex(
  directoryFilter: string | null,
  flags: number,
  relativeModelPath: string | null = null,
): Promise<IndexRequest> {
  const r = await fetch('/api/indexer', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ directoryFilter, relativeModelPath, flags }),
  });
  if (!r.ok) throw new Error('Failed to enqueue index request');
  return r.json();
}
