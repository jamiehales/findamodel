import { Stack, Box, Button, CircularProgress, Typography, Menu, MenuItem } from '@mui/material'
import ArrowDropDownIcon from '@mui/icons-material/ArrowDropDown'
import { useMemo, useState } from 'react'
import { Link, useParams } from 'react-router-dom'
import { generatePlate, type SpawnType, type HullMode } from '../lib/api'
import { useModels, usePrintingListDetail, useClearPrintingListItems, useActivatePrintingList, useUpdatePrintingListSettings } from '../lib/queries'
import ModelCard from '../components/ModelCard'
import PrintingListCanvas, { LAYOUT_LOCALSTORAGE_KEY } from '../components/PrintingListCanvas'
import styles from './PrintingListPage.module.css'

function PrintingListPage() {
  const { listId = 'active' } = useParams<{ listId: string }>()

  const { data: list, isPending: listPending } = usePrintingListDetail(listId)
  const { data: allModels, isPending: modelsPending } = useModels()
  const { mutate: clearItems } = useClearPrintingListItems()
  const { mutate: activateList } = useActivatePrintingList()
  const { mutate: updateSettings } = useUpdatePrintingListSettings()
  const [savingPlate, setSavingPlate] = useState(false)
  const [simulationPaused, setSimulationPaused] = useState(false)
  const [formatMenuAnchor, setFormatMenuAnchor] = useState<HTMLElement | null>(null)

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

  function handleSpawnOrderChange(next: SpawnType) {
    if (!list) return
    updateSettings({ id: list.id, spawnType: next, hullMode: list.hullMode })
  }

  function handleHullModeChange(next: HullMode) {
    if (!list) return
    updateSettings({ id: list.id, spawnType: list.spawnType, hullMode: next })
  }

  async function handleSavePlate(format: '3mf' | 'stl' | 'glb' = '3mf') {
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

      const blob = await generatePlate(placements, format)
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = format === 'stl' ? 'plate.stl' : format === 'glb' ? 'plate.glb' : 'plate.3mf'
      a.click()
      URL.revokeObjectURL(url)
    } finally {
      setSavingPlate(false)
    }
  }

  return (
    <Box className={styles.page}>
      <Stack direction="column" spacing={2} alignItems="left">
          <Stack direction="row" alignItems="baseline" justifyContent="space-between">
            <Typography component="h1" className={styles.title}>
              {listName}
            </Typography>
            <Button component={Link} to="/printing-lists" className={styles.manageLink}>
              Manage lists
            </Button>
          </Stack>

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
                <Box className={styles.exportGroup}>
                  <Button
                    onClick={() => handleSavePlate('3mf')}
                    disabled={savingPlate || !simulationPaused}
                    startIcon={savingPlate ? <CircularProgress size={16} color="inherit" /> : null}
                    className={styles.btnExportMain}
                  >
                    {savingPlate ? 'Preparing…' : 'Export plate'}
                  </Button>
                  <Button
                    onClick={e => setFormatMenuAnchor(e.currentTarget)}
                    disabled={savingPlate || !simulationPaused}
                    className={styles.btnExportArrow}
                    aria-label="export format options"
                    aria-haspopup="menu"
                  >
                    <ArrowDropDownIcon fontSize="small" />
                  </Button>
                </Box>

                <Menu
                  anchorEl={formatMenuAnchor}
                  open={Boolean(formatMenuAnchor)}
                  onClose={() => setFormatMenuAnchor(null)}
                  anchorOrigin={{ vertical: 'bottom', horizontal: 'right' }}
                  transformOrigin={{ vertical: 'top', horizontal: 'right' }}
                >
                  <MenuItem onClick={() => { setFormatMenuAnchor(null); handleSavePlate('3mf') }}>
                    Export as 3MF
                  </MenuItem>
                  <MenuItem onClick={() => { setFormatMenuAnchor(null); handleSavePlate('stl') }}>
                    Export as STL
                  </MenuItem>
                  <MenuItem onClick={() => { setFormatMenuAnchor(null); handleSavePlate('glb') }}>
                    Export as GLB
                  </MenuItem>
                </Menu>

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
          <Box className={styles.loadingCenter}>
            <CircularProgress color="primary" />
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

            <Box className={styles.canvasWrapper}>
              <PrintingListCanvas
                models={listedModels}
                items={items}
                spawnOrder={list?.spawnType ?? 'grouped'}
                hullMode={list?.hullMode ?? 'convex'}
                onSpawnOrderChange={handleSpawnOrderChange}
                onHullModeChange={handleHullModeChange}
                onPausedChange={setSimulationPaused}
              />
            </Box>
          </>
        )}
    </Box>
  )
}

export default PrintingListPage
