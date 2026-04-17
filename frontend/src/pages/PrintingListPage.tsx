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
import { Link, useParams } from 'react-router-dom';
import {
  createPlateGenerationJob,
  createPrintingListArchiveJob,
  fetchPlateGenerationJob,
  fetchPrintingListArchiveJob,
  getPlateGenerationDownloadUrl,
  getPrintingListArchiveDownloadUrl,
  type PlateGenerationJob,
  type PrintingListArchiveJob,
  type SpawnType,
  type HullMode,
} from '../lib/api';
import {
  useModelsByIds,
  usePrintingListDetail,
  useClearPrintingListItems,
  useActivatePrintingList,
  useUpdatePrintingListSettings,
  usePrinters,
  useUpdatePrintingListPrinter,
} from '../lib/queries';
import ModelCard from '../components/ModelCard';
import ConfirmDialog from '../components/ConfirmDialog';
import LoadingView from '../components/LoadingView';
import PrintingListCanvas, { LAYOUT_LOCALSTORAGE_KEY } from '../components/PrintingListCanvas';
import PageLayout from '../components/layouts/PageLayout';
import CardGrid, { DEFAULT_CARD_MIN_WIDTH_PX } from '../components/CardGrid';
import styles from './PrintingListPage.module.css';

function PrintingListPage() {
  const { listId = 'active' } = useParams<{ listId: string }>();

  const { data: list, isPending: listPending } = usePrintingListDetail(listId);
  const modelIds = useMemo(
    () => Array.from(new Set(list?.items.map((i) => i.modelId) ?? [])),
    [list],
  );
  const { data: listedModels = [], isPending: modelsQueryPending } = useModelsByIds(modelIds);
  const { mutate: clearItems } = useClearPrintingListItems();
  const { mutate: activateList } = useActivatePrintingList();
  const { mutate: updateSettings } = useUpdatePrintingListSettings();
  const { mutate: updatePrinter } = useUpdatePrintingListPrinter();
  const { data: printers = [] } = usePrinters();
  const [savingPlate, setSavingPlate] = useState(false);
  const [simulationPaused, setSimulationPaused] = useState(false);
  const [formatMenuAnchor, setFormatMenuAnchor] = useState<HTMLElement | null>(null);
  const [clearDialogOpen, setClearDialogOpen] = useState(false);
  const [archiveJob, setArchiveJob] = useState<PrintingListArchiveJob | null>(null);
  const [archiveError, setArchiveError] = useState<string | null>(null);
  const [archiveDownloading, setArchiveDownloading] = useState(false);
  const [plateJob, setPlateJob] = useState<PlateGenerationJob | null>(null);
  const [plateDownloading, setPlateDownloading] = useState(false);
  const [plateWarning, setPlateWarning] = useState<string | null>(null);
  const [plateError, setPlateError] = useState<string | null>(null);

  const items = useMemo<Record<string, number>>(
    () => (list ? Object.fromEntries(list.items.map((i) => [i.modelId, i.quantity])) : {}),
    [list],
  );

  const listName = list?.name ?? 'Printing list';
  const modelsPending = modelIds.length > 0 && modelsQueryPending;
  const isPending = listPending || modelsPending;
  const hasNonExportableModels = listedModels.some((m) => !m.canExportToPlate);
  const archiveInProgress =
    archiveJob != null && archiveJob.status !== 'failed' && archiveJob.status !== 'completed';
  const archiveBusy = archiveInProgress || archiveDownloading;
  const plateInProgress =
    plateJob != null && plateJob.status !== 'failed' && plateJob.status !== 'completed';
  const plateBusy = savingPlate || plateInProgress || plateDownloading;
  const plateIsSliceJob = plateJob?.format?.startsWith('pngzip') ?? false;

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

  useEffect(() => {
    if (!plateJob || (plateJob.status !== 'queued' && plateJob.status !== 'running')) return;

    let disposed = false;
    let polling = false;

    const pollJob = async () => {
      if (polling) return;
      polling = true;
      try {
        const nextJob = await fetchPlateGenerationJob(plateJob.jobId);
        if (!disposed) setPlateJob(nextJob);
      } catch (error) {
        if (!disposed) {
          const message =
            error instanceof Error ? error.message : 'Failed to fetch plate generation progress';
          setPlateError(message);
          setSavingPlate(false);
          setPlateJob((current) =>
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
  }, [plateJob?.jobId, plateJob?.status]);

  useEffect(() => {
    if (!plateJob || plateJob.status !== 'completed' || plateDownloading) return;

    let disposed = false;

    const downloadPlate = async () => {
      setPlateDownloading(true);
      if (plateJob.warning) {
        const skippedList =
          plateJob.skippedModels.length > 0
            ? ` Skipped: ${plateJob.skippedModels.join(', ')}.`
            : '';
        setPlateWarning(`${plateJob.warning}${skippedList}`);
      }

      try {
        if (disposed) return;

        const url = getPlateGenerationDownloadUrl(plateJob.jobId);
        const anchor = document.createElement('a');
        anchor.href = url;
        anchor.download = plateJob.fileName;
        anchor.style.display = 'none';
        document.body.appendChild(anchor);
        anchor.click();
        anchor.remove();
        setPlateJob(null);
      } catch (error) {
        if (!disposed) {
          setPlateError(error instanceof Error ? error.message : 'Failed to download plate');
        }
      } finally {
        if (!disposed) {
          setSavingPlate(false);
          setPlateDownloading(false);
        }
      }
    };

    void downloadPlate();

    return () => {
      disposed = true;
    };
  }, [plateDownloading, plateJob]);

  useEffect(() => {
    if (plateJob?.status === 'failed') setSavingPlate(false);
  }, [plateJob?.status]);

  function handleSpawnOrderChange(next: SpawnType) {
    if (!list) return;
    updateSettings({ id: list.id, spawnType: next, hullMode: list.hullMode });
  }

  function handleHullModeChange(next: HullMode) {
    if (!list) return;
    updateSettings({ id: list.id, spawnType: list.spawnType, hullMode: next });
  }

  function handlePrinterChange(printerId: string) {
    if (!list) return;
    localStorage.removeItem(LAYOUT_LOCALSTORAGE_KEY);
    updatePrinter({ id: list.id, printerConfigId: printerId });
  }

  async function handleSavePlate(
    format: '3mf' | 'stl' | 'glb' | 'pngzip' | 'pngzip_mesh' | 'pngzip_orthographic' = '3mf',
  ) {
    setPlateWarning(null);
    setPlateError(null);
    setPlateJob(null);
    setSavingPlate(true);
    try {
      let placements: {
        modelId: string;
        instanceIndex: number;
        xMm: number;
        yMm: number;
        angleRad: number;
      }[] = [];
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

      const job = await createPlateGenerationJob(placements, format, list?.printer?.id ?? null);
      setPlateJob(job);
    } catch (error) {
      setPlateError(error instanceof Error ? error.message : 'Failed to generate plate');
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
  const plateCurrentFile = plateJob?.currentEntryName?.split('/').pop() ?? null;
  const plateStatusText = plateDownloading
    ? plateIsSliceJob
      ? 'Downloading slice archive…'
      : 'Downloading plate…'
    : plateJob?.status === 'queued'
      ? plateIsSliceJob
        ? 'Queueing slice export…'
        : 'Queueing plate export…'
      : plateJob?.status === 'running'
        ? plateJob.totalEntries > 0
          ? plateIsSliceJob
            ? `Generating slices ${plateJob.completedEntries} of ${plateJob.totalEntries}…`
            : `Preparing plate ${plateJob.completedEntries} of ${plateJob.totalEntries} models…`
          : plateIsSliceJob
            ? 'Preparing slice export…'
            : 'Preparing plate…'
        : plateJob?.status === 'completed'
          ? plateIsSliceJob
            ? 'Slice archive ready. Starting download…'
            : 'Plate ready. Starting download…'
          : null;

  return (
    <PageLayout spacing={3}>
      <Stack direction="column" spacing={2} alignItems="left">
        <Stack direction="row" alignItems="center" justifyContent="space-between">
          <Typography component="h1" variant="page-title">
            {listName}
          </Typography>
          <Stack direction="row" spacing={1} alignItems="center">
            <Button component={Link} to="/printing-lists" variant="outlined">
              Switch list
            </Button>
            {listedModels.length > 0 && (
              <>
                <Button
                  variant="outlined"
                  onClick={() => {
                    void handleDownloadAllModels();
                  }}
                  disabled={plateBusy || archiveBusy}
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
                    disabled={plateBusy || archiveBusy || !simulationPaused}
                    startIcon={plateBusy ? <CircularProgress size={16} color="inherit" /> : null}
                    sx={{
                      borderRadius: '999px 0 0 999px',
                      borderRight: 'none',
                    }}
                  >
                    {plateBusy ? 'Preparing…' : 'Export plate'}
                  </Button>
                  <Button
                    variant="outlined"
                    onClick={(e) => setFormatMenuAnchor(e.currentTarget)}
                    disabled={plateBusy || archiveBusy || !simulationPaused}
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
            <Button onClick={() => activateList(list.id)} variant="activate">
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
          <MenuItem
            onClick={() => {
              setFormatMenuAnchor(null);
              handleSavePlate('pngzip');
            }}
          >
            Export as PNG slices (default)
          </MenuItem>
          <MenuItem
            onClick={() => {
              setFormatMenuAnchor(null);
              handleSavePlate('pngzip_mesh');
            }}
          >
            Export as PNG slices - Method 1
          </MenuItem>
          <MenuItem
            onClick={() => {
              setFormatMenuAnchor(null);
              handleSavePlate('pngzip_orthographic');
            }}
          >
            Export as PNG slices - Method 2
          </MenuItem>
        </Menu>
      </Stack>

      {(plateStatusText || plateError || plateJob?.errorMessage) && (
        <Box className={styles.archiveStatusCard}>
          {plateStatusText && (
            <Stack spacing={1}>
              <Stack direction="row" spacing={1} alignItems="center">
                <CircularProgress size={16} color="inherit" />
                <Typography className={styles.archiveStatusTitle}>{plateStatusText}</Typography>
              </Stack>
              <LinearProgress
                variant={plateJob && plateJob.totalEntries > 0 ? 'determinate' : 'indeterminate'}
                value={plateJob?.progressPercent ?? 0}
                className={styles.archiveProgress}
              />
              {plateJob && plateJob.totalEntries > 0 && (
                <Typography className={styles.archiveStatusMeta}>
                  {plateJob.progressPercent}% complete
                </Typography>
              )}
              {plateCurrentFile && (
                <Typography className={styles.archiveStatusMeta}>
                  {plateIsSliceJob ? 'Current step' : 'Current model'}: {plateCurrentFile}
                </Typography>
              )}
            </Stack>
          )}

          {(plateError || plateJob?.errorMessage) && (
            <Alert severity="error" onClose={() => setPlateError(null)}>
              {plateError ?? plateJob?.errorMessage}
            </Alert>
          )}
        </Box>
      )}

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

      {(hasNonExportableModels || plateWarning) && (
        <Alert severity="warning">
          {plateWarning ??
            'Some files in this list (LYS, LYT, CTB) cannot be included in exported plates and will be skipped during plate generation.'}
        </Alert>
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
          <CardGrid minCardWidth={DEFAULT_CARD_MIN_WIDTH_PX}>
            {listedModels.map((model) => (
              <ModelCard
                key={model.id}
                model={model}
                href={`/model/${encodeURIComponent(model.id)}`}
              />
            ))}
          </CardGrid>

          <Box className={styles.canvasWrapper}>
            <PrintingListCanvas
              models={listedModels}
              items={items}
              selectedPrinterId={list?.printer?.id ?? null}
              printers={printers}
              bedWidthMm={list?.printer?.bedWidthMm ?? 228}
              bedDepthMm={list?.printer?.bedDepthMm ?? 128}
              onPrinterChange={handlePrinterChange}
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
    </PageLayout>
  );
}

export default PrintingListPage;
