import { Fragment, useEffect, useMemo, useState } from 'react';
import { Link, useLocation, useNavigate } from 'react-router-dom';
import {
  Button,
  Checkbox,
  HelpTooltip,
  Input,
  Label,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Switch,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
  Tag,
} from '@jamiehales/ui-kit/mostlymaking';
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
import PageLayout, { PageHeader } from '../components/layouts/PageLayout';
import { useApplicationLogs } from '../lib/queries';
import styles from './SettingsPage.module.css';

type FieldKey = 'category' | 'type' | 'material' | 'tags';

const FIELD_LABELS: Record<FieldKey, string> = {
  category: 'Category',
  type: 'Type',
  material: 'Material',
  tags: 'Tags',
};

const ALL = '__all__';

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
  const label = FIELD_LABELS[field];

  return (
    <section className={styles.panel} aria-labelledby={`schema-${field}`}>
      <div className={styles.row}>
        <h2 id={`schema-${field}`} className={styles.rowTitle}>
          {label}
        </h2>
        <span className={styles.mono}>{data.configured.length} values</span>
      </div>

      <div className={styles.stack}>
        {data.configured.map((item) => {
          const current = editValues[item.id] ?? item.value;
          return (
            <div key={item.id} className={styles.inline}>
              <Input
                aria-label={`${label} value`}
                value={current}
                onChange={(e) =>
                  setEditValues((prev) => ({
                    ...prev,
                    [item.id]: e.target.value,
                  }))
                }
              />
              <Button
                size="sm"
                variant="secondary"
                disabled={updateMutation.isPending || !current.trim() || current === item.value}
                onClick={() => updateMutation.mutate({ id: item.id, value: current.trim() })}
              >
                Save
              </Button>
              <Button
                size="sm"
                variant="ghost"
                className={styles.delete}
                disabled={deleteMutation.isPending}
                onClick={() => deleteMutation.mutate(item.id)}
              >
                Delete
              </Button>
            </div>
          );
        })}
        <form
          className={styles.inline}
          onSubmit={async (e) => {
            e.preventDefault();
            if (!newValue.trim()) return;
            await createMutation.mutateAsync({ field, value: newValue.trim() });
            setNewValue('');
          }}
        >
          <Input
            aria-label={`New ${label} value`}
            placeholder="New value"
            value={newValue}
            onChange={(e) => setNewValue(e.target.value)}
          />
          <Button
            size="sm"
            variant="secondary"
            type="submit"
            disabled={createMutation.isPending || !newValue.trim()}
          >
            Add
          </Button>
        </form>
      </div>

      <div className={styles.observed}>
        <span className="mm-type-label">In use</span>
        <div className={styles.tags}>
          {data.observed.map((value) => (
            <Tag
              key={value}
              asChild
              variant={observedOnly.includes(value) ? 'default' : 'neutral'}
              title={observedOnly.includes(value) ? 'Not a configured value' : undefined}
            >
              <Link to={toModelsHref(field, value)}>{value}</Link>
            </Tag>
          ))}
          {data.observed.length === 0 && <span className={styles.muted}>Not used yet.</span>}
        </div>
      </div>
    </section>
  );
}

const SETTINGS_SECTIONS = [
  { key: 'settings', label: 'General', path: '/settings' },
  { key: 'printers', label: 'Printers', path: '/settings/printers' },
  { key: 'schema', label: 'Schema', path: '/settings/schema' },
  { key: 'logs', label: 'Logs', path: '/settings/logs' },
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
    <section className={styles.panel} aria-label={title}>
      <h2 className={`mm-type-label ${styles.statsTitle}`}>{title}</h2>
      <dl className={styles.stats}>
        {rows.map((row) => (
          <Fragment key={row.label}>
            <dt>{row.label}</dt>
            <dd>{typeof row.value === 'number' ? row.value.toLocaleString() : row.value}</dd>
          </Fragment>
        ))}
      </dl>
    </section>
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
  if (isPending) return <LoadingView />;
  if (isError || !stats) return <ErrorView message="Couldn't load stats." />;
  return (
    <div className={styles.statsGrid}>
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
          { label: 'Models', value: stats.modelCount },
          { label: 'With previews', value: stats.modelsWithPreviews },
          { label: 'Directory configs', value: stats.directoryConfigCount },
          { label: 'Printing lists', value: stats.printingListCount },
          { label: 'Dictionary values', value: stats.metadataDictionaryValueCount },
        ]}
      />
    </div>
  );
}

export default function SettingsPage() {
  const location = useLocation();
  const navigate = useNavigate();
  const { data: appConfig, isPending: appConfigPending, isError: appConfigError } = useAppConfig();
  const {
    data: instanceStats,
    isPending: instanceStatsPending,
    isError: instanceStatsError,
  } = useInstanceStats();
  const updateAppConfigMutation = useUpdateAppConfig();
  const [defaultRaftHeightMm, setDefaultRaftHeightMm] = useState('');
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
      generatePreviewsEnabled,
      minimumPreviewGenerationVersion: minimumPreviewGenerationVersionValue,
    });

  if (isPending || appConfigPending) return <LoadingView />;

  if (isError || appConfigError || !data || !appConfig) {
    return <ErrorView message="Couldn't load settings." />;
  }

  const previewVersionInvalid =
    !Number.isInteger(minimumPreviewGenerationVersionValue) ||
    minimumPreviewGenerationVersionValue < 0 ||
    (previewGenerationVersionLimit !== undefined &&
      minimumPreviewGenerationVersionValue > previewGenerationVersionLimit);

  return (
    <PageLayout>
      <PageHeader title="Settings" />

      <Tabs
        variant="vertical"
        value={currentSection}
        onValueChange={(next) => {
          const section = SETTINGS_SECTIONS.find((s) => s.key === next);
          if (section) navigate(section.path);
        }}
        className={styles.tabs}
      >
        <TabsList aria-label="Settings sections">
          {SETTINGS_SECTIONS.map((section) => (
            <TabsTrigger key={section.key} value={section.key}>
              {section.label}
            </TabsTrigger>
          ))}
        </TabsList>

        <TabsContent value="settings" className={styles.content}>
          <section className={styles.panel} aria-label="General">
            <div className={styles.row}>
              <div className={styles.rowLabel}>
                <span className={styles.rowTitle}>Model root</span>
                <span className={styles.mono}>{appConfig.modelsDirectoryPath ?? 'Not set'}</span>
              </div>
            </div>
            <div className={styles.row}>
              <Label htmlFor="raft-height" className={styles.rowTitle}>
                Default raft height
              </Label>
              <div className={styles.unit}>
                <Input
                  id="raft-height"
                  type="number"
                  min={0}
                  step="any"
                  value={defaultRaftHeightMm}
                  aria-invalid={
                    !Number.isFinite(raftHeightValue) || raftHeightValue < 0 || undefined
                  }
                  onChange={(e) => setDefaultRaftHeightMm(e.target.value)}
                />
                <span>mm</span>
              </div>
            </div>
            <div className={styles.row}>
              <Label htmlFor="generate-previews" className={styles.rowTitle}>
                Generate previews
              </Label>
              <Switch
                id="generate-previews"
                checked={generatePreviewsEnabled}
                onCheckedChange={(checked) => setGeneratePreviewsEnabled(checked)}
              />
            </div>
            <div className={styles.row}>
              <div className={styles.rowLabelInline}>
                <Label htmlFor="min-preview-version" className={styles.rowTitle}>
                  Minimum preview version
                </Label>
                <HelpTooltip
                  aria-label="About minimum preview version"
                  content="Previews can be slow to generate on large libraries, change this version to force previews created on old versions to be regenerated"
                />
              </div>
              <div className={styles.versionField}>
                <Input
                  id="min-preview-version"
                  type="number"
                  min={0}
                  max={previewGenerationVersionLimit}
                  step={1}
                  value={minimumPreviewGenerationVersion}
                  aria-invalid={previewVersionInvalid || undefined}
                  onChange={(e) => setMinimumPreviewGenerationVersion(e.target.value)}
                />
                {previewVersionInvalid && previewGenerationVersionLimit !== undefined && (
                  <p role="alert" className={styles.error}>
                    Use a whole number from 0 to {previewGenerationVersionLimit}.
                  </p>
                )}
              </div>
            </div>
          </section>
          <div className={styles.footer}>
            <Button
              variant="primary"
              disabled={updateAppConfigMutation.isPending || !appConfigValid}
              onClick={saveConfig}
            >
              Save
            </Button>
          </div>
        </TabsContent>

        <TabsContent value="printers" className={styles.content}>
          <section className={styles.panel} aria-label="Printers">
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
              const setEdit = (patch: Partial<typeof edit>) =>
                setPrinterEdits((current) => ({ ...current, [printer.id]: { ...edit, ...patch } }));

              return (
                <div key={printer.id} className={styles.printer}>
                  {printer.isBuiltIn ? (
                    <div className={styles.printerName}>
                      <span className={styles.rowTitle}>{printer.name}</span>
                      <span className={styles.mono}>
                        {printer.bedWidthMm} × {printer.bedDepthMm} mm
                      </span>
                    </div>
                  ) : (
                    <div className={styles.printerFields}>
                      <Input
                        aria-label="Name"
                        value={edit.name}
                        onChange={(e) => setEdit({ name: e.target.value })}
                      />
                      <div className={styles.unit}>
                        <Input
                          aria-label="Width (mm)"
                          type="number"
                          value={edit.bedWidthMm}
                          onChange={(e) => setEdit({ bedWidthMm: e.target.value })}
                        />
                        <span>mm</span>
                      </div>
                      <div className={styles.unit}>
                        <Input
                          aria-label="Depth (mm)"
                          type="number"
                          value={edit.bedDepthMm}
                          onChange={(e) => setEdit({ bedDepthMm: e.target.value })}
                        />
                        <span>mm</span>
                      </div>
                    </div>
                  )}
                  <div className={styles.printerActions}>
                    {printer.isDefault && <Tag>Default</Tag>}
                    {printer.isBuiltIn && <Tag variant="neutral">Built in</Tag>}
                    {!printer.isBuiltIn && (
                      <Button
                        size="sm"
                        variant="secondary"
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
                    {!printer.isDefault && (
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={setDefaultPrinterMutation.isPending}
                        onClick={() => setDefaultPrinterMutation.mutate(printer.id)}
                      >
                        Make default
                      </Button>
                    )}
                    {!printer.isBuiltIn && (
                      <Button
                        size="sm"
                        variant="ghost"
                        className={styles.delete}
                        disabled={deletePrinterMutation.isPending || printer.isDefault}
                        onClick={() => deletePrinterMutation.mutate(printer.id)}
                      >
                        Delete
                      </Button>
                    )}
                  </div>
                </div>
              );
            })}
          </section>

          <form
            className={`${styles.panel} ${styles.addPrinter}`}
            aria-label="Add printer"
            onSubmit={(e) => {
              e.preventDefault();
              createPrinterMutation.mutate(
                {
                  name: newPrinterName.trim(),
                  bedWidthMm: Number(newPrinterWidthMm),
                  bedDepthMm: Number(newPrinterDepthMm),
                },
                { onSuccess: () => setNewPrinterName('') },
              );
            }}
          >
            <div className={styles.field}>
              <Label htmlFor="new-printer-name">Name</Label>
              <Input
                id="new-printer-name"
                value={newPrinterName}
                onChange={(e) => setNewPrinterName(e.target.value)}
              />
            </div>
            <div className={styles.field}>
              <Label htmlFor="new-printer-width">Width</Label>
              <div className={styles.unit}>
                <Input
                  id="new-printer-width"
                  type="number"
                  value={newPrinterWidthMm}
                  onChange={(e) => setNewPrinterWidthMm(e.target.value)}
                />
                <span>mm</span>
              </div>
            </div>
            <div className={styles.field}>
              <Label htmlFor="new-printer-depth">Depth</Label>
              <div className={styles.unit}>
                <Input
                  id="new-printer-depth"
                  type="number"
                  value={newPrinterDepthMm}
                  onChange={(e) => setNewPrinterDepthMm(e.target.value)}
                />
                <span>mm</span>
              </div>
            </div>
            <Button
              variant="primary"
              type="submit"
              disabled={
                createPrinterMutation.isPending ||
                !newPrinterName.trim() ||
                Number(newPrinterWidthMm) <= 0 ||
                Number(newPrinterDepthMm) <= 0
              }
            >
              Add printer
            </Button>
          </form>
        </TabsContent>

        <TabsContent value="schema" className={styles.content}>
          <FieldSection field="category" data={data.category} />
          <FieldSection field="type" data={data.type} />
          <FieldSection field="material" data={data.material} />
          <FieldSection field="tags" data={data.tags} />
        </TabsContent>

        <TabsContent value="logs" className={styles.content}>
          {logsPending ? (
            <LoadingView />
          ) : logsError || !logsData ? (
            <ErrorView message="Couldn't load the logs." />
          ) : (
            <>
              <div className={styles.logFilters}>
                <Select
                  value={logsChannel || ALL}
                  onValueChange={(v) => setLogsChannel(v === ALL ? '' : v)}
                >
                  <SelectTrigger aria-label="Channel" className={styles.filter}>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={ALL}>All channels</SelectItem>
                    {logsData.availableChannels.map((item) => (
                      <SelectItem key={item} value={item}>
                        {item}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Select
                  value={logsSeverity || ALL}
                  onValueChange={(v) => setLogsSeverity(v === ALL ? '' : v)}
                >
                  <SelectTrigger aria-label="Minimum severity" className={styles.filter}>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={ALL}>All severities</SelectItem>
                    {logsData.availableSeverities.map((item) => (
                      <SelectItem key={item} value={item}>
                        {item}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <Checkbox
                  checked={showExceptionsOnly}
                  onCheckedChange={(checked) => setShowExceptionsOnly(checked === true)}
                >
                  Exceptions only
                </Checkbox>
                <div className={styles.spacer} />
                <span className={styles.mono}>Newest {LOG_LIMIT}</span>
                <Button variant="secondary" disabled={logsRefetching} onClick={() => refetchLogs()}>
                  Refresh
                </Button>
              </div>

              <section className={styles.panel} aria-label="Log entries">
                {logsEntries.map((entry, index) => {
                  const severity = entry.severity.toLowerCase();
                  return (
                    <div key={`${entry.timestamp}-${index}`} className={styles.log}>
                      <span className={styles.mono}>
                        {new Date(entry.timestamp).toLocaleString()}
                      </span>
                      <span
                        className={`${styles.mono} ${severity.startsWith('err') || severity === 'critical' ? styles.errorText : severity.startsWith('warn') ? styles.warnText : ''}`}
                      >
                        {entry.severity}
                      </span>
                      <span className={styles.mono}>{entry.channel}</span>
                      <span className={styles.logMessage}>
                        {entry.message}
                        {entry.exception && (
                          <pre className={styles.exception}>{entry.exception}</pre>
                        )}
                      </span>
                    </div>
                  );
                })}
                {logsEntries.length === 0 && (
                  <p className={styles.emptyRow}>No log entries match these filters.</p>
                )}
              </section>
            </>
          )}
        </TabsContent>

        <TabsContent value="stats" className={styles.content}>
          <InstanceStatsSection
            stats={instanceStats}
            isPending={instanceStatsPending}
            isError={instanceStatsError}
          />
        </TabsContent>
      </Tabs>
    </PageLayout>
  );
}
