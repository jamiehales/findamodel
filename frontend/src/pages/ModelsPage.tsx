import { useCallback, useMemo } from 'react';
import Box from '@mui/material/Box';
import { useSearchParams } from 'react-router-dom';
import ModelGrid from '../components/ModelGrid';
import ModelFilters from '../components/ModelFilters';
import { useFilterOptions } from '../lib/queries';
import type { ModelFilter } from '../lib/api';
import PageLayout from '../components/layouts/PageLayout';
import styles from './ModelsPage.module.css';

function toFilter(searchParams: URLSearchParams): ModelFilter {
  const supportedParam = searchParams.get('supported');
  const supported = supportedParam === 'true' ? true : supportedParam === 'false' ? false : null;

  return {
    search: searchParams.get('search') ?? '',
    creator: searchParams.getAll('creator'),
    collection: searchParams.getAll('collection'),
    subcollection: searchParams.getAll('subcollection'),
    category: searchParams.getAll('category'),
    type: searchParams.getAll('type'),
    material: searchParams.getAll('material'),
    fileType: searchParams.getAll('fileType'),
    supported,
  };
}

function toSearchParams(filter: ModelFilter): URLSearchParams {
  const params = new URLSearchParams();
  if (filter.search) params.set('search', filter.search);
  for (const value of filter.creator) params.append('creator', value);
  for (const value of filter.collection) params.append('collection', value);
  for (const value of filter.subcollection) params.append('subcollection', value);
  for (const value of filter.category) params.append('category', value);
  for (const value of filter.type) params.append('type', value);
  for (const value of filter.material) params.append('material', value);
  for (const value of filter.fileType) params.append('fileType', value);
  if (filter.supported !== null) params.set('supported', String(filter.supported));
  return params;
}

function ModelsPage() {
  const { data: filterOptions } = useFilterOptions();
  const [searchParams, setSearchParams] = useSearchParams();

  const filter = useMemo(() => toFilter(searchParams), [searchParams]);

  const handleFilterChange = useCallback(
    (nextFilter: ModelFilter) => {
      setSearchParams(toSearchParams(nextFilter));
    },
    [setSearchParams],
  );

  return (
    <PageLayout spacing={4}>
      {filterOptions && (
        <Box className={styles.filtersWrapper}>
          <ModelFilters value={filter} onChange={handleFilterChange} options={filterOptions} />
        </Box>
      )}
      <ModelGrid filter={filter} />
    </PageLayout>
  );
}

export default ModelsPage;
