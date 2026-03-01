import Box from '@mui/material/Box'
import Skeleton from '@mui/material/Skeleton'
import Typography from '@mui/material/Typography'
import { useModels } from '../lib/queries'
import ModelCard from './ModelCard'

const gridSx = {
  display: 'grid',
  gridTemplateColumns: 'repeat(auto-fill, minmax(160px, 1fr))',
  gap: '0.875rem',
}

function ModelGrid() {
  const { data: models, isPending, isError } = useModels(80)

  if (isPending) {
    return (
      <Box sx={{ width: '100%', maxWidth: 900, px: 2, boxSizing: 'border-box' }}>
        <Typography
          sx={{
            fontSize: '1.1rem',
            fontWeight: 600,
            color: '#94a3b8',
            textTransform: 'uppercase',
            letterSpacing: '0.08em',
            mb: '0.75rem',
            ml: '0.25rem',
          }}
        >
          Models
        </Typography>
        <Box sx={gridSx}>
          {[1, 2, 3, 4, 5, 6].map(i => (
            <Skeleton
              key={i}
              variant="rectangular"
              sx={{ aspectRatio: '3 / 4', borderRadius: '14px', height: 'auto' }}
            />
          ))}
        </Box>
      </Box>
    )
  }

  if (isError || !models || models.length === 0) return null

  return (
    <Box sx={{ width: '100%', maxWidth: 900, px: 2, boxSizing: 'border-box' }}>
      <Typography
        sx={{
          fontSize: '1.1rem',
          fontWeight: 600,
          color: '#94a3b8',
          textTransform: 'uppercase',
          letterSpacing: '0.08em',
          mb: '0.75rem',
          ml: '0.25rem',
        }}
      >
        Models
      </Typography>
      <Box sx={gridSx}>
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
