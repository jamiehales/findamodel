import React from 'react'
import Box from '@mui/material/Box'
import Button from '@mui/material/Button'
import CircularProgress from '@mui/material/CircularProgress'
import Typography from '@mui/material/Typography'
import { useQuery } from '@tanstack/react-query'
import { useParams, useNavigate } from 'react-router-dom'
import { fetchModels } from '../lib/api'

function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

const badgeColors: Record<string, { bg: string; color: string }> = {
  stl: { bg: 'rgba(99,102,241,0.2)', color: '#818cf8' },
  obj: { bg: 'rgba(16,185,129,0.2)', color: '#34d399' },
}

function ModelPage() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()

  const decodedId = decodeURIComponent(id ?? '')

  const { data: model, isPending, isError } = useQuery({
    queryKey: ['models'],
    queryFn: fetchModels,
    select: models => models.find(m => m.id === decodedId) ?? null,
  })

  const backButton = (
    <Button
      onClick={() => navigate('/')}
      sx={{
        position: 'fixed',
        top: 'calc(env(safe-area-inset-top, 0px) + 0.75rem)',
        left: '1rem',
        background: 'rgba(15,23,42,0.7)',
        backdropFilter: 'blur(8px)',
        color: '#e2e8f0',
        border: '1px solid rgba(255,255,255,0.12)',
        borderRadius: '999px',
        px: '1rem',
        py: '0.5rem',
        fontSize: '0.9rem',
        fontWeight: 500,
        textTransform: 'none',
        zIndex: 10,
        minWidth: 0,
        '&:hover': { background: 'rgba(30,41,59,0.9)' },
        '&:active': { background: 'rgba(30,41,59,0.9)' },
      }}
    >
      ← Back
    </Button>
  )

  if (isPending) {
    return (
      <Box sx={{ minHeight: '100vh', overflow: 'hidden' }}>
        {backButton}
        <Box
          sx={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            minHeight: '100vh',
          }}
        >
          <CircularProgress sx={{ color: 'primary.main' }} />
        </Box>
      </Box>
    )
  }

  if (isError || model === null) {
    return (
      <Box sx={{ minHeight: '100vh', overflow: 'hidden' }}>
        {backButton}
        <Box
          sx={{
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            justifyContent: 'center',
            minHeight: '100vh',
            gap: 2,
            color: 'text.secondary',
          }}
        >
          <Typography>Model not found.</Typography>
        </Box>
      </Box>
    )
  }

  const badge = badgeColors[model.fileType] ?? { bg: 'rgba(255,255,255,0.1)', color: '#94a3b8' }

  const metaRows: { label: string; value: React.ReactNode }[] = [
    model.author       && { label: 'Author',        value: model.author },
    model.collection   && { label: 'Collection',    value: model.collection },
    model.subcollection && { label: 'Subcollection', value: model.subcollection },
    model.category     && { label: 'Category',      value: model.category },
    model.type         && { label: 'Type',           value: model.type },
    model.supported != null && {
      label: 'Supported',
      value: (
        <Box
          component="span"
          sx={{
            fontSize: '0.7rem',
            fontWeight: 700,
            letterSpacing: '0.06em',
            px: '0.5rem',
            py: '0.2rem',
            borderRadius: '4px',
            background: model.supported ? 'rgba(16,185,129,0.15)' : 'rgba(239,68,68,0.15)',
            color: model.supported ? '#34d399' : '#f87171',
          }}
        >
          {model.supported ? 'YES' : 'NO'}
        </Box>
      ),
    },
  ].filter(Boolean) as { label: string; value: React.ReactNode }[]

  return (
    <Box sx={{ minHeight: '100vh', overflow: 'hidden' }}>
      {backButton}

      <Box
        sx={{
          pt: '5rem',
          pb: { xs: '3rem', '@supports (padding-bottom: env(safe-area-inset-bottom))': 'calc(3rem + env(safe-area-inset-bottom))' },
          px: '1.25rem',
          maxWidth: 600,
          mx: 'auto',
          display: 'flex',
          flexDirection: 'column',
          gap: '1.5rem',
        }}
      >
        <Box sx={{ display: 'flex', flexDirection: 'column', gap: '0.375rem' }}>
          <Box
            component="span"
            sx={{
              alignSelf: 'flex-start',
              fontSize: '0.7rem',
              fontWeight: 700,
              letterSpacing: '0.08em',
              px: '0.625rem',
              py: '0.25rem',
              borderRadius: '4px',
              mb: '0.25rem',
              background: badge.bg,
              color: badge.color,
            }}
          >
            {model.fileType.toUpperCase()}
          </Box>

          <Typography
            component="h1"
            sx={{
              fontSize: { xs: '2rem', sm: '2.5rem' },
              fontWeight: 700,
              lineHeight: 1.2,
              letterSpacing: '-0.02em',
              color: '#f1f5f9',
            }}
          >
            {model.name}
          </Typography>

          <Typography sx={{ fontSize: '0.85rem', color: 'text.secondary', wordBreak: 'break-all' }}>
            {model.relativePath}
          </Typography>

          <Typography sx={{ fontSize: '0.85rem', color: '#475569' }}>
            {formatBytes(model.fileSize)}
          </Typography>
        </Box>

        {metaRows.length > 0 && (
          <Box
            sx={{
              display: 'grid',
              gridTemplateColumns: 'auto 1fr',
              columnGap: '1.5rem',
              rowGap: '0.625rem',
              borderRadius: '12px',
              border: '1px solid rgba(255,255,255,0.07)',
              bgcolor: 'background.paper',
              px: '1.25rem',
              py: '1rem',
            }}
          >
            {metaRows.map(({ label, value }) => (
              <React.Fragment key={label}>
                <Typography sx={{ fontSize: '0.8rem', color: '#475569', whiteSpace: 'nowrap', pt: '0.1rem' }}>
                  {label}
                </Typography>
                <Typography component="div" sx={{ fontSize: '0.85rem', color: '#94a3b8' }}>
                  {value}
                </Typography>
              </React.Fragment>
            ))}
          </Box>
        )}

        <Box
          sx={{
            display: 'flex',
            flexDirection: 'column',
            alignItems: 'center',
            justifyContent: 'center',
            gap: '0.75rem',
            height: 240,
            borderRadius: '16px',
            border: '1px dashed rgba(255,255,255,0.1)',
            bgcolor: 'background.paper',
            color: '#475569',
            fontSize: '0.9rem',
          }}
        >
          <Box component="span" sx={{ fontSize: '2.5rem', opacity: 0.3 }}>⬡</Box>
          <Typography sx={{ color: '#475569', fontSize: '0.9rem' }}>3D viewer coming soon</Typography>
        </Box>

        <Button
          component="a"
          href={model.fileUrl}
          download={`${model.name}.${model.fileType}`}
          variant="contained"
          sx={{
            borderRadius: '12px',
            py: '0.875rem',
            fontSize: '0.95rem',
            fontWeight: 600,
            textTransform: 'none',
            textDecoration: 'none',
          }}
        >
          Download .{model.fileType}
        </Button>
      </Box>
    </Box>
  )
}

export default ModelPage
