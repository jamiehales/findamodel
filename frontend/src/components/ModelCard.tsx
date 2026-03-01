import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import type { Model } from '../lib/api'
import AppCard from './AppCard'
import PrintingListControls from './PrintingListControls'

function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

const badgeColors: Record<string, { bg: string; color: string }> = {
  stl: { bg: 'rgba(99,102,241,0.2)', color: '#818cf8' },
  obj: { bg: 'rgba(16,185,129,0.2)', color: '#34d399' },
}

interface ModelCardProps {
  model: Model
  href: string
}

function ModelCard({ model, href }: ModelCardProps) {
  const badge = badgeColors[model.fileType] ?? { bg: 'rgba(255,255,255,0.1)', color: '#94a3b8' }

  return (
    <AppCard
      href={href}
      sx={{
        display: 'block',
        borderRadius: '14px',
        border: '1px solid rgba(255,255,255,0.08)',
        aspectRatio: '3 / 4',
        position: 'relative',
        overflow: 'hidden',
        transition: 'transform 0.15s ease, box-shadow 0.15s ease',
      }}
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
          height: '100%',
          position: 'relative',
          zIndex: 1,
          background: 'linear-gradient(to top, rgba(15,23,42,0.95) 0%, rgba(15,23,42,0.5) 60%, transparent 100%)',
          boxSizing: 'border-box',
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

      <PrintingListControls modelId={model.id} />
    </AppCard>
  )
}

export default ModelCard
