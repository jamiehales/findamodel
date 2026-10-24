import { useEffect, useState } from 'react';
import { useModels, useQueryModels } from '../lib/queries';
import type { ModelFilter } from '../lib/api';
import ModelCard from './ModelCard';
import CardGrid from './CardGrid';
import LoadingView from './LoadingView';
import Pagination from './Pagination';
import styles from './StatusView.module.css';

const PAGE_SIZE = 24;

interface Props {
  filter?: ModelFilter;
  modelName?: string;
  /** Reports the total match count, for the result count beside the search. */
  onTotalChange?: (total: number | null) => void;
}

function FilteredGrid({
  filter,
  modelName,
  onTotalChange,
}: {
  filter: ModelFilter;
  modelName?: string;
  onTotalChange?: (total: number | null) => void;
}) {
  const [page, setPage] = useState(1);
  const offset = (page - 1) * PAGE_SIZE;
  const { data, isPending, isError } = useQueryModels(filter, PAGE_SIZE, offset, modelName);

  useEffect(() => {
    setPage(1);
  }, [filter, modelName]);

  useEffect(() => {
    onTotalChange?.(data ? data.totalCount : null);
  }, [data, onTotalChange]);

  if (isPending) return <LoadingView />;
  if (isError) return <p className={styles.error}>Couldn't load models. Try again.</p>;
  if (data.models.length === 0)
    return <p className={styles.empty}>No models match these filters.</p>;

  const totalPages = Math.max(1, Math.ceil(data.totalCount / PAGE_SIZE));

  return (
    <>
      <CardGrid>
        {data.models.map((model) => (
          <ModelCard key={model.id} model={model} href={`/model/${encodeURIComponent(model.id)}`} />
        ))}
      </CardGrid>
      <Pagination page={page} totalPages={totalPages} onPageChange={setPage} />
    </>
  );
}

function UnfilteredGrid() {
  const { data: models, isPending, isError } = useModels(80);

  if (isPending) return <LoadingView />;
  if (isError || !models || models.length === 0) return null;

  return (
    <CardGrid>
      {models.map((model) => (
        <ModelCard key={model.id} model={model} href={`/model/${encodeURIComponent(model.id)}`} />
      ))}
    </CardGrid>
  );
}

function ModelGrid({ filter, modelName, onTotalChange }: Props) {
  if (filter)
    return <FilteredGrid filter={filter} modelName={modelName} onTotalChange={onTotalChange} />;
  return <UnfilteredGrid />;
}

export default ModelGrid;
