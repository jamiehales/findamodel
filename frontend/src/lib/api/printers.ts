import { apiFetch } from '../http';

export interface PrinterCtbSettings {
  layerHeightMm: number;
  bottomLayerCount: number;
  transitionLayerCount: number;
  exposureTimeSeconds: number;
  bottomExposureTimeSeconds: number;
  bottomLiftHeightMm: number;
  bottomLiftSpeedMmPerMinute: number;
  liftHeightMm: number;
  liftSpeedMmPerMinute: number;
  retractSpeedMmPerMinute: number;
  bottomLightOffDelaySeconds: number;
  lightOffDelaySeconds: number;
  waitTimeBeforeCureSeconds: number;
  waitTimeAfterCureSeconds: number;
  waitTimeAfterLiftSeconds: number;
  lightPwm: number;
  bottomLightPwm: number;
}

const defaultPrinterCtbSettings: PrinterCtbSettings = {
  layerHeightMm: 0.05,
  bottomLayerCount: 4,
  transitionLayerCount: 0,
  exposureTimeSeconds: 2.5,
  bottomExposureTimeSeconds: 30,
  bottomLiftHeightMm: 6,
  bottomLiftSpeedMmPerMinute: 65,
  liftHeightMm: 6,
  liftSpeedMmPerMinute: 80,
  retractSpeedMmPerMinute: 150,
  bottomLightOffDelaySeconds: 0,
  lightOffDelaySeconds: 0,
  waitTimeBeforeCureSeconds: 0,
  waitTimeAfterCureSeconds: 0,
  waitTimeAfterLiftSeconds: 0,
  lightPwm: 255,
  bottomLightPwm: 255,
};

export interface PrinterConfig extends PrinterCtbSettings {
  id: string;
  name: string;
  bedWidthMm: number;
  bedDepthMm: number;
  pixelWidth: number;
  pixelHeight: number;
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
  pixelWidth: number;
  pixelHeight: number;
  ctbSettings?: Partial<PrinterCtbSettings>;
}): Promise<PrinterConfig> {
  const { ctbSettings, ...baseRequest } = request;
  const r = await apiFetch('/api/printers', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      ...baseRequest,
      ...defaultPrinterCtbSettings,
      ...ctbSettings,
    }),
  });
  if (!r.ok) throw new Error('Failed to create printer');
  return r.json();
}

export async function updatePrinter(
  id: string,
  request: {
    name: string;
    bedWidthMm: number;
    bedDepthMm: number;
    pixelWidth: number;
    pixelHeight: number;
    ctbSettings?: Partial<PrinterCtbSettings>;
  },
): Promise<PrinterConfig> {
  const { ctbSettings, ...baseRequest } = request;
  const r = await apiFetch(`/api/printers/${id}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      ...baseRequest,
      ...defaultPrinterCtbSettings,
      ...ctbSettings,
    }),
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
