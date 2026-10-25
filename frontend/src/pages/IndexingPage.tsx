import { useEffect, useMemo, useRef, useState } from 'react';
import { Square } from 'lucide-react';
import {
  Button,
  IconButton,
  Progress,
  SegmentedControl,
  SegmentedControlItem,
  Spinner,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  Tag,
  Tooltip,
} from '@jamiehales/ui-kit/mostlymaking';
import {
  IndexFlags,
  type IndexRunFilesView,
  type IndexRunDetail,
  type IndexRunEvent,
  type IndexRunFile,
  type IndexRunSummary,
} from '../lib/api';
import {
  useCancelIndexerRun,
  useEnqueueIndex,
  useIndexerRun,
  useIndexerRuns,
  useIndexerStatus,
} from '../lib/queries';
import ErrorView from '../components/ErrorView';
import IndexRunStages from '../components/IndexRunStages';
import LoadingView from '../components/LoadingView';
import PageLayout, { PageHeader } from '../components/layouts/PageLayout';
import Pagination from '../components/Pagination';
import styles from './IndexingPage.module.css';

function formatDate(value: string | null): string {
  if (!value) return '-';
  return new Date(value).toLocaleString();
}

function formatDuration(startedAt: string | null, completedAt: string | null): string {
  if (!startedAt) return '-';
  const start = new Date(startedAt).getTime();
  const end = completedAt ? new Date(completedAt).getTime() : Date.now();
  const seconds = Math.max(0, Math.floor((end - start) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ${seconds % 60}s`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

function formatElapsedMs(durationMs: number): string {
  const ms = Math.max(0, Math.floor(durationMs));
  if (ms < 1000) return '<1s';
  const seconds = Math.floor(ms / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ${seconds % 60}s`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

function formatDurationMs(durationMs: number | null): string {
  if (durationMs == null) return '-';
  const ms = Math.max(0, Math.round(durationMs));
  if (ms < 1000) return `${ms}ms`;
  const seconds = Math.floor(ms / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ${seconds % 60}s`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

function normalizePaged<T>(value: unknown): {
  items: T[];
  page: number;
  pageSize: number;
  totalCount: number;
} {
  if (Array.isArray(value)) {
    return {
      items: value as T[],
      page: 1,
      pageSize: value.length,
      totalCount: value.length,
    };
  }

  if (value && typeof value === 'object') {
    const record = value as {
      items?: unknown;
      page?: unknown;
      pageSize?: unknown;
      totalCount?: unknown;
    };
    const items = Array.isArray(record.items) ? (record.items as T[]) : [];
    const page = typeof record.page === 'number' && Number.isFinite(record.page) ? record.page : 1;
    const pageSize =
      typeof record.pageSize === 'number' && Number.isFinite(record.pageSize)
        ? record.pageSize
        : items.length;
    const totalCount =
      typeof record.totalCount === 'number' && Number.isFinite(record.totalCount)
        ? record.totalCount
        : items.length;
    return { items, page, pageSize, totalCount };
  }

  return { items: [], page: 1, pageSize: 0, totalCount: 0 };
}

function resolveFileDurationMs(file: unknown, run: IndexRunSummary): number | null {
  if (!file || typeof file !== 'object') return null;
  const record = file as { durationMs?: unknown; DurationMs?: unknown; processedAt?: unknown };

  const rawDuration =
    typeof record.durationMs === 'number'
      ? record.durationMs
      : typeof record.DurationMs === 'number'
        ? record.DurationMs
        : null;
  if (rawDuration != null && Number.isFinite(rawDuration)) return rawDuration;

  if (typeof record.processedAt === 'string' && run.startedAt) {
    const processedAtMs = new Date(record.processedAt).getTime();
    const startedAtMs = new Date(run.startedAt).getTime();
    if (
      Number.isFinite(processedAtMs) &&
      Number.isFinite(startedAtMs) &&
      processedAtMs >= startedAtMs
    )
      return processedAtMs - startedAtMs;
  }

  return null;
}

function formatRunProcessingTime(run: IndexRunSummary): string {
  if (run.startedAt) return formatDuration(run.startedAt, run.completedAt);
  if (run.status === 'running' && run.requestedAt) return formatDuration(run.requestedAt, null);
  return '-';
}

function flagsLabel(flags: number): string {
  const parts: string[] = [];
  if (flags & IndexFlags.Directories) parts.push('Directories');
  if (flags & IndexFlags.Models) parts.push('Models');
  return parts.join(' + ') || 'None';
}

function targetLabel(run: Pick<IndexRunSummary, 'directoryFilter' | 'relativeModelPath'>): string {
  return run.relativeModelPath ?? run.directoryFilter ?? 'All directories';
}

const STATUS_LABELS: Record<string, string> = {
  cancelling: 'Cancelling',
  running: 'Running',
  queued: 'Queued',
  failed: 'Failed',
  cancelled: 'Cancelled',
  success: 'Success',
};

function StatusTag({ status }: { status: string }) {
  const label = STATUS_LABELS[status] ?? 'Success';
  if (status === 'running' || status === 'cancelling')
    return <Tag className={styles.running}>{label}</Tag>;
  if (status === 'failed') return <Tag className={styles.failed}>{label}</Tag>;
  if (status === 'queued' || status === 'cancelled') return <Tag variant="neutral">{label}</Tag>;
  return <Tag variant="success">{label}</Tag>;
}

const FILE_STATUS_TAG: Record<IndexRunFile['status'], 'success' | 'neutral' | 'outline'> = {
  processed: 'success',
  pending: 'neutral',
  skipped: 'outline',
  failed: 'outline',
};

const FILES_PAGE_SIZE = 200;
const EVENTS_PAGE_SIZE = 200;

function RunSummaryCard({
  run,
  selected,
  selectable,
  onSelect,
  canCancel,
  onCancel,
  isCancelling,
}: {
  run: IndexRunSummary;
  selected: boolean;
  selectable: boolean;
  onSelect: () => void;
  canCancel: boolean;
  onCancel: () => void;
  isCancelling: boolean;
}) {
  const [, setTick] = useState(0);
  const firstSeenAtMsRef = useRef<number>(Date.now());
  useEffect(() => {
    const id = setInterval(() => setTick((n) => n + 1), 1000);
    return () => clearInterval(id);
  }, []);

  const total = run.totalFiles ?? 0;
  const percent = total > 0 ? Math.round((run.processedFiles / total) * 100) : 0;
  const runtimeLabel = (() => {
    if (run.status !== 'running') return formatRunProcessingTime(run);

    const serverStart = run.startedAt ?? run.requestedAt;
    if (!serverStart) return formatElapsedMs(Date.now() - firstSeenAtMsRef.current);

    const serverElapsedMs = Date.now() - new Date(serverStart).getTime();
    const localElapsedMs = Date.now() - firstSeenAtMsRef.current;
    return formatElapsedMs(Math.max(0, serverElapsedMs, localElapsedMs));
  })();

  return (
    <div className={styles.run} data-selected={selected ? '' : undefined}>
      <button
        type="button"
        className={styles.runButton}
        onClick={onSelect}
        disabled={!selectable}
        aria-pressed={selected}
      >
        <span className={styles.runTop}>
          <StatusTag status={run.status} />
          <span className={styles.runTarget}>{targetLabel(run)}</span>
        </span>
        {run.totalFiles != null && (
          <span className={styles.runProgress}>
            <Progress value={Math.min(100, percent)} className={styles.progress} />
            <span className={styles.mono}>
              {run.processedFiles.toLocaleString()} / {run.totalFiles.toLocaleString()}
            </span>
          </span>
        )}
        <span className={styles.runMeta}>
          <span>{flagsLabel(run.flags)}</span>
          <span className={styles.mono}>
            {formatDate(run.requestedAt)} · {runtimeLabel}
          </span>
        </span>
      </button>
      {(canCancel || isCancelling) && (
        <Tooltip content={isCancelling ? 'Cancelling' : 'Cancel run'}>
          <IconButton
            variant="ghost"
            size="sm"
            className={styles.cancel}
            onClick={onCancel}
            disabled={isCancelling}
            aria-label={isCancelling ? 'Cancelling' : 'Cancel run'}
          >
            {isCancelling ? <Spinner label="Cancelling" /> : <Square size={14} strokeWidth={1.5} />}
          </IconButton>
        </Tooltip>
      )}
    </div>
  );
}

export default function IndexingPage() {
  const { data: status } = useIndexerStatus({ adaptivePolling: true });
  const enqueueIndex = useEnqueueIndex();
  const cancelRunMutation = useCancelIndexerRun(7);
  const [cancellingRunId, setCancellingRunId] = useState<string | null>(null);
  const {
    data: runs,
    isPending,
    isError,
  } = useIndexerRuns(7, {
    adaptivePolling: true,
  });

  const runsWithLive = useMemo(() => {
    if (!runs) return [];

    const updatedRuns = [...runs];
    const current = status?.currentRequest;
    const queue = status?.queue ?? [];

    if (current) {
      const currentRunId = current.runId;
      if (currentRunId && updatedRuns.some((run) => run.id === currentRunId)) {
        for (let i = 0; i < updatedRuns.length; i += 1) {
          if (updatedRuns[i].id !== currentRunId) continue;
          updatedRuns[i] = {
            ...updatedRuns[i],
            status: current.isCancellationRequested ? 'cancelling' : 'running',
            startedAt: updatedRuns[i].startedAt ?? current.requestedAt,
            totalFiles: current.totalFiles ?? updatedRuns[i].totalFiles,
            processedFiles: Math.max(updatedRuns[i].processedFiles, current.processedFiles),
            isCancellationRequested: current.isCancellationRequested,
          };
          break;
        }
      } else {
        updatedRuns.unshift({
          id: currentRunId ?? current.id,
          directoryFilter: current.directoryFilter,
          relativeModelPath: current.relativeModelPath,
          flags: current.flags,
          requestedAt: current.requestedAt,
          startedAt: current.requestedAt,
          completedAt: null,
          totalFiles: current.totalFiles,
          processedFiles: current.processedFiles,
          status: current.isCancellationRequested ? 'cancelling' : 'running',
          outcome: null,
          error: null,
          isCancellationRequested: current.isCancellationRequested,
        });
      }
    }

    for (const queued of queue) {
      const queuedRunId = queued.runId;

      if (queuedRunId && updatedRuns.some((run) => run.id === queuedRunId)) {
        for (let i = 0; i < updatedRuns.length; i += 1) {
          if (updatedRuns[i].id !== queuedRunId) continue;
          updatedRuns[i] = {
            ...updatedRuns[i],
            status: queued.isCancellationRequested ? 'cancelling' : 'queued',
            totalFiles: queued.totalFiles ?? updatedRuns[i].totalFiles,
            processedFiles: Math.max(updatedRuns[i].processedFiles, queued.processedFiles),
            isCancellationRequested: queued.isCancellationRequested,
          };
          break;
        }
      } else {
        updatedRuns.push({
          id: queuedRunId ?? `queued:${queued.id}`,
          directoryFilter: queued.directoryFilter,
          relativeModelPath: queued.relativeModelPath,
          flags: queued.flags,
          requestedAt: queued.requestedAt,
          startedAt: null,
          completedAt: null,
          totalFiles: queued.totalFiles,
          processedFiles: queued.processedFiles,
          status: queued.isCancellationRequested ? 'cancelling' : 'queued',
          outcome: null,
          error: null,
          isCancellationRequested: queued.isCancellationRequested,
        });
      }
    }

    const runningRuns = updatedRuns.filter((run) => run.status === 'running');
    const queuedRuns = queue
      .map((queued) => {
        const queuedId = queued.runId ?? `queued:${queued.id}`;
        return updatedRuns.find((run) => run.id === queuedId) ?? null;
      })
      .filter((run): run is IndexRunSummary => run !== null);
    const queuedRunIds = new Set(queuedRuns.map((run) => run.id));
    const otherRuns = updatedRuns.filter(
      (run) => run.status !== 'running' && !queuedRunIds.has(run.id),
    );
    return [...runningRuns, ...queuedRuns, ...otherRuns];
  }, [runs, status?.currentRequest, status?.queue]);

  const defaultSelectedRunId = useMemo(() => {
    return runsWithLive.find((run) => run.status !== 'queued')?.id ?? null;
  }, [runsWithLive]);

  const groupedRuns = useMemo(() => {
    const running = runsWithLive.filter(
      (run) => run.status === 'running' || run.status === 'cancelling',
    );
    const rest = runsWithLive.filter(
      (run) => run.status !== 'running' && run.status !== 'cancelling',
    );
    return { running, rest };
  }, [runsWithLive]);

  useEffect(() => {
    if (!cancellingRunId) return;

    const stillCancellable = runsWithLive.some(
      (run) => run.id === cancellingRunId && canCancelRun(run),
    );

    if (!stillCancellable) {
      setCancellingRunId(null);
    }
  }, [cancellingRunId, runsWithLive]);

  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [filesView, setFilesView] = useState<IndexRunFilesView>('all');
  const selectedRun = runsWithLive.find((run) => run.id === selectedRunId);
  const effectiveRunId =
    selectedRun && selectedRun.status !== 'queued' ? selectedRunId : defaultSelectedRunId;
  const hasRunnableSelection = !!effectiveRunId;
  const [filesPage, setFilesPage] = useState(1);
  const [eventsPage, setEventsPage] = useState(1);

  function canCancelRun(run: Pick<IndexRunSummary, 'id' | 'status'>): boolean {
    if (run.id.startsWith('queued:')) return false;
    return run.status === 'queued' || run.status === 'running';
  }

  function cancelRun(runId: string): void {
    if (cancellingRunId === runId) return;

    setCancellingRunId(runId);
    cancelRunMutation.mutate(runId, {
      onError: () => {
        setCancellingRunId((current) => (current === runId ? null : current));
      },
    });
  }

  const {
    data: runDetail,
    isPending: isRunPending,
    isError: isRunError,
  } = useIndexerRun(
    effectiveRunId,
    {
      filesPage,
      filesPageSize: FILES_PAGE_SIZE,
      filesView,
      eventsPage,
      eventsPageSize: EVENTS_PAGE_SIZE,
    },
    { adaptivePolling: true },
  );

  if (isPending) return <LoadingView />;
  if (isError || !runs) return <ErrorView message="Failed to load indexing history." />;

  const selectRun = (run: IndexRunSummary) => {
    setSelectedRunId(run.id);
    setFilesPage(1);
    setEventsPage(1);
  };

  const renderRun = (run: IndexRunSummary) => (
    <RunSummaryCard
      key={run.id}
      run={run}
      selected={run.id === effectiveRunId}
      selectable={run.status !== 'queued'}
      onSelect={() => selectRun(run)}
      canCancel={canCancelRun(run)}
      onCancel={() => cancelRun(run.id)}
      isCancelling={run.status === 'cancelling' || cancellingRunId === run.id}
    />
  );

  return (
    <PageLayout>
      <PageHeader
        title="Indexing"
        actions={
          <Button
            variant="secondary"
            disabled={enqueueIndex.isPending}
            onClick={() => enqueueIndex.mutate({ directoryFilter: null, flags: IndexFlags.All })}
          >
            Index all
          </Button>
        }
      />

      <div className={styles.layout}>
        <section className={`${styles.panel} ${styles.runs}`} aria-labelledby="runs-heading">
          <div className={styles.panelHeader}>
            <h2 id="runs-heading" className="mm-type-label">
              Runs
            </h2>
            <span className={styles.mono}>Last 7 days</span>
          </div>
          {runsWithLive.length === 0 && (
            <p className={styles.emptyRuns}>No runs in the last 7 days.</p>
          )}
          {groupedRuns.running.map(renderRun)}
          {groupedRuns.rest.map(renderRun)}
        </section>

        <div className={styles.detail}>
          {hasRunnableSelection && isRunPending && <LoadingView />}
          {hasRunnableSelection && !isRunPending && isRunError && (
            <ErrorView message="Couldn't load this run." />
          )}
          {hasRunnableSelection && !isRunPending && !isRunError && runDetail && (
            <RunDetailWithPaging
              detail={runDetail}
              onGoToFiles={(page) => setFilesPage(page)}
              onGoToEvents={(page) => setEventsPage(page)}
              currentFilesPage={filesPage}
              currentEventsPage={eventsPage}
              filesView={filesView}
              onFilesViewChange={(view) => {
                setFilesView(view);
                setFilesPage(1);
              }}
            />
          )}
        </div>
      </div>
    </PageLayout>
  );
}

function RunDetailWithPaging({
  detail,
  onGoToFiles,
  onGoToEvents,
  currentFilesPage,
  currentEventsPage,
  filesView,
  onFilesViewChange,
}: {
  detail: IndexRunDetail;
  onGoToFiles: (page: number) => void;
  onGoToEvents: (page: number) => void;
  currentFilesPage: number;
  currentEventsPage: number;
  filesView: IndexRunFilesView;
  onFilesViewChange: (view: IndexRunFilesView) => void;
}) {
  const [, setTick] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setTick((n) => n + 1), 1000);
    return () => clearInterval(id);
  }, []);

  const run = detail.run;
  const total = run.totalFiles ?? 0;
  const percent = total > 0 ? Math.round((run.processedFiles / total) * 100) : 0;
  const filesPaged = normalizePaged<IndexRunFile>((detail as { files: unknown }).files);
  const eventsPaged = normalizePaged<IndexRunEvent>((detail as { events: unknown }).events);
  const filesIsLegacyArray = Array.isArray((detail as { files: unknown }).files);
  const eventsIsLegacyArray = Array.isArray((detail as { events: unknown }).events);

  const effectiveFilesPageSize = filesIsLegacyArray
    ? FILES_PAGE_SIZE
    : Math.max(1, filesPaged.pageSize);
  const effectiveFilesPage = filesIsLegacyArray ? currentFilesPage : filesPaged.page;
  const effectiveFilesTotalCount = filesPaged.totalCount;

  const effectiveEventsPageSize = eventsIsLegacyArray
    ? EVENTS_PAGE_SIZE
    : Math.max(1, eventsPaged.pageSize);
  const effectiveEventsPage = eventsIsLegacyArray ? currentEventsPage : eventsPaged.page;
  const effectiveEventsTotalCount = eventsPaged.totalCount;

  const pagedFilesItems = filesIsLegacyArray
    ? filesPaged.items.slice(
        (effectiveFilesPage - 1) * effectiveFilesPageSize,
        effectiveFilesPage * effectiveFilesPageSize,
      )
    : filesPaged.items;

  const pagedEventsItems = eventsIsLegacyArray
    ? eventsPaged.items.slice(
        (effectiveEventsPage - 1) * effectiveEventsPageSize,
        effectiveEventsPage * effectiveEventsPageSize,
      )
    : eventsPaged.items;

  const filesTotalPages = Math.max(
    1,
    Math.ceil(effectiveFilesTotalCount / Math.max(1, effectiveFilesPageSize)),
  );
  const eventsTotalPages = Math.max(
    1,
    Math.ceil(effectiveEventsTotalCount / Math.max(1, effectiveEventsPageSize)),
  );

  const filesFrom =
    effectiveFilesTotalCount === 0 ? 0 : (effectiveFilesPage - 1) * effectiveFilesPageSize + 1;
  const filesTo = Math.min(effectiveFilesTotalCount, effectiveFilesPage * effectiveFilesPageSize);

  return (
    <section className={styles.panel} aria-labelledby="run-heading">
      <div className={styles.panelHeader}>
        <h2 id="run-heading" className={styles.runTitle}>
          {targetLabel(run)}
        </h2>
        <StatusTag status={run.status} />
      </div>
      <div className={styles.summary}>
        {run.totalFiles != null && (
          <div className={styles.runProgress}>
            <Progress value={Math.min(100, percent)} className={styles.progress} />
            <span className={styles.mono}>
              {run.processedFiles.toLocaleString()} / {run.totalFiles.toLocaleString()} ({percent}%)
            </span>
          </div>
        )}
        <span className={styles.mono}>
          {flagsLabel(run.flags)} · Started {formatDate(run.startedAt)} ·{' '}
          {formatDuration(run.startedAt, run.completedAt)}
        </span>
        {detail.stages && <IndexRunStages stages={detail.stages} />}
        {run.error && (
          <p role="alert" className={styles.error}>
            {run.error}
          </p>
        )}
      </div>

      <Tabs defaultValue="files" className={styles.tabs}>
        <TabsList aria-label="Run detail">
          <TabsTrigger value="files">Files</TabsTrigger>
          <TabsTrigger value="log">Log</TabsTrigger>
        </TabsList>

        <TabsContent value="files">
          <div className={styles.toolbar}>
            <SegmentedControl
              aria-label="Show files"
              value={filesView}
              onValueChange={(view) => onFilesViewChange(view as IndexRunFilesView)}
            >
              <SegmentedControlItem value="all">All</SegmentedControlItem>
              <SegmentedControlItem value="processed">Processed</SegmentedControlItem>
              <SegmentedControlItem value="pending">Pending</SegmentedControlItem>
            </SegmentedControl>
          </div>
          <ul className={styles.rows}>
            {pagedFilesItems.map((file) => (
              <li key={file.relativePath} className={styles.fileRow}>
                <div className={styles.fileMain}>
                  <span className={styles.path}>{file.relativePath}</span>
                  <span className={styles.mono}>
                    {formatDurationMs(resolveFileDurationMs(file, run))}
                  </span>
                  <Tag
                    variant={FILE_STATUS_TAG[file.status] ?? 'neutral'}
                    className={file.status === 'failed' ? styles.failed : undefined}
                  >
                    {file.status.charAt(0).toUpperCase() + file.status.slice(1)}
                  </Tag>
                </div>
                {(file.generatedPreview || file.generatedHull || file.isNew || file.wasUpdated) && (
                  <div className={styles.artifacts}>
                    {file.isNew && <Tag variant="outline">New</Tag>}
                    {file.wasUpdated && <Tag variant="outline">Updated</Tag>}
                    {file.generatedPreview && <Tag variant="outline">Preview</Tag>}
                    {file.generatedHull && <Tag variant="outline">Hull</Tag>}
                  </div>
                )}
                {file.message && <p className={styles.message}>{file.message}</p>}
              </li>
            ))}
            {pagedFilesItems.length === 0 && <li className={styles.emptyRuns}>No files.</li>}
          </ul>
          <div className={styles.pager}>
            <Pagination
              page={effectiveFilesPage}
              totalPages={filesTotalPages}
              onPageChange={onGoToFiles}
              summary={`${filesFrom.toLocaleString()}–${filesTo.toLocaleString()} of ${effectiveFilesTotalCount.toLocaleString()}`}
            />
          </div>
        </TabsContent>

        <TabsContent value="log">
          <ul className={styles.rows}>
            {pagedEventsItems.map((event, index) => (
              <li key={`${event.createdAt}-${index}`} className={styles.logRow}>
                <span className={styles.mono}>
                  {new Date(event.createdAt).toLocaleTimeString()}
                </span>
                <span
                  className={`${styles.mono} ${event.level.toLowerCase() === 'error' ? styles.errorText : event.level.toLowerCase().startsWith('warn') ? styles.warnText : ''}`}
                >
                  {event.level.toUpperCase()}
                </span>
                <span className={styles.logMessage}>
                  {event.message}
                  {event.relativePath && <span className={styles.mono}> {event.relativePath}</span>}
                </span>
              </li>
            ))}
            {pagedEventsItems.length === 0 && <li className={styles.emptyRuns}>No log entries.</li>}
          </ul>
          <div className={styles.pager}>
            <Pagination
              page={effectiveEventsPage}
              totalPages={eventsTotalPages}
              onPageChange={onGoToEvents}
              summary={`${effectiveEventsTotalCount.toLocaleString()} events`}
            />
          </div>
        </TabsContent>
      </Tabs>
    </section>
  );
}
