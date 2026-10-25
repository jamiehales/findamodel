import { ChevronDown, Download, Trash2 } from 'lucide-react';
import {
  Alert,
  Button,
  IconButton,
  Menu,
  MenuContent,
  MenuItem,
  MenuTrigger,
  Progress,
  Spinner,
  Tag,
} from '@jamiehales/ui-kit/mostlymaking';
import { useEffect, useMemo, useState } from 'react';
import { Link, useParams } from 'react-router-dom';
import {
  createPlateGenerationJob,
  createPrintingListArchiveJob,
  fetchPlateGenerationJob,
  fetchPrintingListArchiveJob,
  getPlateGenerationDownloadUrl,
  getPrintingListArchiveDownloadUrl,
  type PlateFormat,
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
  useUpsertPrintingListItem,
} from '../lib/queries';
import QuantityStepper from '../components/QuantityStepper';
import ErrorBoundary from '../components/ErrorBoundary';
import { useRenderControls } from '../components/RenderControlsContext';
import { withPreviewSupports } from '../lib/http';
import ConfirmDialog from '../components/ConfirmDialog';
import LoadingView from '../components/LoadingView';
import PrintingListCanvas, { LAYOUT_LOCALSTORAGE_KEY } from '../components/PrintingListCanvas';
import PageLayout, { PageHeader } from '../components/layouts/PageLayout';
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
  const { mutate: upsertItem } = useUpsertPrintingListItem();
  const { showSupports } = useRenderControls();
  const { data: printers = [] } = usePrinters();
  const [savingPlate, setSavingPlate] = useState(false);
  const [simulationPaused, setSimulationPaused] = useState(false);
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
  const bedWidthMm = list?.printer?.bedWidthMm ?? 228;
  const bedDepthMm = list?.printer?.bedDepthMm ?? 128;
  const modelsPending = modelIds.length > 0 && modelsQueryPending;
  const isPending = listPending || modelsPending;
  const hasNonExportableModels = listedModels.some((m) => !m.canExportToPlate);
  const archiveInProgress =
    archiveJob != null && archiveJob.status !== 'failed' && archiveJob.status !== 'completed';
  const archiveBusy = archiveInProgress || archiveDownloading;
  const plateInProgress =
    plateJob != null && plateJob.status !== 'failed' && plateJob.status !== 'completed';
  const plateBusy = savingPlate || plateInProgress || plateDownloading;

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

  async function handleSavePlate(format: PlateFormat = '3mf') {
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

      const job = await createPlateGenerationJob(placements, format, {
        bedWidthMm,
        bedDepthMm,
        plateName: list?.name,
      });
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
    ? 'Downloading zip'
    : archiveJob?.status === 'queued'
      ? 'Queueing zip'
      : archiveJob?.status === 'running'
        ? archiveJob.totalEntries > 0
          ? `Packaging ${archiveJob.completedEntries} of ${archiveJob.totalEntries} files`
          : 'Packaging zip'
        : archiveJob?.status === 'completed'
          ? 'Zip ready, downloading'
          : null;
  const plateCurrentFile = plateJob?.currentEntryName?.split('/').pop() ?? null;
  const plateStatusText = plateDownloading
    ? 'Downloading plate'
    : plateJob?.status === 'queued'
      ? 'Queueing plate export'
      : plateJob?.status === 'running'
        ? plateJob.totalEntries > 0
          ? `Preparing plate, ${plateJob.completedEntries} of ${plateJob.totalEntries} models`
          : 'Preparing plate'
        : plateJob?.status === 'completed'
          ? 'Plate ready, downloading'
          : null;

  const totalQuantity = list?.items.reduce((sum, i) => sum + i.quantity, 0) ?? 0;
  const exportDisabled = plateBusy || archiveBusy || !simulationPaused;

  return (
    <PageLayout>
      <PageHeader
        title={listName}
        meta={
          <>
            {list?.isActive ? (
              <Tag variant="success">Active</Tag>
            ) : list ? (
              <Button size="sm" variant="secondary" onClick={() => activateList(list.id)}>
                Set active
              </Button>
            ) : null}
            <Button asChild size="sm" variant="ghost">
              <Link to="/printing-lists">Switch list</Link>
            </Button>
          </>
        }
        actions={
          listedModels.length > 0 ? (
            <>
              <Button variant="ghost" className={styles.clear} onClick={handleClearList}>
                Clear
              </Button>
              <Button
                variant="secondary"
                onClick={() => {
                  void handleDownloadAllModels();
                }}
                disabled={plateBusy || archiveBusy}
              >
                {archiveBusy ? (
                  <Spinner label="Preparing zip" />
                ) : (
                  <Download size={16} strokeWidth={1.5} />
                )}
                Download all
              </Button>
              <div className={styles.split}>
                <Button
                  variant="primary"
                  onClick={() => handleSavePlate('3mf')}
                  disabled={exportDisabled}
                >
                  {plateBusy ? <Spinner label="Preparing plate" /> : null}
                  Export plate
                </Button>
                <Menu>
                  <MenuTrigger asChild>
                    <IconButton
                      variant="primary"
                      aria-label="Export format"
                      disabled={exportDisabled}
                      className={styles.splitMenu}
                    >
                      <ChevronDown size={16} strokeWidth={2} />
                    </IconButton>
                  </MenuTrigger>
                  <MenuContent align="end">
                    <MenuItem onSelect={() => handleSavePlate('3mf')}>Export as 3MF</MenuItem>
                    <MenuItem onSelect={() => handleSavePlate('stl')}>Export as STL</MenuItem>
                    <MenuItem onSelect={() => handleSavePlate('glb')}>Export as GLB</MenuItem>
                    <MenuItem onSelect={() => handleSavePlate('voxl')}>Export as VOXL</MenuItem>
                  </MenuContent>
                </Menu>
              </div>
            </>
          ) : undefined
        }
      />

      {plateStatusText && (
        <JobStatus text={plateStatusText} job={plateJob} currentFile={plateCurrentFile} />
      )}
      {(plateError || plateJob?.errorMessage) && (
        <Alert tone="error">
          <div className={styles.alertRow}>
            <span>{plateError ?? plateJob?.errorMessage}</span>
            <Button size="sm" variant="ghost" onClick={() => setPlateError(null)}>
              Dismiss
            </Button>
          </div>
        </Alert>
      )}

      {archiveStatusText && (
        <JobStatus text={archiveStatusText} job={archiveJob} currentFile={archiveCurrentFile} />
      )}
      {(archiveError || archiveJob?.errorMessage) && (
        <Alert tone="error">
          <div className={styles.alertRow}>
            <span>{archiveError ?? archiveJob?.errorMessage}</span>
            <Button size="sm" variant="ghost" onClick={() => setArchiveError(null)}>
              Dismiss
            </Button>
          </div>
        </Alert>
      )}

      {(hasNonExportableModels || plateWarning) && (
        <Alert>
          {plateWarning ??
            "LYS, LYT and CTB files can't go on an exported plate, so they'll be skipped."}
        </Alert>
      )}

      {isPending ? (
        <LoadingView />
      ) : listedModels.length === 0 ? (
        <div className={styles.empty}>
          <p className={styles.emptyTitle}>No models in this list.</p>
          <Button asChild variant="secondary">
            <Link to="/">Browse models</Link>
          </Button>
        </div>
      ) : (
        <div className={styles.layout}>
          <div className={styles.plateCol}>
            <ErrorBoundary
              fallback={
                <Alert tone="error">
                  Couldn't start the plate view. This browser may not support WebGL.
                </Alert>
              }
            >
              <PrintingListCanvas
                models={listedModels}
                items={items}
                selectedPrinterId={list?.printer?.id ?? null}
                printers={printers}
                bedWidthMm={bedWidthMm}
                bedDepthMm={bedDepthMm}
                onPrinterChange={handlePrinterChange}
                spawnOrder={list?.spawnType ?? 'grouped'}
                hullMode={list?.hullMode ?? 'convex'}
                onSpawnOrderChange={handleSpawnOrderChange}
                onHullModeChange={handleHullModeChange}
                onPausedChange={setSimulationPaused}
              />
            </ErrorBoundary>
          </div>

          <section className={styles.list} aria-labelledby="list-heading">
            <div className={styles.listHeader}>
              <h2 id="list-heading" className="mm-type-label">
                Models
              </h2>
              <span className={styles.mono}>{totalQuantity} total</span>
            </div>
            <ul className={styles.rows}>
              {listedModels.map((model) => {
                const quantity = items[model.id] ?? 0;
                const preview = model.previewUrl
                  ? withPreviewSupports(model.previewUrl, showSupports)
                  : null;
                const setQuantity = (next: number) =>
                  list && upsertItem({ listId: list.id, modelId: model.id, quantity: next });
                return (
                  <li key={model.id} className={styles.row}>
                    <span className={styles.thumb}>
                      {preview ? <img src={preview} alt="" loading="lazy" /> : null}
                    </span>
                    <Link to={`/model/${encodeURIComponent(model.id)}`} className={styles.rowName}>
                      <span className={styles.name}>{model.name}</span>
                      <span className={styles.file}>{model.relativePath.split('/').pop()}</span>
                    </Link>
                    <QuantityStepper
                      size="sm"
                      min={1}
                      value={quantity}
                      label={model.name}
                      onChange={setQuantity}
                    />
                    <IconButton
                      variant="ghost"
                      size="sm"
                      aria-label={`Remove ${model.name}`}
                      onClick={() => setQuantity(0)}
                    >
                      <Trash2 size={16} strokeWidth={1.5} />
                    </IconButton>
                  </li>
                );
              })}
            </ul>
          </section>
        </div>
      )}

      <ConfirmDialog
        open={clearDialogOpen}
        title={`Clear ${list?.name ?? 'this list'}?`}
        message="Every model is taken off the list."
        confirmLabel="Clear"
        onConfirm={handleConfirmClearList}
        onCancel={() => setClearDialogOpen(false)}
      />
    </PageLayout>
  );
}

function JobStatus({
  text,
  job,
  currentFile,
}: {
  text: string;
  job: { totalEntries: number; progressPercent: number } | null;
  currentFile: string | null;
}) {
  const determinate = !!job && job.totalEntries > 0;
  return (
    <div className={styles.status} role="status">
      <Spinner label={text} />
      <span>{text}</span>
      <Progress
        value={determinate ? job!.progressPercent : undefined}
        className={styles.statusBar}
        aria-label={text}
      />
      {currentFile && <span className={styles.mono}>{currentFile}</span>}
    </div>
  );
}

export default PrintingListPage;
