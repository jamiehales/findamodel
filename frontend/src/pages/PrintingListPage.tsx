import {
  Stack,
  Box,
  Button,
  CircularProgress,
  Typography,
  Menu,
  MenuItem,
} from '@mui/material';
import ArrowDropDownIcon from '@mui/icons-material/ArrowDropDown';
import { useMemo, useState } from 'react';
import { useParams } from 'react-router-dom';
import { generatePlate, type SpawnType, type HullMode } from '../lib/api';
import {
  useModels,
  usePrintingListDetail,
  useClearPrintingListItems,
  useActivatePrintingList,
  useUpdatePrintingListSettings,
} from '../lib/queries';
import ModelCard from '../components/ModelCard';
import ConfirmDialog from '../components/ConfirmDialog';
import PrintingListCanvas, { LAYOUT_LOCALSTORAGE_KEY } from '../components/PrintingListCanvas';
import styles from './PrintingListPage.module.css';

function PrintingListPage() {
  const { listId = 'active' } = useParams<{ listId: string }>();

  const { data: list, isPending: listPending } = usePrintingListDetail(listId);
  const { data: allModels, isPending: modelsPending } = useModels();
  const { mutate: clearItems } = useClearPrintingListItems();
  const { mutate: activateList } = useActivatePrintingList();
  const { mutate: updateSettings } = useUpdatePrintingListSettings();
  const [savingPlate, setSavingPlate] = useState(false);
  const [simulationPaused, setSimulationPaused] = useState(false);
  const [formatMenuAnchor, setFormatMenuAnchor] = useState<HTMLElement | null>(null);
  const [clearDialogOpen, setClearDialogOpen] = useState(false);

  const items = useMemo<Record<string, number>>(
    () => (list ? Object.fromEntries(list.items.map((i) => [i.modelId, i.quantity])) : {}),
    [list],
  );
  const listedModels = useMemo(
    () => allModels?.filter((m) => items[m.id] != null) ?? [],
    [allModels, items],
  );

  const listName = list?.name ?? 'Printing list';
  const isPending = modelsPending || listPending;

  function handleSpawnOrderChange(next: SpawnType) {
    if (!list) return;
    updateSettings({ id: list.id, spawnType: next, hullMode: list.hullMode });
  }

  function handleHullModeChange(next: HullMode) {
    if (!list) return;
    updateSettings({ id: list.id, spawnType: list.spawnType, hullMode: next });
  }

  async function handleSavePlate(format: '3mf' | 'stl' | 'glb' = '3mf') {
    setSavingPlate(true);
    try {
      let placements: Parameters<typeof generatePlate>[0] = [];
      try {
        const raw = localStorage.getItem(LAYOUT_LOCALSTORAGE_KEY);
        if (raw) {
          const layout = JSON.parse(raw) as {
            positions: {
              modelId: string;
              instanceIndex: number;
              xMm: number;
              yMm: number;
              angle: number;
            }[];
          };
          placements = layout.positions.map((p) => ({
            modelId: p.modelId,
            instanceIndex: p.instanceIndex,
            xMm: p.xMm,
            yMm: p.yMm,
            angleRad: p.angle,
          }));
        }
      } catch {
        /* proceed with empty placements */
      }

      const blob = await generatePlate(placements, format);
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = format === 'stl' ? 'plate.stl' : format === 'glb' ? 'plate.glb' : 'plate.3mf';
      a.click();
      URL.revokeObjectURL(url);
    } finally {
      setSavingPlate(false);
    }
  }

  function handleClearList() {
    setClearDialogOpen(true);
  }

  function handleConfirmClearList() {
    if (!list) return;
    clearItems(list.id);
    setClearDialogOpen(false);
  }

  return (
    <Box className={styles.page}>
      <Stack direction="column" spacing={2} alignItems="left">
        <Stack direction="row" alignItems="baseline" justifyContent="space-between">
          <Typography component="h1" className={styles.title}>
            {listName}
          </Typography>
          <Stack direction="row" spacing={1} alignItems="center">
            {listedModels.length > 0 && (
              <>
                <Box sx={{ display: 'flex' }}>
                  <Button
                    variant="outlined"
                    onClick={() => handleSavePlate('3mf')}
                    disabled={savingPlate || !simulationPaused}
                    startIcon={savingPlate ? <CircularProgress size={16} color="inherit" /> : null}
                    sx={{
                      borderRadius: '999px 0 0 999px',
                      borderRight: 'none',
                    }}
                  >
                    {savingPlate ? 'Preparing…' : 'Export plate'}
                  </Button>
                  <Button
                    variant="outlined"
                    onClick={(e) => setFormatMenuAnchor(e.currentTarget)}
                    disabled={savingPlate || !simulationPaused}
                    aria-label="export format options"
                    aria-haspopup="menu"
                    sx={{
                      borderRadius: '0 999px 999px 0',
                      px: '0.55rem',
                      minWidth: 0,
                    }}
                  >
                    <ArrowDropDownIcon fontSize="small" />
                  </Button>
                </Box>
                <Button variant="warning" onClick={handleClearList}>
                  Clear list
                </Button>
              </>
            )}
          </Stack>
        </Stack>

        <Stack direction="row" spacing={1} alignItems="center">
          {list && !list.isActive && (
            <Button onClick={() => activateList(list.id)} className={styles.btnActivate}>
              Set active
            </Button>
          )}
        </Stack>

        <Menu
          anchorEl={formatMenuAnchor}
          open={Boolean(formatMenuAnchor)}
          onClose={() => setFormatMenuAnchor(null)}
          anchorOrigin={{ vertical: 'bottom', horizontal: 'right' }}
          transformOrigin={{ vertical: 'top', horizontal: 'right' }}
        >
          <MenuItem
            onClick={() => {
              setFormatMenuAnchor(null);
              handleSavePlate('3mf');
            }}
          >
            Export as 3MF
          </MenuItem>
          <MenuItem
            onClick={() => {
              setFormatMenuAnchor(null);
              handleSavePlate('stl');
            }}
          >
            Export as STL
          </MenuItem>
          <MenuItem
            onClick={() => {
              setFormatMenuAnchor(null);
              handleSavePlate('glb');
            }}
          >
            Export as GLB
          </MenuItem>
        </Menu>
      </Stack>

      {isPending ? (
        <Box className={styles.loadingCenter}>
          <CircularProgress color="primary" />
        </Box>
      ) : listedModels.length === 0 ? (
        <Box className={styles.emptyState}>
          <Typography className={styles.emptyTitle}>No models added yet</Typography>
          <Typography color="text.secondary" className={styles.emptySubtext}>
            Browse models and use "Add to printing list" to add them here.
          </Typography>
        </Box>
      ) : (
        <>
          <Box className={styles.grid}>
            {listedModels.map((model) => (
              <ModelCard
                key={model.id}
                model={model}
                href={`/model/${encodeURIComponent(model.id)}`}
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

      <ConfirmDialog
        open={clearDialogOpen}
        title="Clear list?"
        message={`Are you sure you want to clear all items from "${list?.name ?? 'this list'}"?`}
        confirmLabel="Clear list"
        onConfirm={handleConfirmClearList}
        onCancel={() => setClearDialogOpen(false)}
      />
    </Box>
  );
}

export default PrintingListPage;
