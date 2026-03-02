import { useState } from 'react'
import Box from '@mui/material/Box'
import Button from '@mui/material/Button'
import CircularProgress from '@mui/material/CircularProgress'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import { useModels, useQueryModels } from '../lib/queries'
import type { ModelFilter } from '../lib/api'
import ModelCard from './ModelCard'
import styles from './ModelGrid.module.css'

const PAGE_SIZE = 25

interface Props {
  filter?: ModelFilter
}

function FilteredGrid({ filter }: { filter: ModelFilter }) {
  const [limit, setLimit] = useState(PAGE_SIZE)
  const { data, isPending, isError } = useQueryModels(filter, limit)

  if (isPending) return <LoadingState />
  if (isError || !data || data.models.length === 0) return null

  return (
    <Box className={styles.container}>
      <Typography variant="section-label">Models</Typography>
      <Box className={styles.grid}>
        {data.models.map(model => (
          <ModelCard key={model.id} model={model} href={`/model/${encodeURIComponent(model.id)}`} />
        ))}
      </Box>
      {data.hasMore && (
        <Stack alignItems="center" paddingTop={2}>
          <Button variant="outlined" onClick={() => setLimit(l => l + PAGE_SIZE)}>
            Show more
          </Button>
        </Stack>
      )}
    </Box>
  )
}

function UnfilteredGrid() {
  const { data: models, isPending, isError } = useModels(80)

  if (isPending) return <LoadingState />
  if (isError || !models || models.length === 0) return null

  return (
    <Box className={styles.container}>
      <Typography variant="section-label">Models</Typography>
      <Box className={styles.grid}>
        {models.map(model => (
          <ModelCard key={model.id} model={model} href={`/model/${encodeURIComponent(model.id)}`} />
        ))}
      </Box>
    </Box>
  )
}

function LoadingState() {
  return (
    <Box className={styles.container}>
      <Typography variant="section-label">Models</Typography>
      <Box className={styles.loadingCenter}>
        <CircularProgress color="primary" />
      </Box>
    </Box>
  )
}

function ModelGrid({ filter }: Props) {
  if (filter) return <FilteredGrid filter={filter} />
  return <UnfilteredGrid />
}

export default ModelGrid
