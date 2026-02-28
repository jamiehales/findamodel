import Box from '@mui/material/Box'
import Button from '@mui/material/Button'
import CircularProgress from '@mui/material/CircularProgress'
import Skeleton from '@mui/material/Skeleton'
import Typography from '@mui/material/Typography'
import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { generatePlate } from '../lib/api'
import { useModels } from '../lib/queries'
import { usePrintingList } from '../lib/printingList'
import ModelCard from '../components/ModelCard'
import PrintingListCanvas, { LAYOUT_LOCALSTORAGE_KEY } from '../components/PrintingListCanvas'

const gridSx = {
  display: 'grid',
  gridTemplateColumns: 'repeat(auto-fill, minmax(160px, 1fr))',
  gap: '0.875rem',
}

function PrintingListPage() {
  const navigate = useNavigate()
  const { items } = usePrintingList()
  const { data: allModels, isPending } = useModels()
  const [savingPlate, setSavingPlate] = useState(false)

  const listedModels = allModels?.filter(m => items[m.id] != null) ?? []

  async function handleSavePlate() {
    setSavingPlate(true)
    try {
      // Read the settled layout from localStorage (written by PrintingListCanvas when bodies stop moving)
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
      } catch { /* proceed with empty placements if layout is unavailable */ }

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

  return (
    <Box sx={{ minHeight: '100vh', pb: '3rem' }}>
      {backButton}

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
            Printing list
          </Typography>

          {listedModels.length > 0 && (
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
                <Box key={model.id} sx={{ position: 'relative' }}>
                  <ModelCard
                    model={model}
                    onClick={() => navigate(`/model/${encodeURIComponent(model.id)}`)}
                  />
                  <Box
                    sx={{
                      position: 'absolute',
                      top: '0.5rem',
                      right: '0.5rem',
                      background: 'rgba(99,102,241,0.85)',
                      backdropFilter: 'blur(4px)',
                      color: '#fff',
                      fontSize: '0.75rem',
                      fontWeight: 700,
                      px: '0.5rem',
                      py: '0.2rem',
                      borderRadius: '6px',
                      pointerEvents: 'none',
                      zIndex: 2,
                    }}
                  >
                    ×{items[model.id]}
                  </Box>
                </Box>
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
