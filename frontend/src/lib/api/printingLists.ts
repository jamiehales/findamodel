export type SpawnType = 'grouped' | 'random' | 'largestFirstFillGaps';
export type HullMode = 'convex' | 'sansRaft';

export interface PrintingListSummary {
  id: string;
  name: string;
  isActive: boolean;
  isDefault: boolean;
  spawnType: SpawnType;
  hullMode: HullMode;
  createdAt: string;
  ownerUsername: string | null;
  itemCount: number;
}

export interface PrintingListItem {
  id: string;
  modelId: string;
  quantity: number;
}

export interface PrintingListDetail {
  id: string;
  name: string;
  isActive: boolean;
  isDefault: boolean;
  spawnType: SpawnType;
  hullMode: HullMode;
  createdAt: string;
  ownerUsername: string | null;
  items: PrintingListItem[];
}

export interface PrintingListArchiveJob {
  jobId: string;
  fileName: string;
  status: 'queued' | 'running' | 'completed' | 'failed';
  totalEntries: number;
  completedEntries: number;
  progressPercent: number;
  currentEntryName: string | null;
  errorMessage: string | null;
}

export async function fetchPrintingLists(): Promise<PrintingListSummary[]> {
  const r = await fetch('/api/printing-lists');
  if (!r.ok) throw new Error('Failed to fetch printing lists');
  return r.json();
}

export async function fetchActivePrintingList(): Promise<PrintingListDetail | null> {
  const r = await fetch('/api/printing-lists/active');
  if (r.status === 204) return null;
  if (!r.ok) throw new Error('Failed to fetch active printing list');
  return r.json();
}

export async function fetchPrintingList(id: string): Promise<PrintingListDetail> {
  const r = await fetch(`/api/printing-lists/${id}`);
  if (!r.ok) throw new Error(`Failed to fetch printing list ${id}`);
  return r.json();
}

export async function createPrintingList(name: string): Promise<PrintingListSummary> {
  const r = await fetch('/api/printing-lists', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name }),
  });
  if (!r.ok) throw new Error('Failed to create printing list');
  return r.json();
}

export async function renamePrintingList(id: string, name: string): Promise<PrintingListSummary> {
  const r = await fetch(`/api/printing-lists/${id}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name }),
  });
  if (!r.ok) throw new Error('Failed to rename printing list');
  return r.json();
}

export async function deletePrintingList(id: string): Promise<void> {
  const r = await fetch(`/api/printing-lists/${id}`, { method: 'DELETE' });
  if (!r.ok) throw new Error('Failed to delete printing list');
}

export async function activatePrintingList(id: string): Promise<void> {
  const r = await fetch(`/api/printing-lists/${id}/activate`, { method: 'POST' });
  if (!r.ok) throw new Error('Failed to activate printing list');
}

export async function updatePrintingListSettings(
  id: string,
  settings: { spawnType: SpawnType; hullMode: HullMode },
): Promise<PrintingListDetail> {
  const r = await fetch(`/api/printing-lists/${id}/settings`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(settings),
  });
  if (!r.ok) throw new Error('Failed to update printing list settings');
  return r.json();
}

export async function upsertPrintingListItem(
  listId: string,
  modelId: string,
  quantity: number,
): Promise<PrintingListDetail> {
  const r = await fetch(`/api/printing-lists/${listId}/items/${modelId}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ quantity }),
  });
  if (!r.ok) throw new Error('Failed to update printing list item');
  return r.json();
}

export async function clearPrintingListItems(listId: string): Promise<PrintingListDetail> {
  const r = await fetch(`/api/printing-lists/${listId}/items`, { method: 'DELETE' });
  if (!r.ok) throw new Error('Failed to clear printing list items');
  return r.json();
}

export async function createPrintingListArchiveJob(
  listId: string,
  options?: { flatten?: boolean },
): Promise<PrintingListArchiveJob> {
  const flatten = options?.flatten ?? true;
  const query = new URLSearchParams({ flatten: String(flatten) }).toString();
  const r = await fetch(`/api/printing-lists/${listId}/download-jobs?${query}`, { method: 'POST' });
  if (!r.ok) throw new Error('Failed to start printing list archive');
  return r.json();
}

export async function fetchPrintingListArchiveJob(jobId: string): Promise<PrintingListArchiveJob> {
  const r = await fetch(`/api/printing-lists/download-jobs/${jobId}`);
  if (!r.ok) throw new Error('Failed to fetch printing list archive status');
  return r.json();
}

export function getPrintingListArchiveDownloadUrl(jobId: string): string {
  return `/api/printing-lists/download-jobs/${jobId}/file`;
}
