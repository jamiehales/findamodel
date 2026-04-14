import { getRuntimeConfig } from './runtimeConfig';

function appendDesktopToken(url: string, token: string): string {
  const parsed = new URL(url, window.location.origin);
  parsed.searchParams.set('desktopToken', token);

  if (url.startsWith('http://') || url.startsWith('https://')) {
    return parsed.toString();
  }

  return `${parsed.pathname}${parsed.search}${parsed.hash}`;
}

export function apiUrl(path: string): string {
  const normalizedPath = path.startsWith('/') ? path : `/${path}`;
  const { apiBaseUrl, desktopSessionToken } = getRuntimeConfig();
  const rawUrl = apiBaseUrl ? `${apiBaseUrl}${normalizedPath}` : normalizedPath;

  if (!desktopSessionToken) return rawUrl;
  return appendDesktopToken(rawUrl, desktopSessionToken);
}

export function withPreviewSupports(url: string, includeSupports: boolean): string {
  const parsed = new URL(url, window.location.origin);
  parsed.searchParams.set('includeSupports', includeSupports ? 'true' : 'false');

  if (url.startsWith('http://') || url.startsWith('https://')) {
    return parsed.toString();
  }

  return `${parsed.pathname}${parsed.search}${parsed.hash}`;
}

export function apiFetch(path: string, init?: RequestInit): Promise<Response> {
  const { desktopSessionToken } = getRuntimeConfig();
  const headers = new Headers(init?.headers);

  if (desktopSessionToken) {
    headers.set('X-Findamodel-Desktop-Token', desktopSessionToken);
  }

  return fetch(apiUrl(path), {
    ...init,
    headers,
  });
}
