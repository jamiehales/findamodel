import { useEffect, useMemo, useState } from 'react';
import { Link, useLocation } from 'react-router-dom';
import {
  Button,
  FormControlLabel,
  MenuItem,
  List,
  ListItemButton,
  ListItemText,
  Switch,
  Chip,
  Divider,
  Stack,
  TextField,
  ToggleButton,
  ToggleButtonGroup,
  Typography,
} from '@mui/material';
import {
  useAppConfig,
  useCreatePrinter,
  useDeletePrinter,
  useCreateMetadataDictionaryValue,
  useDeleteMetadataDictionaryValue,
  useInstanceStats,
  useMetadataDictionaryOverview,
  usePrinters,
  useSetDefaultPrinter,
  useUpdatePrinter,
  useUpdateAppConfig,
  useUpdateMetadataDictionaryValue,
} from '../lib/queries';
import type { InstanceStats, MetadataDictionaryField } from '../lib/api';
import ErrorView from '../components/ErrorView';
import LoadingView from '../components/LoadingView';
import PageLayout from '../components/layouts/PageLayout';
import { useApplicationLogs } from '../lib/queries';
import styles from './SettingsPage.module.css';

type FieldKey = 'category' | 'type' | 'material' | 'tags';

const FIELD_LABELS: Record<FieldKey, string> = {
  category: 'Category',
  type: 'Type',
  material: 'Material',
  tags: 'Tags',
};

function toModelsHref(field: FieldKey, value: string): string {
  const params = new URLSearchParams();
  params.append(field, value);
  return `/?${params.toString()}`;
}

function FieldSection({ field, data }: { field: FieldKey; data: MetadataDictionaryField }) {
  const [newValue, setNewValue] = useState('');
  const [editValues, setEditValues] = useState<Record<string, string>>({});
  const createMutation = useCreateMetadataDictionaryValue();
  const updateMutation = useUpdateMetadataDictionaryValue();
  const deleteMutation = useDeleteMetadataDictionaryValue();

  const configuredSet = useMemo(
    () => new Set(data.configured.map((v) => v.value.toLowerCase())),
    [data.configured],
  );

  const observedOnly = useMemo(
    () => data.observed.filter((v) => !configuredSet.has(v.toLowerCase())),
    [data.observed, configuredSet],
  );

  return (
    <Stack className={styles.section}>
      <Typography variant="h6">{FIELD_LABELS[field]}</Typography>

      <Stack spacing={1} className={styles.group}>
        <Stack direction="row" spacing={1} className={styles.addRow}>
          <TextField
            size="small"
            label={`New ${FIELD_LABELS[field]} value`}
            value={newValue}
            onChange={(e) => setNewValue(e.target.value)}
          />
          <Button
            variant="contained"
            disabled={createMutation.isPending || !newValue.trim()}
            onClick={async () => {
              await createMutation.mutateAsync({ field, value: newValue.trim() });
              setNewValue('');
            }}
          >
            Add
          </Button>
        </Stack>

        <Stack spacing={1}>
          {data.configured.map((item) => {
            const current = editValues[item.id] ?? item.value;
            return (
              <Stack key={item.id} direction="row" spacing={1} className={styles.configRow}>
                <TextField
                  size="small"
                  value={current}
                  onChange={(e) =>
                    setEditValues((prev) => ({
                      ...prev,
                      [item.id]: e.target.value,
                    }))
                  }
                />
                <Button
                  variant="outlined"
                  disabled={updateMutation.isPending || !current.trim() || current === item.value}
                  onClick={() => updateMutation.mutate({ id: item.id, value: current.trim() })}
                >
                  Save
                </Button>
                <Button
                  color="error"
                  variant="outlined"
                  disabled={deleteMutation.isPending}
                  onClick={() => deleteMutation.mutate(item.id)}
                >
                  Delete
                </Button>
              </Stack>
            );
          })}
          {data.configured.length === 0 && (
            <Typography color="text.secondary">No configured values.</Typography>
          )}
        </Stack>
      </Stack>

      <Stack spacing={1} className={styles.group}>
        <Divider />
        <Stack direction="row" spacing={1} className={styles.chipsWrap}>
          {data.observed.map((value) => (
            <Chip
              key={value}
              label={value}
              component={Link}
              clickable
              to={toModelsHref(field, value)}
              color={observedOnly.includes(value) ? 'warning' : 'default'}
              variant={observedOnly.includes(value) ? 'filled' : 'outlined'}
            />
          ))}
          {data.observed.length === 0 && (
            <Typography color="text.secondary">No observed values yet.</Typography>
          )}
        </Stack>
      </Stack>
    </Stack>
  );
}

const SETTINGS_SECTIONS = [
  { key: 'settings', label: 'Settings', path: '/settings' },
  { key: 'printers', label: 'Printers', path: '/settings/printers' },
  { key: 'logs', label: 'Logs', path: '/settings/logs' },
  { key: 'schema', label: 'Schema', path: '/settings/schema' },
  { key: 'stats', label: 'Stats', path: '/settings/stats' },
] as const;

type SettingsSectionKey = (typeof SETTINGS_SECTIONS)[number]['key'];

const LOG_LIMIT = 500;

function formatEnabled(value: boolean): string {
  return value ? 'Enabled' : 'Disabled';
}

function formatAvailability(value: boolean): string {
  return value ? 'Available' : 'Unavailable';
}

function StatsGroup({
  title,
  rows,
}: {
  title: string;
  rows: Array<{ label: string; value: string | number }>;
}) {
  return (
    <Stack spacing={1} className={styles.statsCard}>
      <Typography variant="h6">{title}</Typography>
      <Stack spacing={1}>
        {rows.map((row) => (
          <Stack
            key={row.label}
            direction="row"
            spacing={2}
            justifyContent="space-between"
            className={styles.statRow}
          >
            <Typography color="text.secondary">{row.label}</Typography>
            <Typography className={styles.statValue}>{row.value}</Typography>
          </Stack>
        ))}
      </Stack>
    </Stack>
  );
}

function InstanceStatsSection({
  stats,
  isPending,
  isError,
}: {
  stats: InstanceStats | undefined;
  isPending: boolean;
  isError: boolean;
}) {
  return (
    <Stack spacing={2} className={styles.globalSettingsSection}>
      {isPending && <Typography color="text.secondary">Loading instance stats...</Typography>}
      {isError && <Typography color="error">Failed to load instance stats.</Typography>}
      {!isPending && !isError && stats && (
        <Stack className={styles.statsGrid}>
          <StatsGroup
            title="Runtime"
            rows={[
              { label: 'App version', value: stats.applicationVersion },
              { label: 'Environment', value: stats.environment },
              { label: 'Rust toolchain', value: stats.frameworkVersion },
              { label: 'Operating system', value: stats.operatingSystem },
            ]}
          />
          <StatsGroup
            title="Rendering"
            rows={[
              { label: 'Preview GPU', value: formatEnabled(stats.previewGpuEnabled) },
              { label: 'GPU availability', value: formatAvailability(stats.previewGpuAvailable) },
              { label: 'GPU renderer', value: stats.previewRenderer },
              { label: 'Preview version', value: stats.previewGenerationVersion },
              { label: 'Hull version', value: stats.hullGenerationVersion },
            ]}
          />
          <StatsGroup
            title="Database"
            rows={[
              { label: 'Models in database', value: stats.modelCount },
              { label: 'Models with previews', value: stats.modelsWithPreviews },
              { label: 'Directory configs', value: stats.directoryConfigCount },
              { label: 'Printing lists', value: stats.printingListCount },
              { label: 'Metadata dictionary values', value: stats.metadataDictionaryValueCount },
            ]}
          />
        </Stack>
      )}
    </Stack>
  );
}

export default function SettingsPage() {
  const location = useLocation();
  const { data: appConfig, isPending: appConfigPending, isError: appConfigError } = useAppConfig();
  const {
    data: instanceStats,
    isPending: instanceStatsPending,
    isError: instanceStatsError,
  } = useInstanceStats();
  const updateAppConfigMutation = useUpdateAppConfig();
  const [defaultRaftHeightMm, setDefaultRaftHeightMm] = useState('');
  const [theme, setTheme] = useState<string>('nord');
  const [generatePreviewsEnabled, setGeneratePreviewsEnabled] = useState(true);
  const [minimumPreviewGenerationVersion, setMinimumPreviewGenerationVersion] = useState('0');
  const [newPrinterName, setNewPrinterName] = useState('');
  const [newPrinterWidthMm, setNewPrinterWidthMm] = useState('228');
  const [newPrinterDepthMm, setNewPrinterDepthMm] = useState('128');
  const [printerEdits, setPrinterEdits] = useState<
    Record<string, { name: string; bedWidthMm: string; bedDepthMm: string }>
  >({});
  const { data, isPending, isError } = useMetadataDictionaryOverview();
  const { data: printers = [] } = usePrinters();
  const createPrinterMutation = useCreatePrinter();
  const deletePrinterMutation = useDeletePrinter();
  const setDefaultPrinterMutation = useSetDefaultPrinter();
  const updatePrinterMutation = useUpdatePrinter();
  const [logsChannel, setLogsChannel] = useState('');
  const [logsSeverity, setLogsSeverity] = useState('');
  const [showExceptionsOnly, setShowExceptionsOnly] = useState(false);
  const {
    data: logsData,
    isPending: logsPending,
    isError: logsError,
    refetch: refetchLogs,
    isRefetching: logsRefetching,
  } = useApplicationLogs(logsChannel, logsSeverity, LOG_LIMIT);

  useEffect(() => {
    if (appConfig) {
      setDefaultRaftHeightMm(String(appConfig.defaultRaftHeightMm));
      setTheme(appConfig.theme);
      setGeneratePreviewsEnabled(appConfig.generatePreviewsEnabled);
      setMinimumPreviewGenerationVersion(String(appConfig.minimumPreviewGenerationVersion));
    }
  }, [appConfig]);

  useEffect(() => {
    setPrinterEdits((current) => {
      const next: Record<string, { name: string; bedWidthMm: string; bedDepthMm: string }> = {};
      for (const printer of printers) {
        next[printer.id] = current[printer.id] ?? {
          name: printer.name,
          bedWidthMm: String(printer.bedWidthMm),
          bedDepthMm: String(printer.bedDepthMm),
        };
      }
      return next;
    });
  }, [printers]);

  const raftHeightValue = Number(defaultRaftHeightMm);
  const minimumPreviewGenerationVersionValue = Number(minimumPreviewGenerationVersion);
  const previewGenerationVersionLimit = instanceStats?.previewGenerationVersion;

  const appConfigValid =
    defaultRaftHeightMm.trim().length > 0 &&
    Number.isFinite(raftHeightValue) &&
    raftHeightValue >= 0 &&
    Number.isInteger(minimumPreviewGenerationVersionValue) &&
    minimumPreviewGenerationVersionValue >= 0 &&
    (previewGenerationVersionLimit === undefined ||
      minimumPreviewGenerationVersionValue <= previewGenerationVersionLimit);

  const currentSection: SettingsSectionKey = useMemo(() => {
    if (location.pathname.startsWith('/settings/printers')) return 'printers';
    if (location.pathname.startsWith('/settings/logs')) return 'logs';
    if (location.pathname.startsWith('/settings/schema')) return 'schema';
    if (location.pathname.startsWith('/settings/stats')) return 'stats';
    return 'settings';
  }, [location.pathname]);

  const logsEntries = useMemo(() => {
    if (!logsData) return [];
    if (!showExceptionsOnly) return logsData.entries;
    return logsData.entries.filter((entry) => !!entry.exception);
  }, [logsData, showExceptionsOnly]);

  const existingPrinterRows = useMemo(
    () =>
      [...printers].sort((a, b) =>
        a.isBuiltIn === b.isBuiltIn ? a.name.localeCompare(b.name) : a.isBuiltIn ? -1 : 1,
      ),
    [printers],
  );

  const saveConfig = () =>
    updateAppConfigMutation.mutate({
      defaultRaftHeightMm: raftHeightValue,
      theme,
      generatePreviewsEnabled,
      minimumPreviewGenerationVersion: minimumPreviewGenerationVersionValue,
    });

  if (isPending || appConfigPending) return <LoadingView />;

  if (isError || appConfigError || !data || !appConfig) {
    return <ErrorView message="Failed to load settings." />;
  }

  if (currentSection === 'logs' && logsPending) return <LoadingView />;

  if (currentSection === 'logs' && (logsError || !logsData)) {
    return <ErrorView message="Failed to load application logs." />;
  }

  return (
    <PageLayout variant="full" spacing={2}>
      <Typography component="h1" variant="page-title">
        Settings
      </Typography>

      <Stack direction={{ xs: 'column', md: 'row' }} spacing={3} className={styles.settingsLayout}>
        <Stack className={styles.sidebar}>
          <List disablePadding>
            {SETTINGS_SECTIONS.map((section) => (
              <ListItemButton
                key={section.key}
                component={Link}
                to={section.path}
                selected={currentSection === section.key}
              >
                <ListItemText primary={section.label} />
              </ListItemButton>
            ))}
          </List>
        </Stack>

        <Stack spacing={2} className={styles.contentArea}>
          {currentSection === 'settings' && (
            <Stack className={styles.globalSettingsSection}>
              <Stack spacing={2}>
                <Stack direction="row" spacing={1} alignItems="center" className={styles.addRow}>
                  <TextField
                    size="small"
                    type="number"
                    label="Raft height (mm)"
                    value={defaultRaftHeightMm}
                    onChange={(e) => setDefaultRaftHeightMm(e.target.value)}
                  />
                </Stack>
                <Stack direction="row" spacing={2} alignItems="center">
                  <Typography>Theme</Typography>
                  <ToggleButtonGroup
                    size="small"
                    exclusive
                    value={theme}
                    onChange={(_, v) => {
                      if (v) setTheme(v);
                    }}
                  >
                    <ToggleButton value="default">Default</ToggleButton>
                    <ToggleButton value="nord">Nord</ToggleButton>
                  </ToggleButtonGroup>
                </Stack>
                <Stack direction="row" spacing={1}>
                  <FormControlLabel
                    control={
                      <Switch
                        checked={generatePreviewsEnabled}
                        onChange={(e) => setGeneratePreviewsEnabled(e.target.checked)}
                      />
                    }
                    label="Generate previews"
                  />
                </Stack>
                <Stack direction="row" spacing={1} alignItems="center" className={styles.addRow}>
                  <TextField
                    size="small"
                    type="number"
                    label="Minimum preview version"
                    value={minimumPreviewGenerationVersion}
                    onChange={(e) => setMinimumPreviewGenerationVersion(e.target.value)}
                    error={
                      !Number.isInteger(minimumPreviewGenerationVersionValue) ||
                      minimumPreviewGenerationVersionValue < 0 ||
                      (previewGenerationVersionLimit !== undefined &&
                        minimumPreviewGenerationVersionValue > previewGenerationVersionLimit)
                    }
                    helperText={
                      previewGenerationVersionLimit === undefined
                        ? 'Set the minimum cached preview version accepted during indexing.'
                        : `Must be an integer between 0 and ${previewGenerationVersionLimit}. Existing previews below this version will be regenerated on scan.`
                    }
                  />
                </Stack>
                <Stack direction="row" spacing={1}>
                  <Button
                    variant="contained"
                    disabled={updateAppConfigMutation.isPending || !appConfigValid}
                    onClick={saveConfig}
                  >
                    Save
                  </Button>
                </Stack>
              </Stack>
            </Stack>
          )}

          {currentSection === 'printers' && (
            <Stack className={styles.globalSettingsSection}>
              <Stack spacing={2}>
                <Typography variant="h6">Printers</Typography>

                <Stack spacing={1}>
                  {existingPrinterRows.map((printer) => {
                    const edit = printerEdits[printer.id] ?? {
                      name: printer.name,
                      bedWidthMm: String(printer.bedWidthMm),
                      bedDepthMm: String(printer.bedDepthMm),
                    };
                    const width = Number(edit.bedWidthMm);
                    const depth = Number(edit.bedDepthMm);
                    const canSaveCustom =
                      !printer.isBuiltIn &&
                      edit.name.trim().length > 0 &&
                      Number.isFinite(width) &&
                      width > 0 &&
                      Number.isFinite(depth) &&
                      depth > 0 &&
                      (edit.name.trim() !== printer.name ||
                        width !== printer.bedWidthMm ||
                        depth !== printer.bedDepthMm);

                    return (
                      <Stack
                        key={printer.id}
                        direction={{ xs: 'column', md: 'row' }}
                        spacing={1}
                        alignItems={{ xs: 'stretch', md: 'center' }}
                      >
                        <TextField
                          size="small"
                          label="Name"
                          value={edit.name}
                          disabled={printer.isBuiltIn}
                          onChange={(e) =>
                            setPrinterEdits((current) => ({
                              ...current,
                              [printer.id]: {
                                ...edit,
                                name: e.target.value,
                              },
                            }))
                          }
                        />
                        <TextField
                          size="small"
                          type="number"
                          label="Width (mm)"
                          value={edit.bedWidthMm}
                          disabled={printer.isBuiltIn}
                          onChange={(e) =>
                            setPrinterEdits((current) => ({
                              ...current,
                              [printer.id]: {
                                ...edit,
                                bedWidthMm: e.target.value,
                              },
                            }))
                          }
                        />
                        <TextField
                          size="small"
                          type="number"
                          label="Depth (mm)"
                          value={edit.bedDepthMm}
                          disabled={printer.isBuiltIn}
                          onChange={(e) =>
                            setPrinterEdits((current) => ({
                              ...current,
                              [printer.id]: {
                                ...edit,
                                bedDepthMm: e.target.value,
                              },
                            }))
                          }
                        />
                        <Stack direction="row" spacing={1}>
                          {!printer.isBuiltIn && (
                            <Button
                              variant="contained"
                              disabled={!canSaveCustom || updatePrinterMutation.isPending}
                              onClick={() =>
                                updatePrinterMutation.mutate({
                                  id: printer.id,
                                  name: edit.name.trim(),
                                  bedWidthMm: Number(edit.bedWidthMm),
                                  bedDepthMm: Number(edit.bedDepthMm),
                                })
                              }
                            >
                              Save
                            </Button>
                          )}
                          <Button
                            variant={printer.isDefault ? 'contained' : 'outlined'}
                            disabled={printer.isDefault || setDefaultPrinterMutation.isPending}
                            onClick={() => setDefaultPrinterMutation.mutate(printer.id)}
                          >
                            {printer.isDefault ? 'Default' : 'Set default'}
                          </Button>
                          {!printer.isBuiltIn && (
                            <Button
                              variant="warning"
                              disabled={deletePrinterMutation.isPending || printer.isDefault}
                              onClick={() => deletePrinterMutation.mutate(printer.id)}
                            >
                              Delete
                            </Button>
                          )}
                          {printer.isBuiltIn && <Chip size="small" label="Built in" />}
                        </Stack>
                      </Stack>
                    );
                  })}
                </Stack>

                <Divider />

                <Typography variant="h6">New Printer</Typography>

                <Stack direction={{ xs: 'column', md: 'row' }} spacing={1} alignItems="center">
                  <TextField
                    size="small"
                    label="Printer name"
                    value={newPrinterName}
                    onChange={(e) => setNewPrinterName(e.target.value)}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Width (mm)"
                    value={newPrinterWidthMm}
                    onChange={(e) => setNewPrinterWidthMm(e.target.value)}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Depth (mm)"
                    value={newPrinterDepthMm}
                    onChange={(e) => setNewPrinterDepthMm(e.target.value)}
                  />
                  <Button
                    variant="contained"
                    disabled={
                      createPrinterMutation.isPending ||
                      !newPrinterName.trim() ||
                      Number(newPrinterWidthMm) <= 0 ||
                      Number(newPrinterDepthMm) <= 0
                    }
                    onClick={() => {
                      createPrinterMutation.mutate(
                        {
                          name: newPrinterName.trim(),
                          bedWidthMm: Number(newPrinterWidthMm),
                          bedDepthMm: Number(newPrinterDepthMm),
                        },
                        {
                          onSuccess: () => {
                            setNewPrinterName('');
                          },
                        },
                      );
                    }}
                  >
                    Add printer
                  </Button>
                </Stack>
              </Stack>
            </Stack>
          )}

          {currentSection === 'logs' && logsData && (
            <Stack spacing={2} className={styles.globalSettingsSection}>
              <Stack direction="row" spacing={2} className={styles.filterRow}>
                <TextField
                  select
                  size="small"
                  label="Channel"
                  value={logsChannel}
                  onChange={(e) => setLogsChannel(e.target.value)}
                  className={`${styles.filterControl} ${styles.channelFilterControl}`}
                >
                  <MenuItem value="">All channels</MenuItem>
                  {logsData.availableChannels.map((item) => (
                    <MenuItem key={item} value={item}>
                      {item}
                    </MenuItem>
                  ))}
                </TextField>

                <TextField
                  select
                  size="small"
                  label="Minimum severity"
                  value={logsSeverity}
                  onChange={(e) => setLogsSeverity(e.target.value)}
                  className={styles.filterControl}
                >
                  <MenuItem value="">All severities</MenuItem>
                  {logsData.availableSeverities.map((item) => (
                    <MenuItem key={item} value={item}>
                      {item}
                    </MenuItem>
                  ))}
                </TextField>

                <Button
                  variant={showExceptionsOnly ? 'contained' : 'outlined'}
                  onClick={() => setShowExceptionsOnly((prev) => !prev)}
                >
                  {showExceptionsOnly ? 'Showing exceptions only' : 'Filter to exceptions'}
                </Button>

                <Button variant="outlined" disabled={logsRefetching} onClick={() => refetchLogs()}>
                  Refresh
                </Button>
              </Stack>

              <Typography color="text.secondary">
                Showing newest {LOG_LIMIT} log entries from this running backend instance.
              </Typography>

              <Stack spacing={1} className={styles.logsList}>
                {logsEntries.map((entry, index) => (
                  <Stack key={`${entry.timestamp}-${index}`} className={styles.logCard} spacing={1}>
                    <Stack direction="row" spacing={1} className={styles.logMetaRow}>
                      <Typography className={styles.logMetaText}>
                        {new Date(entry.timestamp).toLocaleString()}
                      </Typography>
                      <Typography className={styles.logMetaText}>{entry.severity}</Typography>
                      <Typography className={styles.logMetaText}>{entry.channel}</Typography>
                    </Stack>
                    <Typography variant="body2">{entry.message}</Typography>
                    {entry.exception && (
                      <Typography
                        variant="caption"
                        component="pre"
                        className={styles.exceptionText}
                      >
                        {entry.exception}
                      </Typography>
                    )}
                  </Stack>
                ))}
                {logsEntries.length === 0 && (
                  <Typography color="text.secondary">
                    No logs matched the selected filters.
                  </Typography>
                )}
              </Stack>
            </Stack>
          )}

          {currentSection === 'schema' && (
            <Stack spacing={2} className={styles.globalSettingsSection}>
              <Stack direction="column" spacing={2} className={styles.sectionsColumn}>
                <FieldSection field="category" data={data.category} />
                <FieldSection field="type" data={data.type} />
                <FieldSection field="material" data={data.material} />
                <FieldSection field="tags" data={data.tags} />
              </Stack>
            </Stack>
          )}

          {currentSection === 'stats' && (
            <InstanceStatsSection
              stats={instanceStats}
              isPending={instanceStatsPending}
              isError={instanceStatsError}
            />
          )}
        </Stack>
      </Stack>
    </PageLayout>
  );
}
