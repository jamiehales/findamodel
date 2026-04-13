import { useCallback, useEffect, useMemo, useState } from 'react';
import Box from '@mui/material/Box';
import { useSearchParams } from 'react-router-dom';
import ModelGrid from '../components/ModelGrid';
import ModelFilters from '../components/ModelFilters';
import {
  useFilterOptions,
  useMetadataDictionaryOverview,
  useModelNameOptions,
} from '../lib/queries';
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
  for (const value of filter.creator) params.append('creator', value);
  for (const value of filter.collection) params.append('collection', value);
  for (const value of filter.subcollection) params.append('subcollection', value);
  for (const value of filter.tags) params.append('tags', value);
  for (const value of filter.category) params.append('category', value);
  for (const value of filter.type) params.append('type', value);
  for (const value of filter.material) params.append('material', value);
  for (const value of filter.fileType) params.append('fileType', value);
  if (filter.supported !== null) params.set('supported', String(filter.supported));
  return params;
}

function ModelsPage() {
  const [searchParams, setSearchParams] = useSearchParams();
  const filter = useMemo(() => toFilter(searchParams), [searchParams]);
  const selectedModelName = searchParams.get('modelName') ?? '';
  const [modelNameInput, setModelNameInput] = useState('');
  const [debouncedModelNameInput, setDebouncedModelNameInput] = useState('');

  useEffect(() => {
    const timeoutId = window.setTimeout(() => {
      setDebouncedModelNameInput(modelNameInput);
    }, 250);

    return () => window.clearTimeout(timeoutId);
  }, [modelNameInput]);

  const { data: filterOptions } = useFilterOptions(filter, selectedModelName);
  const { data: metadataDictionary } = useMetadataDictionaryOverview();
  const { data: modelNameOptions = [], isPending: modelNameOptionsLoading } = useModelNameOptions(
    filter,
    50,
    debouncedModelNameInput,
  );

  const mergedOptions = useMemo(() => {
    if (!filterOptions) return filterOptions;
    const schemaTags = metadataDictionary?.tags.configured.map((v) => v.value) ?? [];
    const merged = [...new Set([...schemaTags, ...filterOptions.tags])];
    return { ...filterOptions, tags: merged };
  }, [filterOptions, metadataDictionary]);

  const handleFilterChange = useCallback(
    (nextFilter: ModelFilter) => {
      setSearchParams(toSearchParams(nextFilter, selectedModelName));
    },
    [selectedModelName, setSearchParams],
  );

  const handleModelNameChange = useCallback(
    (nextModelName: string) => {
      setSearchParams(toSearchParams(filter, nextModelName));
    },
    [filter, setSearchParams],
  );

  return (
    <PageLayout spacing={4}>
      {mergedOptions && (
        <Box className={styles.filtersWrapper}>
          <ModelFilters
            value={filter}
            onChange={handleFilterChange}
            options={mergedOptions}
            modelNameOptions={modelNameOptions}
            modelNameOptionsLoading={modelNameOptionsLoading}
            modelNameValue={selectedModelName}
            onModelNameInputChange={setModelNameInput}
            onModelNameChange={handleModelNameChange}
          />
        </Box>
      )}
      <ModelGrid filter={filter} modelName={selectedModelName} />
    </PageLayout>
  );
}

export default ModelsPage;
