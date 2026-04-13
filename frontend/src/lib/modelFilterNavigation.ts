export function appendFilterValue(
  sourceSearch: string,
  key: 'tags' | 'category' | 'type' | 'material' | 'fileType',
  value: string,
): string {
  const params = new URLSearchParams(sourceSearch);
  const normalized = value.trim();
  if (!normalized) return params.toString();

  const existing = params.getAll(key);
  if (!existing.includes(normalized)) {
    params.append(key, normalized);
  }

  return params.toString();
}

export function appendSupportedFilter(sourceSearch: string, supported: boolean): string {
  const params = new URLSearchParams(sourceSearch);
  if (params.get('supported') == null) {
    params.set('supported', String(supported));
  }
  return params.toString();
}
