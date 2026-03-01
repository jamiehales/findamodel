import Box from '@mui/material/Box'
import Button from '@mui/material/Button'
import CircularProgress from '@mui/material/CircularProgress'
import Skeleton from '@mui/material/Skeleton'
import Typography from '@mui/material/Typography'
import { useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { generatePlate } from '../lib/api'
import { useModels, usePrintingListDetail, useClearPrintingListItems, useActivatePrintingList } from '../lib/queries'
import ModelCard from '../components/ModelCard'
import PrintingListCanvas, { LAYOUT_LOCALSTORAGE_KEY } from '../components/PrintingListCanvas'

const gridSx = {
  display: 'grid',
  gridTemplateColumns: 'repeat(auto-fill, minmax(160px, 1fr))',
  gap: '0.875rem',
}

function PrintingListPage() {
  const navigate = useNavigate()
  const { listId = 'active' } = useParams<{ listId: string }>()

  const { data: list, isPending: listPending } = usePrintingListDetail(listId)
  const { data: allModels, isPending: modelsPending } = useModels()
  const { mutate: clearItems } = useClearPrintingListItems()
  const { mutate: activateList } = useActivatePrintingList()
  const [savingPlate, setSavingPlate] = useState(false)

  const items: Record<string, number> = list
    ? Object.fromEntries(list.items.map(i => [i.modelId, i.quantity]))
    : {}

  const listName = list?.name ?? 'Printing list'
  const showControls = list?.isActive === true
  const isPending = modelsPending || listPending
  const listedModels = allModels?.filter(m => items[m.id] != null) ?? []

  async function handleSavePlate() {
    setSavingPlate(true)
    try {
      let placements: Parameters<typeof generatePlate>[0] = []
      try {
        const raw = localStorage.getItem(LAYOUT_LOCALSTORAGE_KEY)
        if (raw) {
          const layout = JSON.parse(raw) as {
            positions: { modelId: string; instanceIndex: number; xMm: number; yMm: number; angle: number }[]
          }
          placements = layout.positions.map(p => ({
            modelId: p.modelId,
            instanceIndex: p.instanceIndex,
            xMm: p.xMm,
            yMm: p.yMm,
            angleRad: p.angle,
          }))
        }
      } catch { /* proceed with empty placements */ }

      const blob = await generatePlate(placements)
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = 'plate.stl'
      a.click()
      URL.revokeObjectURL(url)
    } finally {
      setSavingPlate(false)
    }
  }

  return (
    <Box sx={{ minHeight: '100vh', pb: '3rem' }}>
      <Button
        onClick={() => navigate(-1)}
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

      <Box
        sx={{
          pt: '5rem',
          px: '1.25rem',
          maxWidth: 900,
          mx: 'auto',
          display: 'flex',
          flexDirection: 'column',
          gap: '1.5rem',
        }}
      >
        <Box sx={{ display: 'flex', alignItems: 'center', gap: '1rem', flexWrap: 'wrap' }}>
          <Typography
            component="h1"
            sx={{
              fontSize: { xs: '2rem', sm: '2.5rem' },
              fontWeight: 700,
              letterSpacing: '-0.02em',
              color: '#f1f5f9',
              lineHeight: 1.2,
            }}
          >
            {listName}
          </Typography>

          {list && !list.isActive && (
            <Button
              onClick={() => activateList(list.id)}
              sx={{
                background: 'rgba(255,255,255,0.06)',
                backdropFilter: 'blur(8px)',
                color: '#94a3b8',
                border: '1px solid rgba(255,255,255,0.10)',
                borderRadius: '999px',
                px: '1.25rem',
                py: '0.5rem',
                fontSize: '0.9rem',
                fontWeight: 500,
                textTransform: 'none',
                minWidth: 0,
                '&:hover': { background: 'rgba(255,255,255,0.10)', color: '#e2e8f0' },
              }}
            >
              Set active
            </Button>
          )}

          {listedModels.length > 0 && (
            <>
              <Button
                onClick={handleSavePlate}
                disabled={savingPlate}
                startIcon={savingPlate ? <CircularProgress size={16} color="inherit" /> : null}
                sx={{
                  background: 'rgba(99,102,241,0.85)',
                  backdropFilter: 'blur(8px)',
                  color: '#fff',
                  border: '1px solid rgba(255,255,255,0.12)',
                  borderRadius: '999px',
                  px: '1.25rem',
                  py: '0.5rem',
                  fontSize: '0.9rem',
                  fontWeight: 600,
                  textTransform: 'none',
                  minWidth: 0,
                  '&:hover': { background: 'rgba(79,82,211,0.9)' },
                  '&:disabled': { background: 'rgba(99,102,241,0.4)', color: 'rgba(255,255,255,0.6)' },
                }}
              >
                {savingPlate ? 'Preparing…' : 'Save plate'}
              </Button>

              <Button
                onClick={() => list && clearItems(list.id)}
                sx={{
                  background: 'rgba(255,255,255,0.06)',
                  backdropFilter: 'blur(8px)',
                  color: '#94a3b8',
                  border: '1px solid rgba(255,255,255,0.10)',
                  borderRadius: '999px',
                  px: '1.25rem',
                  py: '0.5rem',
                  fontSize: '0.9rem',
                  fontWeight: 500,
                  textTransform: 'none',
                  minWidth: 0,
                  '&:hover': { background: 'rgba(255,255,255,0.10)', color: '#e2e8f0' },
                }}
              >
                Clear list
              </Button>
            </>
          )}
        </Box>

        {isPending ? (
          <Box sx={gridSx}>
            {[1, 2, 3, 4].map(i => (
              <Skeleton
                key={i}
                variant="rectangular"
                sx={{ aspectRatio: '3 / 4', borderRadius: '14px', height: 'auto' }}
              />
            ))}
          </Box>
        ) : listedModels.length === 0 ? (
          <Typography sx={{ color: 'text.secondary', fontSize: '1rem' }}>
            No models added yet. Browse models and use "Add to printing list" to add them here.
          </Typography>
        ) : (
          <>
            <Box sx={gridSx}>
              {listedModels.map(model => (
                <ModelCard
                  key={model.id}
                  model={model}
                  href={`/model/${encodeURIComponent(model.id)}`}
                  showControls={showControls}
                />
              ))}
            </Box>

            <PrintingListCanvas models={listedModels} items={items} />
          </>
        )}
      </Box>
    </Box>
  )
}

export default PrintingListPage
