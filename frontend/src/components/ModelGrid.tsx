import { useEffect, useState } from 'react';
import Box from '@mui/material/Box';
import CircularProgress from '@mui/material/CircularProgress';
import Pagination from '@mui/material/Pagination';
import Stack from '@mui/material/Stack';
import { useModels, useQueryModels } from '../lib/queries';
import type { ModelFilter } from '../lib/api';
import ModelCard from './ModelCard';
import CardGrid, { DEFAULT_CARD_MIN_WIDTH_PX } from './CardGrid';
import styles from './ModelGrid.module.css';

const PAGE_SIZE = 25;

interface Props {
  filter?: ModelFilter;
}

function FilteredGrid({ filter }: { filter: ModelFilter }) {
  const [page, setPage] = useState(1);
  const offset = (page - 1) * PAGE_SIZE;
  const { data, isPending, isError } = useQueryModels(filter, PAGE_SIZE, offset);

  useEffect(() => {
    setPage(1);
  }, [filter]);

  if (isPending) return <LoadingState />;
  if (isError || !data || data.models.length === 0) return null;

  const totalPages = Math.max(1, Math.ceil(data.totalCount / PAGE_SIZE));

  return (
    <Box className={styles.container}>
      <CardGrid minCardWidth={DEFAULT_CARD_MIN_WIDTH_PX}>
        {data.models.map((model) => (
          <ModelCard key={model.id} model={model} href={`/model/${encodeURIComponent(model.id)}`} />
        ))}
      </CardGrid>
      {totalPages > 1 && (
        <Stack alignItems="center" paddingTop={2}>
          <Pagination
            color="primary"
            count={totalPages}
            page={page}
            onChange={(_event, value) => setPage(value)}
          />
        </Stack>
      )}
    </Box>
  );
}

function UnfilteredGrid() {
  const { data: models, isPending, isError } = useModels(80);

  if (isPending) return <LoadingState />;
  if (isError || !models || models.length === 0) return null;

  return (
    <Box className={styles.container}>
      <CardGrid minCardWidth={DEFAULT_CARD_MIN_WIDTH_PX}>
        {models.map((model) => (
          <ModelCard key={model.id} model={model} href={`/model/${encodeURIComponent(model.id)}`} />
        ))}
      </CardGrid>
    </Box>
  );
}

function LoadingState() {
  return (
    <Box className={styles.container}>
      <Box className={styles.loadingCenter}>
        <CircularProgress color="primary" />
      </Box>
    </Box>
  );
}

function ModelGrid({ filter }: Props) {
  if (filter) return <FilteredGrid filter={filter} />;
  return <UnfilteredGrid />;
}

export default ModelGrid;
