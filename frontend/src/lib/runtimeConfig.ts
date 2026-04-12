export type RuntimeMode = 'server' | 'desktop';

export interface RuntimeConfig {
  apiBaseUrl: string;
  mode: RuntimeMode;
  desktopSessionToken: string | null;
}

interface TauriInternals {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
}

interface DesktopRuntimeConfigResponse {
  apiBaseUrl?: string;
  mode?: RuntimeMode;
  desktopSessionToken?: string | null;
}

const DEFAULT_HEALTH_TIMEOUT_MS = 20_000;

let runtimeConfig: RuntimeConfig = {
  apiBaseUrl: normalizeBaseUrl(import.meta.env.VITE_API_BASE_URL ?? ''),
  mode: 'server',
  desktopSessionToken: null,
};

function normalizeBaseUrl(value: string): string {
  const trimmed = value.trim();
  if (!trimmed) return '';
  return trimmed.endsWith('/') ? trimmed.slice(0, -1) : trimmed;
}

async function tryLoadDesktopRuntimeConfig(): Promise<RuntimeConfig | null> {
  const tauriInvoke =
    typeof window !== 'undefined'
      ? (window.__TAURI_INTERNALS__ as TauriInternals | undefined)?.invoke
      : undefined;
  if (!tauriInvoke) return null;

  const response = await tauriInvoke<DesktopRuntimeConfigResponse>('desktop_runtime_config');
  const apiBaseUrl = normalizeBaseUrl(response.apiBaseUrl ?? '');
  return {
    apiBaseUrl,
    mode: response.mode === 'desktop' ? 'desktop' : 'server',
    desktopSessionToken: response.desktopSessionToken ?? null,
  };
}

function withTokenHeaders(headers?: HeadersInit): Headers {
  const merged = new Headers(headers);
  if (runtimeConfig.desktopSessionToken) {
    merged.set('X-Findamodel-Desktop-Token', runtimeConfig.desktopSessionToken);
  }
  return merged;
}

function toHealthUrl(): string {
  if (runtimeConfig.apiBaseUrl) {
    return `${runtimeConfig.apiBaseUrl}/health`;
  }
  return '/health';
}

export async function waitForBackendReady(
  timeoutMs: number = DEFAULT_HEALTH_TIMEOUT_MS,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  let lastError: string | null = null;

  while (Date.now() < deadline) {
    try {
      const response = await fetch(toHealthUrl(), {
        headers: withTokenHeaders(),
        cache: 'no-store',
      });
      if (response.ok) return;
      lastError = `health endpoint returned HTTP ${response.status}`;
    } catch (error) {
      lastError = error instanceof Error ? error.message : 'health request failed';
    }

    await new Promise((resolve) => setTimeout(resolve, 400));
  }

  throw new Error(
    lastError
      ? `Backend did not become healthy: ${lastError}`
      : 'Backend did not become healthy in time',
  );
}

export async function initializeRuntimeConfig(): Promise<void> {
  const desktopConfig = await tryLoadDesktopRuntimeConfig();
  if (desktopConfig) {
    runtimeConfig = desktopConfig;
  }
}

export function getRuntimeConfig(): RuntimeConfig {
  return runtimeConfig;
}
