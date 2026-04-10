import { useState } from 'react';
import Box from '@mui/material/Box';
import ModelGrid from '../components/ModelGrid';
import ModelFilters from '../components/ModelFilters';
import { useFilterOptions } from '../lib/queries';
import { emptyFilter, type ModelFilter } from '../lib/api';
import styles from './ModelsPage.module.css';

function ModelsPage() {
  const { data: filterOptions } = useFilterOptions();
  const [filter, setFilter] = useState<ModelFilter>(emptyFilter);

  return (
    <Box className={styles.page}>
      {filterOptions && (
        <Box className={styles.filtersWrapper}>
          <ModelFilters value={filter} onChange={setFilter} options={filterOptions} />
        </Box>
      )}
      <ModelGrid filter={filter} />
    </Box>
  );
}

export default ModelsPage;
