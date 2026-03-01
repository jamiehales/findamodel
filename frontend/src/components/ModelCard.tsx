import Box from '@mui/material/Box'
import Card from '@mui/material/Card'
import CardActionArea from '@mui/material/CardActionArea'
import Typography from '@mui/material/Typography'
import type { Model } from '../lib/api'
import { usePrintingList } from '../lib/printingList'

function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

const badgeColors: Record<string, { bg: string; color: string }> = {
  stl: { bg: 'rgba(99,102,241,0.2)', color: '#818cf8' },
  obj: { bg: 'rgba(16,185,129,0.2)', color: '#34d399' },
}

const btnSx = {
  pointerEvents: 'auto',
  width: 26, height: 26,
  borderRadius: '50%',
  border: '1px solid rgba(255,255,255,0.18)',
  background: 'rgba(15,23,42,0.7)',
  color: '#e2e8f0',
  fontSize: '1rem',
  lineHeight: 1,
  cursor: 'pointer',
  display: 'flex', alignItems: 'center', justifyContent: 'center',
  p: 0,
  '&:hover': { background: 'rgba(30,41,59,0.9)' },
  '&:active': { transform: 'scale(0.92)' },
} as const

interface ModelCardProps {
  model: Model
  onClick: () => void
}

function ModelCard({ model, onClick }: ModelCardProps) {
  const { items, setQuantity } = usePrintingList()
  const badge = badgeColors[model.fileType] ?? { bg: 'rgba(255,255,255,0.1)', color: '#94a3b8' }
  const quantity = items[model.id]

  return (
    <Card
      sx={{
        borderRadius: '14px',
        border: '1px solid rgba(255,255,255,0.08)',
        aspectRatio: '3 / 4',
        position: 'relative',
        overflow: 'hidden',
        transition: 'transform 0.15s ease, box-shadow 0.15s ease',
        '&:hover': { transform: 'scale(1.03)', boxShadow: '0 8px 24px rgba(0,0,0,0.4)' },
        '&:active': { transform: 'scale(0.97)' },
      }}
    >
      <CardActionArea
        onClick={onClick}
        sx={{ height: '100%', display: 'flex', alignItems: 'stretch' }}
      >
        {model.previewUrl && (
          <Box
            component="img"
            src={model.previewUrl}
            alt=""
            sx={{
              position: 'absolute',
              inset: 0,
              width: '100%',
              height: '100%',
              objectFit: 'cover',
            }}
          />
        )}

        <Box
          sx={{
            display: 'flex',
            flexDirection: 'column',
            justifyContent: 'flex-end',
            p: '0.875rem',
            gap: '0.25rem',
            width: '100%',
            position: 'relative',
            zIndex: 1,
            background: 'linear-gradient(to top, rgba(15,23,42,0.95) 0%, rgba(15,23,42,0.5) 60%, transparent 100%)',
          }}
        >
          <Box
            component="span"
            sx={{
              alignSelf: 'flex-start',
              fontSize: '0.65rem',
              fontWeight: 700,
              letterSpacing: '0.06em',
              px: '0.5rem',
              py: '0.2rem',
              borderRadius: '4px',
              mb: '0.25rem',
              background: badge.bg,
              color: badge.color,
            }}
          >
            {model.fileType.toUpperCase()}
          </Box>

          <Typography
            sx={{
              color: '#f1f5f9',
              fontSize: '0.875rem',
              fontWeight: 600,
              lineHeight: 1.3,
              display: '-webkit-box',
              WebkitLineClamp: 2,
              WebkitBoxOrient: 'vertical',
              overflow: 'hidden',
            }}
          >
            {model.name}
          </Typography>

          <Typography sx={{ color: '#64748b', fontSize: '0.75rem' }}>
            {formatBytes(model.fileSize)}
          </Typography>
        </Box>
      </CardActionArea>

      <Box
        sx={{
          position: 'absolute',
          top: 0,
          left: 0,
          right: 0,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          px: '0.4rem',
          py: '0.35rem',
          zIndex: 2,
          background: 'linear-gradient(to bottom, rgba(15,23,42,0.75) 0%, transparent 100%)',
          pointerEvents: 'none',
        }}
      >
        <Box
          component="button"
          onClick={e => { e.stopPropagation(); setQuantity(model.id, (quantity ?? 0) - 1) }}
          sx={btnSx}
        >
          −
        </Box>

        <Typography
          sx={{
            color: '#e2e8f0',
            fontSize: '0.8rem',
            fontWeight: 700,
            letterSpacing: '0.02em',
            textShadow: '0 1px 4px rgba(0,0,0,0.6)',
            pointerEvents: 'none',
            visibility: quantity !== undefined ? 'visible' : 'hidden',
          }}
        >
          ×{quantity ?? 0}
        </Typography>

        <Box
          component="button"
          onClick={e => { e.stopPropagation(); setQuantity(model.id, (quantity ?? 0) + 1) }}
          sx={btnSx}
        >
          +
        </Box>
      </Box>
    </Card>
  )
}

export default ModelCard
