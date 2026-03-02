import { Stack, Box, Button, CircularProgress, Skeleton, Typography } from '@mui/material'
import { useMemo, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { generatePlate } from '../lib/api'
import { useModels, usePrintingListDetail, useClearPrintingListItems, useActivatePrintingList } from '../lib/queries'
import ModelCard from '../components/ModelCard'
import PrintingListCanvas, { LAYOUT_LOCALSTORAGE_KEY } from '../components/PrintingListCanvas'
import styles from './PrintingListPage.module.css'

function PrintingListPage() {
  const navigate = useNavigate()
  const { listId = 'active' } = useParams<{ listId: string }>()

  const { data: list, isPending: listPending } = usePrintingListDetail(listId)
  const { data: allModels, isPending: modelsPending } = useModels()
  const { mutate: clearItems } = useClearPrintingListItems()
  const { mutate: activateList } = useActivatePrintingList()
  const [savingPlate, setSavingPlate] = useState(false)
  const [simulationPaused, setSimulationPaused] = useState(false)

  const items = useMemo<Record<string, number>>(
    () => list ? Object.fromEntries(list.items.map(i => [i.modelId, i.quantity])) : {},
    [list],
  )
  const listedModels = useMemo(
    () => allModels?.filter(m => items[m.id] != null) ?? [],
    [allModels, items],
  )

  const listName = list?.name ?? 'Printing list'
  const showControls = list?.isActive === true
  const isPending = modelsPending || listPending

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
    <Box className={styles.page}>
      <Button variant="back" onClick={() => navigate(-1)}>
        ← Back
      </Button>

      <Box className={styles.content}>
        <Stack direction="column" spacing={2} alignItems="left">
          <Typography component="h1" className={styles.title}>
            {listName}
          </Typography>

          <Stack direction="row" spacing={1} alignItems="center">
            {list && !list.isActive && (
              <Button
                onClick={() => activateList(list.id)}
                className={styles.btnActivate}
              >
                Set active
              </Button>
            )}

            {listedModels.length > 0 && (
              <>
                <Button
                  onClick={handleSavePlate}
                  disabled={savingPlate || !simulationPaused}
                  startIcon={savingPlate ? <CircularProgress size={16} color="inherit" /> : null}
                  className={styles.btnExport}
                >
                  {savingPlate ? 'Preparing…' : 'Export plate'}
                </Button>

                <Button
                  onClick={() => list && clearItems(list.id)}
                  className={styles.btnClear}
                >
                  Clear list
                </Button>
              </>
            )}
          </Stack>
        </Stack>

        {isPending ? (
          <Box className={styles.grid}>
            {[1, 2, 3, 4].map(i => (
              <Skeleton
                key={i}
                variant="rectangular"
                className={styles.skeleton}
              />
            ))}
          </Box>
        ) : listedModels.length === 0 ? (
          <Typography color="text.secondary" className={styles.emptyText}>
            No models added yet. Browse models and use "Add to printing list" to add them here.
          </Typography>
        ) : (
          <>
            <Box className={styles.grid}>
              {listedModels.map(model => (
                <ModelCard
                  key={model.id}
                  model={model}
                  href={`/model/${encodeURIComponent(model.id)}`}
                  showControls={showControls}
                />
              ))}
            </Box>

            <PrintingListCanvas models={listedModels} items={items} onPausedChange={setSimulationPaused} />
          </>
        )}
      </Box>
    </Box>
  )
}

export default PrintingListPage
