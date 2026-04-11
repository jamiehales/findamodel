import {
  Stack,
  Box,
  Button,
  CircularProgress,
  Typography,
  Menu,
  MenuItem,
  Alert,
  LinearProgress,
} from '@mui/material';
import ArrowDropDownIcon from '@mui/icons-material/ArrowDropDown';
import DownloadRoundedIcon from '@mui/icons-material/DownloadRounded';
import { useEffect, useMemo, useState } from 'react';
import { useParams } from 'react-router-dom';
import {
  createPrintingListArchiveJob,
  fetchPrintingListArchiveJob,
  generatePlate,
  getPrintingListArchiveDownloadUrl,
  type PrintingListArchiveJob,
  type SpawnType,
  type HullMode,
} from '../lib/api';
import {
  useModels,
  usePrintingListDetail,
  useClearPrintingListItems,
  useActivatePrintingList,
  useUpdatePrintingListSettings,
} from '../lib/queries';
import ModelCard from '../components/ModelCard';
import ConfirmDialog from '../components/ConfirmDialog';
import LoadingView from '../components/LoadingView';
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
  const [archiveJob, setArchiveJob] = useState<PrintingListArchiveJob | null>(null);
  const [archiveError, setArchiveError] = useState<string | null>(null);
  const [archiveDownloading, setArchiveDownloading] = useState(false);

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
  const archiveInProgress =
    archiveJob != null && archiveJob.status !== 'failed' && archiveJob.status !== 'completed';
  const archiveBusy = archiveInProgress || archiveDownloading;

  useEffect(() => {
    if (!archiveJob || (archiveJob.status !== 'queued' && archiveJob.status !== 'running')) return;

    let disposed = false;
    let polling = false;

    const pollJob = async () => {
      if (polling) return;
      polling = true;
      try {
        const nextJob = await fetchPrintingListArchiveJob(archiveJob.jobId);
        if (!disposed) setArchiveJob(nextJob);
      } catch (error) {
        if (!disposed) {
          const message =
            error instanceof Error ? error.message : 'Failed to fetch archive progress';
          setArchiveError(message);
          setArchiveJob((current) =>
            current
              ? {
                  ...current,
                  status: 'failed',
                  errorMessage: message,
                }
              : current,
          );
        }
      } finally {
        polling = false;
      }
    };

    void pollJob();
    const intervalId = window.setInterval(() => {
      void pollJob();
    }, 500);

    return () => {
      disposed = true;
      window.clearInterval(intervalId);
    };
  }, [archiveJob?.jobId, archiveJob?.status]);

  useEffect(() => {
    if (!archiveJob || archiveJob.status !== 'completed' || archiveDownloading) return;

    let disposed = false;

    const downloadArchive = async () => {
      setArchiveDownloading(true);
      try {
        if (disposed) return;

        const url = getPrintingListArchiveDownloadUrl(archiveJob.jobId);
        const anchor = document.createElement('a');
        anchor.href = url;
        anchor.download = archiveJob.fileName;
        anchor.style.display = 'none';
        document.body.appendChild(anchor);
        anchor.click();
        anchor.remove();
        setArchiveJob(null);
      } catch (error) {
        if (!disposed) {
          setArchiveError(
            error instanceof Error ? error.message : 'Failed to download printing list archive',
          );
        }
      } finally {
        if (!disposed) setArchiveDownloading(false);
      }
    };

    void downloadArchive();

    return () => {
      disposed = true;
    };
  }, [archiveDownloading, archiveJob]);

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

  async function handleDownloadAllModels(flatten = true) {
    if (!list) return;

    setArchiveError(null);

    try {
      const nextJob = await createPrintingListArchiveJob(list.id, { flatten });
      setArchiveJob(nextJob);
    } catch (error) {
      setArchiveError(error instanceof Error ? error.message : 'Failed to start archive');
    }
  }

  function handleConfirmClearList() {
    if (!list) return;
    clearItems(list.id);
    setClearDialogOpen(false);
  }

  const archiveCurrentFile = archiveJob?.currentEntryName?.split('/').pop() ?? null;
  const archiveStatusText = archiveDownloading
    ? 'Downloading zip…'
    : archiveJob?.status === 'queued'
      ? 'Queueing zip…'
      : archiveJob?.status === 'running'
        ? archiveJob.totalEntries > 0
          ? `Packaging ${archiveJob.completedEntries} of ${archiveJob.totalEntries} files…`
          : 'Packaging zip…'
        : archiveJob?.status === 'completed'
          ? 'Zip ready. Starting download…'
          : null;

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
                <Button
                  variant="outlined"
                  onClick={() => {
                    void handleDownloadAllModels();
                  }}
                  disabled={savingPlate || archiveBusy}
                  startIcon={
                    archiveBusy ? (
                      <CircularProgress size={16} color="inherit" />
                    ) : (
                      <DownloadRoundedIcon fontSize="small" />
                    )
                  }
                >
                  {archiveBusy ? 'Preparing zip…' : 'Download all models'}
                </Button>
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

      {(archiveStatusText || archiveError || archiveJob?.errorMessage) && (
        <Box className={styles.archiveStatusCard}>
          {archiveStatusText && (
            <Stack spacing={1}>
              <Stack direction="row" spacing={1} alignItems="center">
                <CircularProgress size={16} color="inherit" />
                <Typography className={styles.archiveStatusTitle}>{archiveStatusText}</Typography>
              </Stack>
              <LinearProgress
                variant={
                  archiveJob && archiveJob.totalEntries > 0 ? 'determinate' : 'indeterminate'
                }
                value={archiveJob?.progressPercent ?? 0}
                className={styles.archiveProgress}
              />
              {archiveJob && archiveJob.totalEntries > 0 && (
                <Typography className={styles.archiveStatusMeta}>
                  {archiveJob.progressPercent}% complete
                </Typography>
              )}
              {archiveCurrentFile && (
                <Typography className={styles.archiveStatusMeta}>
                  Current file: {archiveCurrentFile}
                </Typography>
              )}
            </Stack>
          )}

          {(archiveError || archiveJob?.errorMessage) && (
            <Alert severity="error" onClose={() => setArchiveError(null)}>
              {archiveError ?? archiveJob?.errorMessage}
            </Alert>
          )}
        </Box>
      )}

      {isPending ? (
        <LoadingView />
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
