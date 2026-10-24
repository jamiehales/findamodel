import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useSearchParams } from 'react-router-dom';
import { SlidersHorizontal, X } from 'lucide-react';
import { Button, SearchField, Tag } from '@jamiehales/ui-kit/mostlymaking';
import { AppSheet } from '../components/AppDialog';
import ModelGrid from '../components/ModelGrid';
import ModelFilters from '../components/ModelFilters';
import { useFilterOptions, useMetadataDictionaryOverview } from '../lib/queries';
import { emptyFilter, type ModelFilter } from '../lib/api';
import styles from './ModelsPage.module.css';

const LIST_FIELDS = [
  'creator',
  'collection',
  'subcollection',
  'category',
  'type',
  'tags',
  'material',
  'fileType',
] as const;

function toFilter(searchParams: URLSearchParams): ModelFilter {
  const supportedParam = searchParams.get('supported');
  const supported = supportedParam === 'true' ? true : supportedParam === 'false' ? false : null;

  return {
    search: searchParams.get('search') ?? '',
    creator: searchParams.getAll('creator'),
    collection: searchParams.getAll('collection'),
    subcollection: searchParams.getAll('subcollection'),
    tags: searchParams.getAll('tags'),
    category: searchParams.getAll('category'),
    type: searchParams.getAll('type'),
    material: searchParams.getAll('material'),
    fileType: searchParams.getAll('fileType'),
    supported,
  };
}

function toSearchParams(filter: ModelFilter, modelName: string): URLSearchParams {
  const params = new URLSearchParams();
  if (filter.search) params.set('search', filter.search);
  if (modelName) params.set('modelName', modelName);
  for (const field of LIST_FIELDS) {
    for (const value of filter[field]) params.append(field, value);
  }
  if (filter.supported !== null) params.set('supported', String(filter.supported));
  return params;
}

const numberFormat = new Intl.NumberFormat();

function ModelsPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const filter = useMemo(() => toFilter(searchParams), [searchParams]);
  const modelName = searchParams.get('modelName') ?? '';
  const [searchInput, setSearchInput] = useState(filter.search);
  const [total, setTotal] = useState<number | null>(null);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const { data: filterOptions } = useFilterOptions(filter, modelName);
  const { data: metadataDictionary } = useMetadataDictionaryOverview();

  useEffect(() => {
    setSearchInput(filter.search);
  }, [filter.search]);

  useEffect(
    () => () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    },
    [],
  );

  const mergedOptions = useMemo(() => {
    if (!filterOptions) return filterOptions;
    const schemaTags = metadataDictionary?.tags.configured.map((v) => v.value) ?? [];
    return { ...filterOptions, tags: [...new Set([...schemaTags, ...filterOptions.tags])] };
  }, [filterOptions, metadataDictionary]);

  const applyFilter = useCallback(
    (next: ModelFilter, nextModelName = modelName) => {
      setSearchParams(toSearchParams(next, nextModelName));
    },
    [modelName, setSearchParams],
  );

  const handleSearchChange = (raw: string) => {
    setSearchInput(raw);
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => applyFilter({ ...filter, search: raw }), 250);
  };

  const active: { key: string; label: string; remove: () => void }[] = [];
  if (modelName) {
    active.push({ key: 'modelName', label: modelName, remove: () => applyFilter(filter, '') });
  }
  for (const field of LIST_FIELDS) {
    for (const value of filter[field]) {
      active.push({
        key: `${field}:${value}`,
        label: field === 'fileType' ? value.toUpperCase() : value,
        remove: () => applyFilter({ ...filter, [field]: filter[field].filter((v) => v !== value) }),
      });
    }
  }
  if (filter.supported !== null) {
    active.push({
      key: 'supported',
      label: filter.supported ? 'Supported' : 'Unsupported',
      remove: () => applyFilter({ ...filter, supported: null }),
    });
  }

  const filterCount = active.length;
  const clearFilters = () => applyFilter({ ...emptyFilter, search: filter.search }, '');

  return (
    <div className={styles.layout}>
      {mergedOptions && (
        <ModelFilters value={filter} onChange={applyFilter} options={mergedOptions} />
      )}
      <main className={styles.main}>
        <div className={styles.toolbar}>
          <SearchField
            aria-label="Search models"
            placeholder="Search"
            value={searchInput}
            onChange={(e) => handleSearchChange(e.target.value)}
            className={styles.search}
          />
          <Button
            variant="secondary"
            className={styles.filtersButton}
            onClick={() => setFiltersOpen(true)}
            aria-label={filterCount > 0 ? `Filters, ${filterCount} on` : 'Filters'}
          >
            <SlidersHorizontal size={16} strokeWidth={1.5} aria-hidden="true" />
            Filters
            {filterCount > 0 && <span className={styles.filterCount}>{filterCount}</span>}
          </Button>
          <div className={styles.spacer} />
          {total !== null && (
            <span className={styles.count}>
              {numberFormat.format(total)} {total === 1 ? 'model' : 'models'}
            </span>
          )}
        </div>

        {active.length > 0 && (
          <div className={styles.active}>
            {active.map((a) => (
              <Tag key={a.key} asChild>
                <button
                  type="button"
                  className={styles.remove}
                  onClick={a.remove}
                  aria-label={`Remove filter ${a.label}`}
                >
                  {a.label}
                  <X size={12} strokeWidth={2} aria-hidden="true" />
                </button>
              </Tag>
            ))}
            <Button variant="ghost" size="sm" onClick={clearFilters}>
              Clear
            </Button>
          </div>
        )}

        <ModelGrid filter={filter} modelName={modelName} onTotalChange={setTotal} />
      </main>

      {mergedOptions && (
        <AppSheet
          open={filtersOpen}
          onClose={() => setFiltersOpen(false)}
          title="Filters"
          side="left"
          footer={
            <>
              <Button variant="ghost" disabled={filterCount === 0} onClick={clearFilters}>
                Clear
              </Button>
              <Button variant="primary" onClick={() => setFiltersOpen(false)}>
                {total === null
                  ? 'Done'
                  : `Show ${numberFormat.format(total)} ${total === 1 ? 'model' : 'models'}`}
              </Button>
            </>
          }
        >
          <ModelFilters
            variant="sheet"
            value={filter}
            onChange={applyFilter}
            options={mergedOptions}
          />
        </AppSheet>
      )}
    </div>
  );
}

export default ModelsPage;
