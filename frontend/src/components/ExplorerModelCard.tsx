import Box from '@mui/material/Box'
import Card from '@mui/material/Card'
import CardActionArea from '@mui/material/CardActionArea'
import Typography from '@mui/material/Typography'
import type { ExplorerModel } from '../lib/api'

function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

const badgeColors: Record<string, { bg: string; color: string }> = {
  stl: { bg: 'rgba(99,102,241,0.2)', color: '#818cf8' },
  obj: { bg: 'rgba(16,185,129,0.2)', color: '#34d399' },
}

interface Props {
  model: ExplorerModel
  onClick: () => void
}

export default function ExplorerModelCard({ model, onClick }: Props) {
  const badge = badgeColors[model.fileType] ?? { bg: 'rgba(255,255,255,0.1)', color: '#94a3b8' }
  const isIndexed = model.id != null

  return (
    <Card
      sx={{
        borderRadius: '14px',
        border: `1px solid ${isIndexed ? 'rgba(255,255,255,0.08)' : 'rgba(255,255,255,0.04)'}`,
        aspectRatio: '3 / 4',
        position: 'relative',
        overflow: 'hidden',
        opacity: isIndexed ? 1 : 0.6,
        transition: 'transform 0.15s ease, box-shadow 0.15s ease',
        '&:hover': { transform: 'scale(1.03)', boxShadow: '0 8px 24px rgba(0,0,0,0.4)' },
        '&:active': { transform: 'scale(0.97)' },
      }}
    >
      <CardActionArea
        onClick={onClick}
        disabled={!isIndexed}
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
            {model.fileName.replace(/\.[^.]+$/, '')}
          </Typography>

          {model.fileSize != null && (
            <Typography sx={{ color: '#64748b', fontSize: '0.75rem' }}>
              {formatBytes(model.fileSize)}
            </Typography>
          )}

          {!isIndexed && (
            <Typography sx={{ color: '#475569', fontSize: '0.7rem', fontStyle: 'italic' }}>
              Not yet indexed
            </Typography>
          )}
        </Box>
      </CardActionArea>
    </Card>
  )
}
