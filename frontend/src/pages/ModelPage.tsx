import React from 'react'
import Box from '@mui/material/Box'
import Button from '@mui/material/Button'
import CircularProgress from '@mui/material/CircularProgress'
import IconButton from '@mui/material/IconButton'
import Typography from '@mui/material/Typography'
import { useParams, useNavigate } from 'react-router-dom'
import { useModel, useActivePrintingList, useUpsertPrintingListItem } from '../lib/queries'
import ModelViewer from '../components/ModelViewer'
import HullPreview from '../components/HullPreview'
import PathBreadcrumb from '../components/PathBreadcrumb'

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

  const { data: model, isPending, isError } = useModel(decodedId)
  const { data: activeList } = useActivePrintingList()
  const { mutate: upsertItem } = useUpsertPrintingListItem()
  const activeListId = activeList?.id ?? ''
  const qty = model ? (activeList?.items.find(i => i.modelId === model.id)?.quantity ?? 0) : 0

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
    model.creator       && { label: 'Creator',        value: model.creator },
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
          pb: { xs: '3rem', '@supports (paddingBottom: env(safeAreaInsetBottom))': 'calc(3rem + env(safeAreaInsetBottom))' },
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

          <PathBreadcrumb path={model.relativePath} />

          <Typography sx={{ fontSize: '0.85rem', color: '#475569' }}>
            {formatBytes(model.fileSize)}
          </Typography>
        </Box>

        {qty === 0 ? (
          <Button
            onClick={() => upsertItem({ listId: activeListId, modelId: model.id, quantity: qty + 1 })}
            variant="outlined"
            sx={{
              borderRadius: '12px',
              py: '0.875rem',
              fontSize: '0.95rem',
              fontWeight: 600,
              textTransform: 'none',
              borderColor: 'rgba(99,102,241,0.5)',
              color: '#818cf8',
              '&:hover': { borderColor: '#818cf8', background: 'rgba(99,102,241,0.08)' },
            }}
          >
            Add to printing list
          </Button>
        ) : (
          <Box
            sx={{
              display: 'flex',
              alignItems: 'center',
              borderRadius: '12px',
              border: '1px solid rgba(99,102,241,0.4)',
              overflow: 'hidden',
            }}
          >
            <IconButton
              onClick={() => upsertItem({ listId: activeListId, modelId: model.id, quantity: qty - 1 })}
              aria-label="Decrease quantity"
              sx={{
                borderRadius: 0,
                px: '1.25rem',
                py: '0.75rem',
                color: '#818cf8',
                fontSize: '1.25rem',
                '&:hover': { background: 'rgba(99,102,241,0.1)' },
              }}
            >
              −
            </IconButton>
            <Typography
              sx={{
                flex: 1,
                textAlign: 'center',
                fontSize: '1rem',
                fontWeight: 600,
                color: '#e2e8f0',
                userSelect: 'none',
                minWidth: '3rem',
              }}
            >
              {qty}
            </Typography>
            <IconButton
              onClick={() => upsertItem({ listId: activeListId, modelId: model.id, quantity: qty + 1 })}
              aria-label="Increase quantity"
              sx={{
                borderRadius: 0,
                px: '1.25rem',
                py: '0.75rem',
                color: '#818cf8',
                fontSize: '1.25rem',
                '&:hover': { background: 'rgba(99,102,241,0.1)' },
              }}
            >
              +
            </IconButton>
          </Box>
        )}

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

        {metaRows.length > 0 && (
          <Box
            sx={{
              display: 'grid',
              gridTemplateColumns: 'auto 1fr',
              gap: '0.625rem 1.5rem',
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
            height: 380,
            borderRadius: '16px',
            overflow: 'hidden',
            border: '1px solid rgba(255,255,255,0.07)',
          }}
        >
          <ModelViewer modelId={model.id} fileType={model.fileType} convexHull={model.convexHull} />
        </Box>

        {(model.convexHull || model.concaveHull || model.convexSansRaftHull) && (
          <HullPreview
            convexHull={model.convexHull}
            concaveHull={model.concaveHull}
            convexSansRaftHull={model.convexSansRaftHull}
            label="Hull Projections"
          />
        )}
      </Box>
    </Box>
  )
}

export default ModelPage
