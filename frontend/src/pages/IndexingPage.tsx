import Paper from '@mui/material/Paper';
import Stack from '@mui/material/Stack';
import Typography from '@mui/material/Typography';
import Chip from '@mui/material/Chip';
import Divider from '@mui/material/Divider';
import LinearProgress from '@mui/material/LinearProgress';
import Button from '@mui/material/Button';
import TextField from '@mui/material/TextField';
import { useEffect, useMemo, useState } from 'react';
import {
  IndexFlags,
  type IndexRunDetail,
  type IndexRunEvent,
  type IndexRunFile,
  type IndexRunSummary,
} from '../lib/api';
import { useIndexerRun, useIndexerRuns, useIndexerStatus } from '../lib/queries';
import ErrorView from '../components/ErrorView';
import LoadingView from '../components/LoadingView';
import PageLayout from '../components/layouts/PageLayout';
import styles from './IndexingPage.module.css';

function parseApiDate(value: string): Date {
  const trimmed = value.trim();
  const hasExplicitTimezone = /(?:Z|[+-]\d{2}:\d{2})$/i.test(trimmed);
  const looksIsoWithoutZone = /^\d{4}-\d{2}-\d{2}T/.test(trimmed) && !hasExplicitTimezone;
  return new Date(looksIsoWithoutZone ? `${trimmed}Z` : trimmed);
}

function parseApiTimeMs(value: string | null): number | null {
  if (!value) return null;
  const ms = parseApiDate(value).getTime();
  return Number.isFinite(ms) ? ms : null;
}

function formatDate(value: string | null): string {
  if (!value) return '-';
  return parseApiDate(value).toLocaleString();
}

function formatDuration(startedAt: string | null, completedAt: string | null): string {
  const start = parseApiTimeMs(startedAt);
  if (start == null) return '-';
  const completed = parseApiTimeMs(completedAt);
  const end = completed ?? Date.now();
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
    const processedAtMs = parseApiTimeMs(record.processedAt);
    const startedAtMs = parseApiTimeMs(run.startedAt);
    if (processedAtMs != null && startedAtMs != null && processedAtMs >= startedAtMs)
      return processedAtMs - startedAtMs;
  }

  return null;
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

function statusChip(status: string) {
  if (status === 'running') return <Chip size="small" variant="status-running" label="Running" />;
  if (status === 'failed')
    return <Chip size="small" color="error" variant="outlined" label="Failed" />;
  return <Chip size="small" color="success" variant="outlined" label="Success" />;
}

function artifactChip(enabled: boolean, label: string) {
  return <Chip size="small" label={label} variant={enabled ? 'badge-enabled' : 'badge-disabled'} />;
}

const FILES_PAGE_SIZE = 200;
const EVENTS_PAGE_SIZE = 200;

function RunSummaryCard({
  run,
  selected,
  onSelect,
}: {
  run: IndexRunSummary;
  selected: boolean;
  onSelect: () => void;
}) {
  const [, setTick] = useState(0);

  useEffect(() => {
    if (run.status !== 'running' || !run.startedAt) return;
    const id = setInterval(() => setTick((n) => n + 1), 1000);
    return () => clearInterval(id);
  }, [run.status, run.startedAt]);

  const total = run.totalFiles ?? 0;
  const percent = total > 0 ? Math.round((run.processedFiles / total) * 100) : 0;
  const processingTimeLabel =
    run.status === 'running' && run.startedAt
      ? formatElapsedMs(Date.now() - (parseApiTimeMs(run.startedAt) ?? Date.now()))
      : null;
  const durationLabel =
    run.status !== 'running' && run.startedAt && run.completedAt
      ? formatDuration(run.startedAt, run.completedAt)
      : null;

  return (
    <button
      type="button"
      onClick={onSelect}
      className={`${styles.runCard} ${selected ? styles.runCardActive : ''}`}
    >
      <Stack spacing={0.5}>
        <Stack direction="row" alignItems="center" justifyContent="space-between">
          <Typography variant="subtitle2" className={styles.cardTitle}>
            {targetLabel(run)}
          </Typography>
          {statusChip(run.status)}
        </Stack>
        <Typography variant="caption" color="text.secondary">
          {flagsLabel(run.flags)}
        </Typography>
        <Typography variant="caption" color="text.secondary">
          Requested {formatDate(run.requestedAt)}
        </Typography>
        {processingTimeLabel && (
          <Typography variant="caption" color="text.secondary">
            Processing time {processingTimeLabel}
          </Typography>
        )}
        {durationLabel && (
          <Typography variant="caption" color="text.secondary">
            Duration {durationLabel}
          </Typography>
        )}
        {run.totalFiles != null && (
          <>
            <LinearProgress
              variant="determinate"
              value={Math.min(100, percent)}
              className={styles.progressBar}
            />
            <Typography variant="caption" color="text.secondary">
              {run.processedFiles}/{run.totalFiles} files
            </Typography>
          </>
        )}
      </Stack>
    </button>
  );
}

export default function IndexingPage() {
  const { data: status } = useIndexerStatus({ adaptivePolling: true });
  const {
    data: runs,
    isPending,
    isError,
  } = useIndexerRuns(7, {
    adaptivePolling: true,
  });

  const defaultSelectedRunId = useMemo(() => {
    const liveRunId = status?.currentRequest?.runId;
    if (liveRunId && runs?.some((r) => r.id === liveRunId)) return liveRunId;
    return runs?.[0]?.id ?? null;
  }, [status?.currentRequest?.runId, runs]);

  const runsWithLive = useMemo(() => {
    if (!runs) return [];
    const current = status?.currentRequest;
    if (!current) return runs;

    const currentRunId = current.runId;
    if (currentRunId && runs.some((r) => r.id === currentRunId)) {
      return runs.map((run) =>
        run.id === currentRunId
          ? {
              ...run,
              status: 'running' as const,
              startedAt: run.startedAt ?? current.requestedAt,
              totalFiles: current.totalFiles ?? run.totalFiles,
              processedFiles: Math.max(run.processedFiles, current.processedFiles),
            }
          : run,
      );
    }

    return [
      {
        id: currentRunId ?? current.id,
        directoryFilter: current.directoryFilter,
        relativeModelPath: current.relativeModelPath,
        flags: current.flags,
        requestedAt: current.requestedAt,
        startedAt: current.requestedAt,
        completedAt: null,
        totalFiles: current.totalFiles,
        processedFiles: current.processedFiles,
        status: 'running' as const,
        outcome: null,
        error: null,
      },
      ...runs,
    ];
  }, [runs, status?.currentRequest]);

  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const effectiveRunId = selectedRunId ?? defaultSelectedRunId;
  const [filesPage, setFilesPage] = useState(1);
  const [eventsPage, setEventsPage] = useState(1);

  const {
    data: runDetail,
    isPending: isRunPending,
    isError: isRunError,
  } = useIndexerRun(
    effectiveRunId,
    {
      filesPage,
      filesPageSize: FILES_PAGE_SIZE,
      eventsPage,
      eventsPageSize: EVENTS_PAGE_SIZE,
    },
    { adaptivePolling: true },
  );

  if (isPending) return <LoadingView />;
  if (isError || !runs) return <ErrorView message="Failed to load indexing history." />;

  return (
    <PageLayout>
      <Stack
        direction="row"
        alignItems="center"
        justifyContent="space-between"
        className={styles.pageHeader}
      >
        <Stack spacing={0.5}>
          <Typography variant="page-title">Indexing</Typography>
          <Typography variant="body2" color="text.secondary">
            Live indexing progress and retained history (7 days)
          </Typography>
        </Stack>
      </Stack>

      <Stack direction={{ xs: 'column', lg: 'row' }} spacing={2} className={styles.contentWrap}>
        <Paper variant="outlined" className={styles.sidebarPanel}>
          <Stack spacing={1} className={styles.panelBody}>
            <Typography variant="h6">Runs</Typography>
            <Divider />
            <Stack spacing={1} className={styles.runList}>
              {runsWithLive.length === 0 && (
                <Typography variant="body2" color="text.secondary">
                  No indexing runs in the last 7 days.
                </Typography>
              )}
              {runsWithLive.map((run) => (
                <RunSummaryCard
                  key={run.id}
                  run={run}
                  selected={run.id === effectiveRunId}
                  onSelect={() => {
                    setSelectedRunId(run.id);
                    setFilesPage(1);
                    setEventsPage(1);
                  }}
                />
              ))}
            </Stack>
          </Stack>
        </Paper>

        <Stack className={styles.mainPanel}>
          {isRunPending && <LoadingView />}
          {!isRunPending && isRunError && (
            <ErrorView message="Failed to load the selected index run detail." />
          )}
          {!isRunPending && !isRunError && runDetail && (
            <RunDetailWithPaging
              detail={runDetail}
              onPrevFiles={() => setFilesPage((p) => Math.max(1, p - 1))}
              onNextFiles={() => setFilesPage((p) => p + 1)}
              onPrevEvents={() => setEventsPage((p) => Math.max(1, p - 1))}
              onNextEvents={() => setEventsPage((p) => p + 1)}
              onGoToFiles={(page) => setFilesPage(page)}
              onGoToEvents={(page) => setEventsPage(page)}
              currentFilesPage={filesPage}
              currentEventsPage={eventsPage}
            />
          )}
        </Stack>
      </Stack>
    </PageLayout>
  );
}

function RunDetailWithPaging({
  detail,
  onPrevFiles,
  onNextFiles,
  onPrevEvents,
  onNextEvents,
  onGoToFiles,
  onGoToEvents,
  currentFilesPage,
  currentEventsPage,
}: {
  detail: IndexRunDetail;
  onPrevFiles: () => void;
  onNextFiles: () => void;
  onPrevEvents: () => void;
  onNextEvents: () => void;
  onGoToFiles: (page: number) => void;
  onGoToEvents: (page: number) => void;
  currentFilesPage: number;
  currentEventsPage: number;
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

  const [filesPageInput, setFilesPageInput] = useState(String(filesPaged.page));
  const [eventsPageInput, setEventsPageInput] = useState(String(eventsPaged.page));

  useEffect(() => {
    setFilesPageInput(String(effectiveFilesPage));
  }, [effectiveFilesPage]);

  useEffect(() => {
    setEventsPageInput(String(effectiveEventsPage));
  }, [effectiveEventsPage]);

  function parseJumpPage(value: string, maxPage: number): number | null {
    const parsed = Number.parseInt(value, 10);
    if (!Number.isFinite(parsed)) return null;
    return Math.min(maxPage, Math.max(1, parsed));
  }

  return (
    <Stack spacing={2}>
      <Paper variant="outlined" className={styles.panel}>
        <Stack spacing={1.25} className={styles.panelBody}>
          <Stack direction="row" alignItems="center" justifyContent="space-between">
            <Typography variant="h6">Current Progress</Typography>
            {statusChip(run.status)}
          </Stack>
          <Typography variant="body2" color="text.secondary">
            {targetLabel(run)}
          </Typography>
          <Typography variant="caption" color="text.secondary">
            {flagsLabel(run.flags)}
          </Typography>
          {run.totalFiles != null && (
            <>
              <LinearProgress
                variant="determinate"
                value={Math.min(100, percent)}
                className={styles.progressBarLarge}
              />
              <Typography variant="body2">
                {run.processedFiles}/{run.totalFiles} files ({percent}%)
              </Typography>
            </>
          )}
          <Typography variant="caption" color="text.secondary">
            Started: {formatDate(run.startedAt)} | Duration:{' '}
            {formatDuration(run.startedAt, run.completedAt)}
          </Typography>
          {run.error && (
            <Typography variant="body2" className={styles.errorText}>
              {run.error}
            </Typography>
          )}
        </Stack>
      </Paper>

      <Paper variant="outlined" className={styles.panel}>
        <Stack spacing={1} className={styles.panelBody}>
          <Stack direction="row" justifyContent="space-between" alignItems="center">
            <Typography variant="h6">Indexed Files</Typography>
            <Stack direction="row" spacing={1} alignItems="center" className={styles.pagerRow}>
              <Typography variant="caption" color="text.secondary">
                Page {effectiveFilesPage}/{filesTotalPages} ({effectiveFilesTotalCount} files)
              </Typography>
              <TextField
                size="small"
                value={filesPageInput}
                onChange={(event) => setFilesPageInput(event.target.value)}
                className={styles.pageJumpInput}
                inputProps={{ inputMode: 'numeric', pattern: '[0-9]*' }}
              />
              <Button
                size="small"
                variant="outlined"
                onClick={() => {
                  const nextPage = parseJumpPage(filesPageInput, filesTotalPages);
                  if (nextPage != null) onGoToFiles(nextPage);
                }}
              >
                Go
              </Button>
              <Button
                size="small"
                variant="outlined"
                onClick={onPrevFiles}
                disabled={effectiveFilesPage <= 1}
              >
                Prev
              </Button>
              <Button
                size="small"
                variant="outlined"
                onClick={onNextFiles}
                disabled={effectiveFilesPage >= filesTotalPages}
              >
                Next
              </Button>
            </Stack>
          </Stack>
          <Typography variant="caption" color="text.secondary">
            Shows a paged subset of files for this run, with generated artifact outcomes.
          </Typography>
          <Divider />
          <Stack spacing={0.75} className={styles.filesList}>
            {pagedFilesItems.map((file) => (
              <Stack key={file.relativePath} spacing={0.5} className={styles.fileRow}>
                <Stack direction="row" alignItems="center" justifyContent="space-between">
                  <Typography variant="body2" className={styles.filePath}>
                    {file.relativePath}
                  </Typography>
                  <Stack direction="row" spacing={0.75} alignItems="center">
                    <Typography variant="caption" color="text.secondary">
                      {formatDurationMs(resolveFileDurationMs(file, run))}
                    </Typography>
                    <Chip size="small" label={file.status} className={styles.statusChip} />
                  </Stack>
                </Stack>
                <Stack direction="row" spacing={0.5} className={styles.artifactRow}>
                  {artifactChip(file.generatedPreview, 'preview')}
                  {artifactChip(file.generatedHull, 'hull')}
                  {artifactChip(file.generatedAiTags, 'ai tags')}
                  {artifactChip(file.generatedAiDescription, 'ai description')}
                  {artifactChip(file.isNew, 'new')}
                  {artifactChip(file.wasUpdated, 'updated')}
                  {file.aiGenerationReason && (
                    <Chip
                      size="small"
                      label={`ai: ${file.aiGenerationReason}`}
                      className={styles.aiReasonChip}
                    />
                  )}
                </Stack>
                {file.message && (
                  <Typography variant="caption" color="text.secondary">
                    {file.message}
                  </Typography>
                )}
              </Stack>
            ))}
          </Stack>
        </Stack>
      </Paper>

      <Paper variant="outlined" className={styles.panel}>
        <Stack spacing={1} className={styles.panelBody}>
          <Stack direction="row" justifyContent="space-between" alignItems="center">
            <Typography variant="h6">Run Log</Typography>
            <Stack direction="row" spacing={1} alignItems="center" className={styles.pagerRow}>
              <Typography variant="caption" color="text.secondary">
                Page {effectiveEventsPage}/{eventsTotalPages} ({effectiveEventsTotalCount} events)
              </Typography>
              <TextField
                size="small"
                value={eventsPageInput}
                onChange={(event) => setEventsPageInput(event.target.value)}
                className={styles.pageJumpInput}
                inputProps={{ inputMode: 'numeric', pattern: '[0-9]*' }}
              />
              <Button
                size="small"
                variant="outlined"
                onClick={() => {
                  const nextPage = parseJumpPage(eventsPageInput, eventsTotalPages);
                  if (nextPage != null) onGoToEvents(nextPage);
                }}
              >
                Go
              </Button>
              <Button
                size="small"
                variant="outlined"
                onClick={onPrevEvents}
                disabled={effectiveEventsPage <= 1}
              >
                Prev
              </Button>
              <Button
                size="small"
                variant="outlined"
                onClick={onNextEvents}
                disabled={effectiveEventsPage >= eventsTotalPages}
              >
                Next
              </Button>
            </Stack>
          </Stack>
          <Divider />
          <Stack spacing={0.75} className={styles.logList}>
            {pagedEventsItems.map((event, index) => (
              <Stack key={`${event.createdAt}-${index}`} spacing={0.2} className={styles.logRow}>
                <Typography variant="caption" className={styles.logMeta}>
                  [{parseApiDate(event.createdAt).toLocaleTimeString()}] {event.level.toUpperCase()}
                  {event.relativePath ? ` • ${event.relativePath}` : ''}
                </Typography>
                <Typography variant="body2">{event.message}</Typography>
              </Stack>
            ))}
          </Stack>
        </Stack>
      </Paper>
    </Stack>
  );
}
