import Box from '@mui/material/Box'
import CircularProgress from '@mui/material/CircularProgress'
import Typography from '@mui/material/Typography'
import { useModels } from '../lib/queries'
import ModelCard from './ModelCard'
import styles from './ModelGrid.module.css'

function ModelGrid() {
  const { data: models, isPending, isError } = useModels(80)

  if (isPending) {
    return (
      <Box className={styles.container}>
        <Typography variant="section-label">
          Models
        </Typography>
        <Box className={styles.loadingCenter}>
          <CircularProgress color="primary" />
        </Box>
      </Box>
    )
  }

  if (isError || !models || models.length === 0) return null

  return (
    <Box className={styles.container}>
      <Typography variant="section-label">
        Models
      </Typography>
      <Box className={styles.grid}>
        {models.map(model => (
          <ModelCard
            key={model.id}
            model={model}
            href={`/model/${encodeURIComponent(model.id)}`}
          />
        ))}
      </Box>
    </Box>
  )
}

export default ModelGrid
