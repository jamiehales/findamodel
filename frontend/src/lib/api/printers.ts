import { apiFetch } from '../http';

export interface PrinterConfig {
  id: string;
  name: string;
  bedWidthMm: number;
  bedDepthMm: number;
  isBuiltIn: boolean;
  isDefault: boolean;
}

export async function fetchPrinters(): Promise<PrinterConfig[]> {
  const r = await apiFetch('/api/printers');
  if (!r.ok) throw new Error('Failed to fetch printers');
  return r.json();
}

export async function createPrinter(request: {
  name: string;
  bedWidthMm: number;
  bedDepthMm: number;
}): Promise<PrinterConfig> {
  const r = await apiFetch('/api/printers', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  });
  if (!r.ok) throw new Error('Failed to create printer');
  return r.json();
}

export async function updatePrinter(
  id: string,
  request: { name: string; bedWidthMm: number; bedDepthMm: number },
): Promise<PrinterConfig> {
  const r = await apiFetch(`/api/printers/${id}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  });
  if (!r.ok) throw new Error('Failed to update printer');
  return r.json();
}

export async function deletePrinter(id: string): Promise<void> {
  const r = await apiFetch(`/api/printers/${id}`, { method: 'DELETE' });
  if (!r.ok) throw new Error('Failed to delete printer');
}

export async function setDefaultPrinter(id: string): Promise<void> {
  const r = await apiFetch(`/api/printers/${id}/set-default`, { method: 'POST' });
  if (!r.ok) throw new Error('Failed to set default printer');
}
