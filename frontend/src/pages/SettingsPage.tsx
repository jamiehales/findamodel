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
  Tab,
  Tabs,
  TextField,
  ToggleButton,
  ToggleButtonGroup,
  Typography,
} from '@mui/material';
import {
  useAppConfig,
  useAutoSupportSettingsPreviewGeometry,
  useCreatePrinter,
  useCreateAutoSupportSettingsPreview,
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
import type {
  AutoSupportSettingsPreviewScenario,
  InstanceStats,
  MetadataDictionaryField,
  PrinterCtbSettings,
} from '../lib/api';
import ErrorView from '../components/ErrorView';
import LoadingView from '../components/LoadingView';
import AutoSupportLayerSlider from '../components/AutoSupportLayerSlider';
import ModelViewer from '../components/ModelViewer';
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
  { key: 'autosupport', label: 'Auto Supports', path: '/settings/autosupport' },
  { key: 'printers', label: 'Printers', path: '/settings/printers' },
  { key: 'logs', label: 'Logs', path: '/settings/logs' },
  { key: 'ai', label: 'AI Settings', path: '/settings/ai' },
  { key: 'schema', label: 'Schema', path: '/settings/schema' },
  { key: 'stats', label: 'Stats', path: '/settings/stats' },
] as const;

type SettingsSectionKey = (typeof SETTINGS_SECTIONS)[number]['key'];

const LOG_LIMIT = 500;

const DEFAULT_AUTO_SUPPORT_PREVIEW_SCENARIOS: AutoSupportSettingsPreviewScenario[] = [
  {
    scenarioId: 'thin-plane-parallel',
    name: 'Box 40x40x2mm (parallel)',
    source: 'builtin',
    status: 'not-generated',
    supportCount: 0,
    errorMessage: null,
    supportPoints: null,
    islands: null,
  },
  {
    scenarioId: 'thin-plane-30deg',
    name: 'Box 40x40x2mm (30 degrees)',
    source: 'builtin',
    status: 'not-generated',
    supportCount: 0,
    errorMessage: null,
    supportPoints: null,
    islands: null,
  },
  {
    scenarioId: 'sphere-40',
    name: 'Sphere 40mm diameter',
    source: 'builtin',
    status: 'not-generated',
    supportCount: 0,
    errorMessage: null,
    supportPoints: null,
    islands: null,
  },
  {
    scenarioId: 'cube-40',
    name: 'Cube 40mm',
    source: 'builtin',
    status: 'not-generated',
    supportCount: 0,
    errorMessage: null,
    supportPoints: null,
    islands: null,
  },
  {
    scenarioId: 'cube-40-rotated-45',
    name: 'Cube 40mm rotated 45 degrees',
    source: 'builtin',
    status: 'not-generated',
    supportCount: 0,
    errorMessage: null,
    supportPoints: null,
    islands: null,
  },
  {
    scenarioId: 'cone-upside-down',
    name: 'Upside-down cone 40mm diameter x 80mm height',
    source: 'builtin',
    status: 'not-generated',
    supportCount: 0,
    errorMessage: null,
    supportPoints: null,
    islands: null,
  },
  {
    scenarioId: 'donut-40',
    name: 'Donut 40mm diameter',
    source: 'builtin',
    status: 'not-generated',
    supportCount: 0,
    errorMessage: null,
    supportPoints: null,
    islands: null,
  },
];

const AUTO_SUPPORT_PREVIEW_ORDER = new Map(
  DEFAULT_AUTO_SUPPORT_PREVIEW_SCENARIOS.map((scenario, index) => [scenario.scenarioId, index]),
);

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
              { label: '.NET runtime', value: stats.frameworkVersion },
              { label: 'Operating system', value: stats.operatingSystem },
            ]}
          />
          <StatsGroup
            title="Rendering"
            rows={[
              { label: 'Preview GPU', value: formatEnabled(stats.previewGpuEnabled) },
              { label: 'GPU availability', value: formatAvailability(stats.previewGpuAvailable) },
              { label: 'GL renderer', value: stats.previewRenderer },
              { label: 'Preview version', value: stats.previewGenerationVersion },
              { label: 'Hull version', value: stats.hullGenerationVersion },
            ]}
          />
          <StatsGroup
            title="AI"
            rows={[
              { label: 'Internal LLM GPU', value: formatEnabled(stats.internalLlmGpuEnabled) },
              { label: 'GPU layers', value: stats.internalLlmGpuLayerCount },
              { label: 'Models with AI tags', value: stats.modelsWithGeneratedTags },
              {
                label: 'Models with AI descriptions',
                value: stats.modelsWithGeneratedDescriptions,
              },
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

function AutoSupportPreviewViewport({
  previewId,
  scenario,
  showForceMarkers,
}: {
  previewId: string | null;
  scenario: AutoSupportSettingsPreviewScenario;
  showForceMarkers: boolean;
}) {
  const {
    data: splitGeometry,
    isPending,
    isError,
  } = useAutoSupportSettingsPreviewGeometry(
    previewId,
    scenario.scenarioId,
    scenario.status === 'completed' && !!previewId,
  );

  const [selectedLayerIndex, setSelectedLayerIndex] = useState(Number.MAX_SAFE_INTEGER);
  const sliceLayers = scenario.sliceLayers ?? [];
  const hasSliceLayers = sliceLayers.length > 0;
  const clampedLayerIndex = hasSliceLayers
    ? Math.min(Math.max(selectedLayerIndex, 0), sliceLayers.length - 1)
    : 0;
  const selectedSliceLayer = hasSliceLayers ? sliceLayers[clampedLayerIndex] : null;

  useEffect(() => {
    if (!hasSliceLayers) {
      setSelectedLayerIndex(Number.MAX_SAFE_INTEGER);
      return;
    }

    setSelectedLayerIndex((current) => Math.min(Math.max(current, 0), sliceLayers.length - 1));
  }, [hasSliceLayers, sliceLayers.length, scenario.scenarioId]);

  return (
    <Stack spacing={1} className={styles.previewCard}>
      <Typography variant="subtitle1">{scenario.name}</Typography>
      <Typography variant="body2" color="text.secondary">
        {scenario.source === 'stl' ? 'Source: hardcoded STL' : 'Source: generated test shape'}
      </Typography>
      {scenario.status === 'failed' && (
        <Typography color="error">
          {scenario.errorMessage ?? 'Preview generation failed.'}
        </Typography>
      )}
      {scenario.status === 'completed' && (
        <Stack spacing={0.5}>
          <Typography color="text.secondary">
            Supports generated: {scenario.supportCount}
          </Typography>
          {selectedSliceLayer && (
            <Typography color="text.secondary">
              Layer {selectedSliceLayer.layerIndex + 1} of {sliceLayers.length} (
              {selectedSliceLayer.sliceHeightMm.toFixed(2)} mm)
            </Typography>
          )}
          {scenario.totalMs != null && (
            <Typography color="text.secondary">
              Timing: {scenario.totalMs.toFixed(0)} ms total
              {scenario.generateMs != null ? `, ${scenario.generateMs.toFixed(0)} ms generate` : ''}
              {scenario.encodeMs != null ? `, ${scenario.encodeMs.toFixed(0)} ms encode` : ''}
            </Typography>
          )}
        </Stack>
      )}
      {scenario.status === 'not-generated' && (
        <Typography color="text.secondary">This preview has not been generated yet.</Typography>
      )}
      <Stack className={styles.previewViewer}>
        {scenario.status !== 'completed' ? (
          <Stack className={styles.previewPlaceholder}>
            <Typography color="text.secondary">No preview available.</Typography>
          </Stack>
        ) : isPending ? (
          <Stack className={styles.previewPlaceholder}>
            <Typography color="text.secondary">Loading preview geometry...</Typography>
          </Stack>
        ) : isError || !splitGeometry ? (
          <Stack className={styles.previewPlaceholder}>
            <Typography color="error">Failed to load preview geometry.</Typography>
          </Stack>
        ) : (
          <Stack className={styles.previewViewerLayout} direction="row" spacing={0}>
            <Stack className={styles.previewViewerCanvas}>
              <ModelViewer
                modelId="settings-auto-support-preview"
                modelOverride={{
                  sphereCentreX: splitGeometry.body.sphereCentre.x,
                  sphereCentreY: splitGeometry.body.sphereCentre.y,
                  sphereCentreZ: splitGeometry.body.sphereCentre.z,
                  dimensionXMm: splitGeometry.body.dimensionXMm,
                  dimensionYMm: splitGeometry.body.dimensionYMm,
                  dimensionZMm: splitGeometry.body.dimensionZMm,
                  raftHeightMm: 0,
                }}
                convexHull={null}
                concaveHull={null}
                convexSansRaftHull={null}
                supported
                splitGeometryOverride={splitGeometry}
                supportPointsOverride={scenario.supportPoints}
                islandsOverride={scenario.islands}
                sliceLayersOverride={sliceLayers}
                selectedSliceLayerIndex={clampedLayerIndex}
                selectedSliceHeightMm={selectedSliceLayer?.sliceHeightMm ?? null}
                slicePreviewEnabled
                showForceMarkers={showForceMarkers}
              />
            </Stack>
            <AutoSupportLayerSlider
              className={styles.previewLayerSlider}
              sliceLayers={sliceLayers}
              selectedLayerIndex={clampedLayerIndex}
              onLayerChange={setSelectedLayerIndex}
            />
          </Stack>
        )}
      </Stack>
    </Stack>
  );
}

type PrinterCtbSettingsDraft = Record<keyof PrinterCtbSettings, string>;

const DEFAULT_PRINTER_CTB_SETTINGS_DRAFT: PrinterCtbSettingsDraft = {
  layerHeightMm: '0.05',
  bottomLayerCount: '4',
  transitionLayerCount: '0',
  exposureTimeSeconds: '2.5',
  bottomExposureTimeSeconds: '30',
  bottomLiftHeightMm: '6',
  bottomLiftSpeedMmPerMinute: '65',
  liftHeightMm: '6',
  liftSpeedMmPerMinute: '80',
  retractSpeedMmPerMinute: '150',
  bottomLightOffDelaySeconds: '0',
  lightOffDelaySeconds: '0',
  waitTimeBeforeCureSeconds: '0',
  waitTimeAfterCureSeconds: '0',
  waitTimeAfterLiftSeconds: '0',
  lightPwm: '255',
  bottomLightPwm: '255',
};

const PRINTER_CTB_FIELDS: Array<{
  key: keyof PrinterCtbSettings;
  label: string;
  min: number;
  step?: string;
}> = [
  { key: 'layerHeightMm', label: 'Layer Height (mm)', min: 0.001, step: '0.01' },
  { key: 'bottomLayerCount', label: 'Bottom Layers', min: 0, step: '1' },
  { key: 'transitionLayerCount', label: 'Transition Layers', min: 0, step: '1' },
  { key: 'exposureTimeSeconds', label: 'Exposure (s)', min: 0.001, step: '0.1' },
  { key: 'bottomExposureTimeSeconds', label: 'Bottom Exposure (s)', min: 0.001, step: '0.1' },
  { key: 'bottomLiftHeightMm', label: 'Bottom Lift Height (mm)', min: 0, step: '0.1' },
  { key: 'bottomLiftSpeedMmPerMinute', label: 'Bottom Lift Speed (mm/min)', min: 0.001, step: '1' },
  { key: 'liftHeightMm', label: 'Lift Height (mm)', min: 0, step: '0.1' },
  { key: 'liftSpeedMmPerMinute', label: 'Lift Speed (mm/min)', min: 0.001, step: '1' },
  { key: 'retractSpeedMmPerMinute', label: 'Retract Speed (mm/min)', min: 0.001, step: '1' },
  { key: 'bottomLightOffDelaySeconds', label: 'Bottom Light-Off Delay (s)', min: 0, step: '0.1' },
  { key: 'lightOffDelaySeconds', label: 'Light-Off Delay (s)', min: 0, step: '0.1' },
  { key: 'waitTimeBeforeCureSeconds', label: 'Wait Before Cure (s)', min: 0, step: '0.1' },
  { key: 'waitTimeAfterCureSeconds', label: 'Wait After Cure (s)', min: 0, step: '0.1' },
  { key: 'waitTimeAfterLiftSeconds', label: 'Wait After Lift (s)', min: 0, step: '0.1' },
  { key: 'lightPwm', label: 'Light PWM', min: 1, step: '1' },
  { key: 'bottomLightPwm', label: 'Bottom Light PWM', min: 1, step: '1' },
];

function toPrinterCtbSettingsDraft(printer: PrinterCtbSettings): PrinterCtbSettingsDraft {
  return {
    layerHeightMm: String(printer.layerHeightMm),
    bottomLayerCount: String(printer.bottomLayerCount),
    transitionLayerCount: String(printer.transitionLayerCount),
    exposureTimeSeconds: String(printer.exposureTimeSeconds),
    bottomExposureTimeSeconds: String(printer.bottomExposureTimeSeconds),
    bottomLiftHeightMm: String(printer.bottomLiftHeightMm),
    bottomLiftSpeedMmPerMinute: String(printer.bottomLiftSpeedMmPerMinute),
    liftHeightMm: String(printer.liftHeightMm),
    liftSpeedMmPerMinute: String(printer.liftSpeedMmPerMinute),
    retractSpeedMmPerMinute: String(printer.retractSpeedMmPerMinute),
    bottomLightOffDelaySeconds: String(printer.bottomLightOffDelaySeconds),
    lightOffDelaySeconds: String(printer.lightOffDelaySeconds),
    waitTimeBeforeCureSeconds: String(printer.waitTimeBeforeCureSeconds),
    waitTimeAfterCureSeconds: String(printer.waitTimeAfterCureSeconds),
    waitTimeAfterLiftSeconds: String(printer.waitTimeAfterLiftSeconds),
    lightPwm: String(printer.lightPwm),
    bottomLightPwm: String(printer.bottomLightPwm),
  };
}

function parsePrinterCtbSettingsDraft(draft: PrinterCtbSettingsDraft): PrinterCtbSettings | null {
  const layerHeightMm = Number(draft.layerHeightMm);
  const bottomLayerCount = Number(draft.bottomLayerCount);
  const transitionLayerCount = Number(draft.transitionLayerCount);
  const exposureTimeSeconds = Number(draft.exposureTimeSeconds);
  const bottomExposureTimeSeconds = Number(draft.bottomExposureTimeSeconds);
  const bottomLiftHeightMm = Number(draft.bottomLiftHeightMm);
  const bottomLiftSpeedMmPerMinute = Number(draft.bottomLiftSpeedMmPerMinute);
  const liftHeightMm = Number(draft.liftHeightMm);
  const liftSpeedMmPerMinute = Number(draft.liftSpeedMmPerMinute);
  const retractSpeedMmPerMinute = Number(draft.retractSpeedMmPerMinute);
  const bottomLightOffDelaySeconds = Number(draft.bottomLightOffDelaySeconds);
  const lightOffDelaySeconds = Number(draft.lightOffDelaySeconds);
  const waitTimeBeforeCureSeconds = Number(draft.waitTimeBeforeCureSeconds);
  const waitTimeAfterCureSeconds = Number(draft.waitTimeAfterCureSeconds);
  const waitTimeAfterLiftSeconds = Number(draft.waitTimeAfterLiftSeconds);
  const lightPwm = Number(draft.lightPwm);
  const bottomLightPwm = Number(draft.bottomLightPwm);

  const valid =
    Number.isFinite(layerHeightMm) &&
    layerHeightMm > 0 &&
    Number.isFinite(bottomLayerCount) &&
    Number.isInteger(bottomLayerCount) &&
    bottomLayerCount >= 0 &&
    Number.isFinite(transitionLayerCount) &&
    Number.isInteger(transitionLayerCount) &&
    transitionLayerCount >= 0 &&
    Number.isFinite(exposureTimeSeconds) &&
    exposureTimeSeconds > 0 &&
    Number.isFinite(bottomExposureTimeSeconds) &&
    bottomExposureTimeSeconds > 0 &&
    Number.isFinite(bottomLiftHeightMm) &&
    bottomLiftHeightMm >= 0 &&
    Number.isFinite(bottomLiftSpeedMmPerMinute) &&
    bottomLiftSpeedMmPerMinute > 0 &&
    Number.isFinite(liftHeightMm) &&
    liftHeightMm >= 0 &&
    Number.isFinite(liftSpeedMmPerMinute) &&
    liftSpeedMmPerMinute > 0 &&
    Number.isFinite(retractSpeedMmPerMinute) &&
    retractSpeedMmPerMinute > 0 &&
    Number.isFinite(bottomLightOffDelaySeconds) &&
    bottomLightOffDelaySeconds >= 0 &&
    Number.isFinite(lightOffDelaySeconds) &&
    lightOffDelaySeconds >= 0 &&
    Number.isFinite(waitTimeBeforeCureSeconds) &&
    waitTimeBeforeCureSeconds >= 0 &&
    Number.isFinite(waitTimeAfterCureSeconds) &&
    waitTimeAfterCureSeconds >= 0 &&
    Number.isFinite(waitTimeAfterLiftSeconds) &&
    waitTimeAfterLiftSeconds >= 0 &&
    Number.isFinite(lightPwm) &&
    Number.isInteger(lightPwm) &&
    lightPwm >= 1 &&
    lightPwm <= 255 &&
    Number.isFinite(bottomLightPwm) &&
    Number.isInteger(bottomLightPwm) &&
    bottomLightPwm >= 1 &&
    bottomLightPwm <= 255;

  if (!valid) return null;

  return {
    layerHeightMm,
    bottomLayerCount,
    transitionLayerCount,
    exposureTimeSeconds,
    bottomExposureTimeSeconds,
    bottomLiftHeightMm,
    bottomLiftSpeedMmPerMinute,
    liftHeightMm,
    liftSpeedMmPerMinute,
    retractSpeedMmPerMinute,
    bottomLightOffDelaySeconds,
    lightOffDelaySeconds,
    waitTimeBeforeCureSeconds,
    waitTimeAfterCureSeconds,
    waitTimeAfterLiftSeconds,
    lightPwm,
    bottomLightPwm,
  };
}

function printerCtbSettingsEqual(a: PrinterCtbSettings, b: PrinterCtbSettings): boolean {
  return (
    a.layerHeightMm === b.layerHeightMm &&
    a.bottomLayerCount === b.bottomLayerCount &&
    a.transitionLayerCount === b.transitionLayerCount &&
    a.exposureTimeSeconds === b.exposureTimeSeconds &&
    a.bottomExposureTimeSeconds === b.bottomExposureTimeSeconds &&
    a.bottomLiftHeightMm === b.bottomLiftHeightMm &&
    a.bottomLiftSpeedMmPerMinute === b.bottomLiftSpeedMmPerMinute &&
    a.liftHeightMm === b.liftHeightMm &&
    a.liftSpeedMmPerMinute === b.liftSpeedMmPerMinute &&
    a.retractSpeedMmPerMinute === b.retractSpeedMmPerMinute &&
    a.bottomLightOffDelaySeconds === b.bottomLightOffDelaySeconds &&
    a.lightOffDelaySeconds === b.lightOffDelaySeconds &&
    a.waitTimeBeforeCureSeconds === b.waitTimeBeforeCureSeconds &&
    a.waitTimeAfterCureSeconds === b.waitTimeAfterCureSeconds &&
    a.waitTimeAfterLiftSeconds === b.waitTimeAfterLiftSeconds &&
    a.lightPwm === b.lightPwm &&
    a.bottomLightPwm === b.bottomLightPwm
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
  const createAutoSupportSettingsPreviewMutation = useCreateAutoSupportSettingsPreview();
  const [previewIdByScenarioId, setPreviewIdByScenarioId] = useState<Record<string, string>>({});
  const [previewScenarios, setPreviewScenarios] = useState<AutoSupportSettingsPreviewScenario[]>(
    DEFAULT_AUTO_SUPPORT_PREVIEW_SCENARIOS,
  );
  const [activePreviewScenarioId, setActivePreviewScenarioId] = useState(
    DEFAULT_AUTO_SUPPORT_PREVIEW_SCENARIOS[0].scenarioId,
  );
  const [showPreviewForceMarkers, setShowPreviewForceMarkers] = useState(true);
  const [defaultRaftHeightMm, setDefaultRaftHeightMm] = useState('');
  const [theme, setTheme] = useState<string>('nord');
  const [generatePreviewsEnabled, setGeneratePreviewsEnabled] = useState(true);
  const [minimumPreviewGenerationVersion, setMinimumPreviewGenerationVersion] = useState('0');
  const [tagGenerationEnabled, setTagGenerationEnabled] = useState(false);
  const [aiDescriptionEnabled, setAiDescriptionEnabled] = useState(false);
  const [tagGenerationProvider, setTagGenerationProvider] = useState<'internal' | 'ollama'>(
    'internal',
  );
  const [tagGenerationEndpoint, setTagGenerationEndpoint] = useState('http://localhost:11434');
  const [tagGenerationModelOverride, setTagGenerationModelOverride] = useState('');
  const [tagGenerationTimeoutMs, setTagGenerationTimeoutMs] = useState('60000');
  const [tagGenerationMaxTags, setTagGenerationMaxTags] = useState('12');
  const [tagGenerationMinConfidence, setTagGenerationMinConfidence] = useState('0.45');
  const [tagGenerationPromptOverride, setTagGenerationPromptOverride] = useState('');
  const [descriptionGenerationPromptOverride, setDescriptionGenerationPromptOverride] =
    useState('');
  const [autoSupportBedMarginMm, setAutoSupportBedMarginMm] = useState('2');
  const [autoSupportMinVoxelSizeMm, setAutoSupportMinVoxelSizeMm] = useState('0.8');
  const [autoSupportMaxVoxelSizeMm, setAutoSupportMaxVoxelSizeMm] = useState('2');
  const [autoSupportMinLayerHeightMm, setAutoSupportMinLayerHeightMm] = useState('0.75');
  const [autoSupportMaxLayerHeightMm, setAutoSupportMaxLayerHeightMm] = useState('1.5');
  const [autoSupportMergeDistanceMm, setAutoSupportMergeDistanceMm] = useState('2.5');
  const [autoSupportMinIslandAreaMm2, setAutoSupportMinIslandAreaMm2] = useState('4');
  const [autoSupportMaxSupportDistanceMm, setAutoSupportMaxSupportDistanceMm] = useState('10');
  const [
    autoSupportUnsupportedIslandVolumeThresholdMm3,
    setAutoSupportUnsupportedIslandVolumeThresholdMm3,
  ] = useState('1');
  const [autoSupportPullForceThreshold, setAutoSupportPullForceThreshold] = useState('3');
  const [autoSupportSphereRadiusMm, setAutoSupportSphereRadiusMm] = useState('1.2');
  const [autoSupportResinStrength, setAutoSupportResinStrength] = useState('1');
  const [autoSupportCrushForceThreshold, setAutoSupportCrushForceThreshold] = useState('20');
  const [autoSupportMaxAngularForce, setAutoSupportMaxAngularForce] = useState('40');
  const [autoSupportResinDensityGPerMl, setAutoSupportResinDensityGPerMl] = useState('1.25');
  const [autoSupportPeelForceMultiplier, setAutoSupportPeelForceMultiplier] = useState('0.15');
  const [autoSupportMicroTipRadiusMm, setAutoSupportMicroTipRadiusMm] = useState('0.4');
  const [autoSupportLightTipRadiusMm, setAutoSupportLightTipRadiusMm] = useState('0.7');
  const [autoSupportMediumTipRadiusMm, setAutoSupportMediumTipRadiusMm] = useState('1');
  const [autoSupportHeavyTipRadiusMm, setAutoSupportHeavyTipRadiusMm] = useState('1.5');
  const [autoSupportSuctionMultiplier, setAutoSupportSuctionMultiplier] = useState('3');
  const [autoSupportAreaGrowthThreshold, setAutoSupportAreaGrowthThreshold] = useState('0.5');
  const [autoSupportAreaGrowthMultiplier, setAutoSupportAreaGrowthMultiplier] = useState('1.5');
  const [autoSupportGravityEnabled, setAutoSupportGravityEnabled] = useState(true);
  const [autoSupportDragCoefficientMultiplier, setAutoSupportDragCoefficientMultiplier] =
    useState('0.5');
  const [autoSupportMinFeatureWidthMm, setAutoSupportMinFeatureWidthMm] = useState('1');
  const [autoSupportShrinkagePercent, setAutoSupportShrinkagePercent] = useState('5');
  const [autoSupportShrinkageEdgeBias, setAutoSupportShrinkageEdgeBias] = useState('0.7');
  const [autoSupportV2VoxelSizeMm, setAutoSupportV2VoxelSizeMm] = useState('2');
  const [autoSupportV2OptimizationEnabled, setAutoSupportV2OptimizationEnabled] = useState(true);
  const [autoSupportV2CoarseVoxelSizeMm, setAutoSupportV2CoarseVoxelSizeMm] = useState('4');
  const [autoSupportV2FineVoxelSizeMm, setAutoSupportV2FineVoxelSizeMm] = useState('0.5');
  const [autoSupportV2RefinementMarginMm, setAutoSupportV2RefinementMarginMm] = useState('2');
  const [autoSupportV2RefinementMaxRegions, setAutoSupportV2RefinementMaxRegions] = useState('12');
  const [autoSupportV2RiskForceMarginRatio, setAutoSupportV2RiskForceMarginRatio] = useState('0.2');
  const [autoSupportV2MinRegionVolumeMm3, setAutoSupportV2MinRegionVolumeMm3] = useState('8');
  const [newPrinterName, setNewPrinterName] = useState('');
  const [newPrinterWidthMm, setNewPrinterWidthMm] = useState('228');
  const [newPrinterDepthMm, setNewPrinterDepthMm] = useState('128');
  const [newPrinterPixelWidth, setNewPrinterPixelWidth] = useState('7680');
  const [newPrinterPixelHeight, setNewPrinterPixelHeight] = useState('4320');
  const [newPrinterCtbSettings, setNewPrinterCtbSettings] = useState<PrinterCtbSettingsDraft>(
    DEFAULT_PRINTER_CTB_SETTINGS_DRAFT,
  );
  const [printerEdits, setPrinterEdits] = useState<
    Record<
      string,
      {
        name: string;
        bedWidthMm: string;
        bedDepthMm: string;
        pixelWidth: string;
        pixelHeight: string;
        ctbSettings: PrinterCtbSettingsDraft;
      }
    >
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
      setTagGenerationEnabled(appConfig.tagGenerationEnabled);
      setAiDescriptionEnabled(appConfig.aiDescriptionEnabled);
      setTagGenerationProvider(
        appConfig.tagGenerationProvider === 'ollama' ? 'ollama' : 'internal',
      );
      setTagGenerationEndpoint(appConfig.tagGenerationEndpoint);
      setTagGenerationModelOverride(appConfig.tagGenerationModelOverride);
      setTagGenerationTimeoutMs(String(appConfig.tagGenerationTimeoutMs));
      setTagGenerationMaxTags(String(appConfig.tagGenerationMaxTags));
      setTagGenerationMinConfidence(String(appConfig.tagGenerationMinConfidence));
      setTagGenerationPromptOverride(appConfig.tagGenerationPromptTemplateOverride);
      setDescriptionGenerationPromptOverride(appConfig.descriptionGenerationPromptTemplateOverride);
      setAutoSupportBedMarginMm(String(appConfig.autoSupportBedMarginMm));
      setAutoSupportMinVoxelSizeMm(String(appConfig.autoSupportMinVoxelSizeMm));
      setAutoSupportMaxVoxelSizeMm(String(appConfig.autoSupportMaxVoxelSizeMm));
      setAutoSupportMinLayerHeightMm(String(appConfig.autoSupportMinLayerHeightMm));
      setAutoSupportMaxLayerHeightMm(String(appConfig.autoSupportMaxLayerHeightMm));
      setAutoSupportMergeDistanceMm(String(appConfig.autoSupportMergeDistanceMm));
      setAutoSupportMinIslandAreaMm2(String(appConfig.autoSupportMinIslandAreaMm2));
      setAutoSupportMaxSupportDistanceMm(String(appConfig.autoSupportMaxSupportDistanceMm));
      setAutoSupportUnsupportedIslandVolumeThresholdMm3(
        String(appConfig.autoSupportUnsupportedIslandVolumeThresholdMm3),
      );
      setAutoSupportPullForceThreshold(String(appConfig.autoSupportPullForceThreshold));
      setAutoSupportSphereRadiusMm(String(appConfig.autoSupportSphereRadiusMm));
      setAutoSupportResinStrength(String(appConfig.autoSupportResinStrength));
      setAutoSupportCrushForceThreshold(String(appConfig.autoSupportCrushForceThreshold));
      setAutoSupportMaxAngularForce(String(appConfig.autoSupportMaxAngularForce));
      setAutoSupportResinDensityGPerMl(String(appConfig.autoSupportResinDensityGPerMl));
      setAutoSupportPeelForceMultiplier(String(appConfig.autoSupportPeelForceMultiplier));
      setAutoSupportMicroTipRadiusMm(String(appConfig.autoSupportMicroTipRadiusMm));
      setAutoSupportLightTipRadiusMm(String(appConfig.autoSupportLightTipRadiusMm));
      setAutoSupportMediumTipRadiusMm(String(appConfig.autoSupportMediumTipRadiusMm));
      setAutoSupportHeavyTipRadiusMm(String(appConfig.autoSupportHeavyTipRadiusMm));
      setAutoSupportSuctionMultiplier(String(appConfig.autoSupportSuctionMultiplier));
      setAutoSupportAreaGrowthThreshold(String(appConfig.autoSupportAreaGrowthThreshold));
      setAutoSupportAreaGrowthMultiplier(String(appConfig.autoSupportAreaGrowthMultiplier));
      setAutoSupportGravityEnabled(appConfig.autoSupportGravityEnabled);
      setAutoSupportDragCoefficientMultiplier(
        String(appConfig.autoSupportDragCoefficientMultiplier),
      );
      setAutoSupportMinFeatureWidthMm(String(appConfig.autoSupportMinFeatureWidthMm));
      setAutoSupportShrinkagePercent(String(appConfig.autoSupportShrinkagePercent));
      setAutoSupportShrinkageEdgeBias(String(appConfig.autoSupportShrinkageEdgeBias));
      setAutoSupportV2VoxelSizeMm(String(appConfig.autoSupportV2VoxelSizeMm));
      setAutoSupportV2OptimizationEnabled(appConfig.autoSupportV2OptimizationEnabled);
      setAutoSupportV2CoarseVoxelSizeMm(String(appConfig.autoSupportV2CoarseVoxelSizeMm));
      setAutoSupportV2FineVoxelSizeMm(String(appConfig.autoSupportV2FineVoxelSizeMm));
      setAutoSupportV2RefinementMarginMm(String(appConfig.autoSupportV2RefinementMarginMm));
      setAutoSupportV2RefinementMaxRegions(String(appConfig.autoSupportV2RefinementMaxRegions));
      setAutoSupportV2RiskForceMarginRatio(String(appConfig.autoSupportV2RiskForceMarginRatio));
      setAutoSupportV2MinRegionVolumeMm3(String(appConfig.autoSupportV2MinRegionVolumeMm3));
    }
  }, [appConfig]);

  useEffect(() => {
    setPrinterEdits((current) => {
      const next: Record<
        string,
        {
          name: string;
          bedWidthMm: string;
          bedDepthMm: string;
          pixelWidth: string;
          pixelHeight: string;
          ctbSettings: PrinterCtbSettingsDraft;
        }
      > = {};
      for (const printer of printers) {
        next[printer.id] = current[printer.id] ?? {
          name: printer.name,
          bedWidthMm: String(printer.bedWidthMm),
          bedDepthMm: String(printer.bedDepthMm),
          pixelWidth: String(printer.pixelWidth),
          pixelHeight: String(printer.pixelHeight),
          ctbSettings: toPrinterCtbSettingsDraft(printer),
        };
      }
      return next;
    });
  }, [printers]);

  const raftHeightValue = Number(defaultRaftHeightMm);
  const minimumPreviewGenerationVersionValue = Number(minimumPreviewGenerationVersion);
  const timeoutValue = Number(tagGenerationTimeoutMs);
  const maxTagsValue = Number(tagGenerationMaxTags);
  const minConfidenceValue = Number(tagGenerationMinConfidence);
  const autoSupportBedMarginValue = Number(autoSupportBedMarginMm);
  const autoSupportMinVoxelSizeValue = Number(autoSupportMinVoxelSizeMm);
  const autoSupportMaxVoxelSizeValue = Number(autoSupportMaxVoxelSizeMm);
  const autoSupportMinLayerHeightValue = Number(autoSupportMinLayerHeightMm);
  const autoSupportMaxLayerHeightValue = Number(autoSupportMaxLayerHeightMm);
  const autoSupportMergeDistanceValue = Number(autoSupportMergeDistanceMm);
  const autoSupportMinIslandAreaValue = Number(autoSupportMinIslandAreaMm2);
  const autoSupportMaxSupportDistanceValue = Number(autoSupportMaxSupportDistanceMm);
  const autoSupportUnsupportedIslandVolumeThresholdValue = Number(
    autoSupportUnsupportedIslandVolumeThresholdMm3,
  );
  const autoSupportPullForceThresholdValue = Number(autoSupportPullForceThreshold);
  const autoSupportSphereRadiusValue = Number(autoSupportSphereRadiusMm);
  const autoSupportResinStrengthValue = Number(autoSupportResinStrength);
  const autoSupportCrushForceThresholdValue = Number(autoSupportCrushForceThreshold);
  const autoSupportMaxAngularForceValue = Number(autoSupportMaxAngularForce);
  const autoSupportResinDensityValue = Number(autoSupportResinDensityGPerMl);
  const autoSupportPeelForceMultiplierValue = Number(autoSupportPeelForceMultiplier);
  const autoSupportMicroTipRadiusValue = Number(autoSupportMicroTipRadiusMm);
  const autoSupportLightTipRadiusValue = Number(autoSupportLightTipRadiusMm);
  const autoSupportMediumTipRadiusValue = Number(autoSupportMediumTipRadiusMm);
  const autoSupportHeavyTipRadiusValue = Number(autoSupportHeavyTipRadiusMm);
  const autoSupportSuctionMultiplierValue = Number(autoSupportSuctionMultiplier);
  const autoSupportAreaGrowthThresholdValue = Number(autoSupportAreaGrowthThreshold);
  const autoSupportAreaGrowthMultiplierValue = Number(autoSupportAreaGrowthMultiplier);
  const autoSupportDragCoefficientMultiplierValue = Number(autoSupportDragCoefficientMultiplier);
  const autoSupportMinFeatureWidthValue = Number(autoSupportMinFeatureWidthMm);
  const autoSupportShrinkagePercentValue = Number(autoSupportShrinkagePercent);
  const autoSupportShrinkageEdgeBiasValue = Number(autoSupportShrinkageEdgeBias);
  const autoSupportV2VoxelSizeValue = Number(autoSupportV2VoxelSizeMm);
  const autoSupportV2CoarseVoxelSizeValue = Number(autoSupportV2CoarseVoxelSizeMm);
  const autoSupportV2FineVoxelSizeValue = Number(autoSupportV2FineVoxelSizeMm);
  const autoSupportV2RefinementMarginValue = Number(autoSupportV2RefinementMarginMm);
  const autoSupportV2RefinementMaxRegionsValue = Number(autoSupportV2RefinementMaxRegions);
  const autoSupportV2RiskForceMarginRatioValue = Number(autoSupportV2RiskForceMarginRatio);
  const autoSupportV2MinRegionVolumeMm3Value = Number(autoSupportV2MinRegionVolumeMm3);
  const previewGenerationVersionLimit = instanceStats?.previewGenerationVersion;

  const autoSupportConfigValid =
    Number.isFinite(autoSupportBedMarginValue) &&
    autoSupportBedMarginValue >= 0 &&
    autoSupportBedMarginValue <= 20 &&
    Number.isFinite(autoSupportMinVoxelSizeValue) &&
    Number.isFinite(autoSupportMaxVoxelSizeValue) &&
    autoSupportMinVoxelSizeValue >= 0.1 &&
    autoSupportMaxVoxelSizeValue >= autoSupportMinVoxelSizeValue &&
    Number.isFinite(autoSupportMinLayerHeightValue) &&
    Number.isFinite(autoSupportMaxLayerHeightValue) &&
    autoSupportMinLayerHeightValue >= 0.05 &&
    autoSupportMaxLayerHeightValue >= autoSupportMinLayerHeightValue &&
    Number.isFinite(autoSupportMergeDistanceValue) &&
    autoSupportMergeDistanceValue >= 0.1 &&
    Number.isFinite(autoSupportMinIslandAreaValue) &&
    autoSupportMinIslandAreaValue >= 0 &&
    Number.isFinite(autoSupportResinStrengthValue) &&
    autoSupportResinStrengthValue >= 0.1 &&
    Number.isFinite(autoSupportCrushForceThresholdValue) &&
    autoSupportCrushForceThresholdValue >= 0.1 &&
    Number.isFinite(autoSupportMaxAngularForceValue) &&
    autoSupportMaxAngularForceValue >= 0.1 &&
    Number.isFinite(autoSupportPeelForceMultiplierValue) &&
    autoSupportPeelForceMultiplierValue > 0 &&
    Number.isFinite(autoSupportLightTipRadiusValue) &&
    autoSupportLightTipRadiusValue >= 0.1 &&
    autoSupportLightTipRadiusValue <= 5 &&
    Number.isFinite(autoSupportMediumTipRadiusValue) &&
    autoSupportMediumTipRadiusValue >= 0.1 &&
    autoSupportMediumTipRadiusValue <= 7 &&
    Number.isFinite(autoSupportHeavyTipRadiusValue) &&
    autoSupportHeavyTipRadiusValue >= 0.1 &&
    autoSupportHeavyTipRadiusValue <= 10 &&
    Number.isFinite(autoSupportSuctionMultiplierValue) &&
    autoSupportSuctionMultiplierValue >= 1 &&
    autoSupportSuctionMultiplierValue <= 10 &&
    Number.isFinite(autoSupportAreaGrowthThresholdValue) &&
    autoSupportAreaGrowthThresholdValue >= 0.1 &&
    autoSupportAreaGrowthThresholdValue <= 5 &&
    Number.isFinite(autoSupportAreaGrowthMultiplierValue) &&
    autoSupportAreaGrowthMultiplierValue >= 1 &&
    autoSupportAreaGrowthMultiplierValue <= 5 &&
    Number.isFinite(autoSupportDragCoefficientMultiplierValue) &&
    autoSupportDragCoefficientMultiplierValue >= 0 &&
    autoSupportDragCoefficientMultiplierValue <= 5 &&
    Number.isFinite(autoSupportMinFeatureWidthValue) &&
    autoSupportMinFeatureWidthValue >= 0.1 &&
    autoSupportMinFeatureWidthValue <= 10 &&
    Number.isFinite(autoSupportShrinkagePercentValue) &&
    autoSupportShrinkagePercentValue >= 0 &&
    autoSupportShrinkagePercentValue <= 15 &&
    Number.isFinite(autoSupportShrinkageEdgeBiasValue) &&
    autoSupportShrinkageEdgeBiasValue >= 0 &&
    autoSupportShrinkageEdgeBiasValue <= 1;

  const appConfigValid =
    defaultRaftHeightMm.trim().length > 0 &&
    Number.isFinite(raftHeightValue) &&
    raftHeightValue >= 0 &&
    Number.isInteger(minimumPreviewGenerationVersionValue) &&
    minimumPreviewGenerationVersionValue >= 0 &&
    (previewGenerationVersionLimit === undefined ||
      minimumPreviewGenerationVersionValue <= previewGenerationVersionLimit) &&
    tagGenerationEndpoint.trim().length > 0 &&
    Number.isInteger(timeoutValue) &&
    timeoutValue >= 1000 &&
    timeoutValue <= 300000 &&
    Number.isInteger(maxTagsValue) &&
    maxTagsValue >= 1 &&
    maxTagsValue <= 64 &&
    Number.isFinite(minConfidenceValue) &&
    minConfidenceValue >= 0 &&
    minConfidenceValue <= 1 &&
    autoSupportConfigValid;

  const currentSection: SettingsSectionKey = useMemo(() => {
    if (location.pathname.startsWith('/settings/autosupport')) return 'autosupport';
    if (location.pathname.startsWith('/settings/printers')) return 'printers';
    if (location.pathname.startsWith('/settings/logs')) return 'logs';
    if (location.pathname.startsWith('/settings/ai')) return 'ai';
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
      tagGenerationEnabled,
      aiDescriptionEnabled,
      tagGenerationProvider,
      tagGenerationEndpoint: tagGenerationEndpoint.trim(),
      tagGenerationModel: tagGenerationModelOverride.trim(),
      tagGenerationTimeoutMs: timeoutValue,
      tagGenerationMaxTags: maxTagsValue,
      tagGenerationMinConfidence: minConfidenceValue,
      tagGenerationPromptTemplate: tagGenerationPromptOverride.trim(),
      descriptionGenerationPromptTemplate: descriptionGenerationPromptOverride.trim(),
      autoSupportBedMarginMm: autoSupportBedMarginValue,
      autoSupportMinVoxelSizeMm: autoSupportMinVoxelSizeValue,
      autoSupportMaxVoxelSizeMm: autoSupportMaxVoxelSizeValue,
      autoSupportMinLayerHeightMm: autoSupportMinLayerHeightValue,
      autoSupportMaxLayerHeightMm: autoSupportMaxLayerHeightValue,
      autoSupportMergeDistanceMm: autoSupportMergeDistanceValue,
      autoSupportMinIslandAreaMm2: autoSupportMinIslandAreaValue,
      autoSupportMaxSupportDistanceMm: autoSupportMaxSupportDistanceValue,
      autoSupportUnsupportedIslandVolumeThresholdMm3:
        autoSupportUnsupportedIslandVolumeThresholdValue,
      autoSupportPullForceThreshold: autoSupportPullForceThresholdValue,
      autoSupportSphereRadiusMm: autoSupportSphereRadiusValue,
      autoSupportMaxSupportsPerIsland: appConfig?.autoSupportMaxSupportsPerIsland ?? 6,
      autoSupportResinStrength: autoSupportResinStrengthValue,
      autoSupportCrushForceThreshold: autoSupportCrushForceThresholdValue,
      autoSupportMaxAngularForce: autoSupportMaxAngularForceValue,
      autoSupportResinDensityGPerMl: autoSupportResinDensityValue,
      autoSupportPeelForceMultiplier: autoSupportPeelForceMultiplierValue,
      autoSupportMicroTipRadiusMm: autoSupportMicroTipRadiusValue,
      autoSupportLightTipRadiusMm: autoSupportLightTipRadiusValue,
      autoSupportMediumTipRadiusMm: autoSupportMediumTipRadiusValue,
      autoSupportHeavyTipRadiusMm: autoSupportHeavyTipRadiusValue,
      autoSupportSuctionMultiplier: autoSupportSuctionMultiplierValue,
      autoSupportAreaGrowthThreshold: autoSupportAreaGrowthThresholdValue,
      autoSupportAreaGrowthMultiplier: autoSupportAreaGrowthMultiplierValue,
      autoSupportGravityEnabled: autoSupportGravityEnabled,
      autoSupportDragCoefficientMultiplier: autoSupportDragCoefficientMultiplierValue,
      autoSupportMinFeatureWidthMm: autoSupportMinFeatureWidthValue,
      autoSupportShrinkagePercent: autoSupportShrinkagePercentValue,
      autoSupportShrinkageEdgeBias: autoSupportShrinkageEdgeBiasValue,
      autoSupportV2VoxelSizeMm: autoSupportV2VoxelSizeValue,
      autoSupportV2OptimizationEnabled: autoSupportV2OptimizationEnabled,
      autoSupportV2CoarseVoxelSizeMm: autoSupportV2CoarseVoxelSizeValue,
      autoSupportV2FineVoxelSizeMm: autoSupportV2FineVoxelSizeValue,
      autoSupportV2RefinementMarginMm: autoSupportV2RefinementMarginValue,
      autoSupportV2RefinementMaxRegions: autoSupportV2RefinementMaxRegionsValue,
      autoSupportV2RiskForceMarginRatio: autoSupportV2RiskForceMarginRatioValue,
      autoSupportV2MinRegionVolumeMm3: autoSupportV2MinRegionVolumeMm3Value,
    });

  const generateAutoSupportPreview = () =>
    createAutoSupportSettingsPreviewMutation.mutate(
      {
        tuning: {
          bedMarginMm: autoSupportBedMarginValue,
          minVoxelSizeMm: autoSupportMinVoxelSizeValue,
          maxVoxelSizeMm: autoSupportMaxVoxelSizeValue,
          minLayerHeightMm: autoSupportMinLayerHeightValue,
          maxLayerHeightMm: autoSupportMaxLayerHeightValue,
          mergeDistanceMm: autoSupportMergeDistanceValue,
          minIslandAreaMm2: autoSupportMinIslandAreaValue,
          resinStrength: autoSupportResinStrengthValue,
          crushForceThreshold: autoSupportCrushForceThresholdValue,
          maxAngularForce: autoSupportMaxAngularForceValue,
          peelForceMultiplier: autoSupportPeelForceMultiplierValue,
          lightTipRadiusMm: autoSupportLightTipRadiusValue,
          mediumTipRadiusMm: autoSupportMediumTipRadiusValue,
          heavyTipRadiusMm: autoSupportHeavyTipRadiusValue,
          suctionMultiplier: autoSupportSuctionMultiplierValue,
          areaGrowthThreshold: autoSupportAreaGrowthThresholdValue,
          areaGrowthMultiplier: autoSupportAreaGrowthMultiplierValue,
          gravityEnabled: autoSupportGravityEnabled,
          resinDensityGPerMl: autoSupportResinDensityValue,
          dragCoefficientMultiplier: autoSupportDragCoefficientMultiplierValue,
          minFeatureWidthMm: autoSupportMinFeatureWidthValue,
          shrinkagePercent: autoSupportShrinkagePercentValue,
          shrinkageEdgeBias: autoSupportShrinkageEdgeBiasValue,
        },
        scenarioId: activePreviewScenarioId,
      },
      {
        onSuccess: (result) => {
          setPreviewScenarios((current) => {
            const byId = new Map(current.map((scenario) => [scenario.scenarioId, scenario]));
            for (const scenario of result.scenarios) {
              const existing = byId.get(scenario.scenarioId);
              const shouldPreserveExisting =
                scenario.status === 'not-generated' &&
                existing !== undefined &&
                existing.status !== 'not-generated';

              if (!shouldPreserveExisting) {
                byId.set(scenario.scenarioId, scenario);
              }
            }

            return [...byId.values()].sort((a, b) => {
              const left = AUTO_SUPPORT_PREVIEW_ORDER.get(a.scenarioId) ?? Number.MAX_SAFE_INTEGER;
              const right = AUTO_SUPPORT_PREVIEW_ORDER.get(b.scenarioId) ?? Number.MAX_SAFE_INTEGER;
              return left - right;
            });
          });

          const generatedScenarioIds = result.scenarios
            .filter((scenario) => scenario.status === 'completed')
            .map((scenario) => scenario.scenarioId);

          if (generatedScenarioIds.length > 0) {
            setPreviewIdByScenarioId((current) => {
              const next = { ...current };
              for (const scenarioId of generatedScenarioIds) {
                next[scenarioId] = result.previewId;
              }
              return next;
            });
          }
        },
      },
    );

  const activePreviewScenario =
    previewScenarios.find((scenario) => scenario.scenarioId === activePreviewScenarioId) ??
    previewScenarios[0] ??
    null;

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

          {currentSection === 'autosupport' && (
            <Stack className={styles.globalSettingsSection}>
              <Stack spacing={2}>
                <Typography variant="h6">Auto support preview tuning</Typography>
                <Typography color="text.secondary">
                  Change settings, then regenerate previews to compare support behavior across test
                  models and optional hardcoded STLs.
                </Typography>
                <Stack
                  direction={{ xs: 'column', md: 'row' }}
                  spacing={1}
                  className={styles.addRow}
                >
                  <TextField
                    size="small"
                    type="number"
                    label="Bed margin (mm)"
                    value={autoSupportBedMarginMm}
                    onChange={(e) => setAutoSupportBedMarginMm(e.target.value)}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Min voxel size (mm)"
                    value={autoSupportMinVoxelSizeMm}
                    onChange={(e) => setAutoSupportMinVoxelSizeMm(e.target.value)}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Max voxel size (mm)"
                    value={autoSupportMaxVoxelSizeMm}
                    onChange={(e) => setAutoSupportMaxVoxelSizeMm(e.target.value)}
                  />
                </Stack>
                <Stack
                  direction={{ xs: 'column', md: 'row' }}
                  spacing={1}
                  className={styles.addRow}
                >
                  <TextField
                    size="small"
                    type="number"
                    label="Min layer height (mm)"
                    value={autoSupportMinLayerHeightMm}
                    onChange={(e) => setAutoSupportMinLayerHeightMm(e.target.value)}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Max layer height (mm)"
                    value={autoSupportMaxLayerHeightMm}
                    onChange={(e) => setAutoSupportMaxLayerHeightMm(e.target.value)}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Merge supports threshold (mm)"
                    value={autoSupportMergeDistanceMm}
                    onChange={(e) => setAutoSupportMergeDistanceMm(e.target.value)}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Min island area (mm²)"
                    value={autoSupportMinIslandAreaMm2}
                    onChange={(e) => setAutoSupportMinIslandAreaMm2(e.target.value)}
                  />
                </Stack>
                <Typography variant="h6">Support tip sizing</Typography>
                <Typography color="text.secondary">
                  Tip radii, resin strength, crush force, and angular force control support capacity
                  and stability checks.
                </Typography>
                <Stack
                  direction={{ xs: 'column', md: 'row' }}
                  spacing={1}
                  className={styles.addRow}
                >
                  <TextField
                    size="small"
                    type="number"
                    label="Resin strength"
                    value={autoSupportResinStrength}
                    onChange={(e) => setAutoSupportResinStrength(e.target.value)}
                    helperText="Dimensionless multiplier (default 1.0)"
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Crush force threshold"
                    value={autoSupportCrushForceThreshold}
                    onChange={(e) => setAutoSupportCrushForceThreshold(e.target.value)}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Max angular force"
                    value={autoSupportMaxAngularForce}
                    onChange={(e) => setAutoSupportMaxAngularForce(e.target.value)}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Peel force multiplier"
                    value={autoSupportPeelForceMultiplier}
                    onChange={(e) => setAutoSupportPeelForceMultiplier(e.target.value)}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Light tip radius (mm)"
                    value={autoSupportLightTipRadiusMm}
                    onChange={(e) => setAutoSupportLightTipRadiusMm(e.target.value)}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Medium tip radius (mm)"
                    value={autoSupportMediumTipRadiusMm}
                    onChange={(e) => setAutoSupportMediumTipRadiusMm(e.target.value)}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Heavy tip radius (mm)"
                    value={autoSupportHeavyTipRadiusMm}
                    onChange={(e) => setAutoSupportHeavyTipRadiusMm(e.target.value)}
                  />
                </Stack>
                <Typography variant="h6">Advanced force models</Typography>
                <Typography color="text.secondary">
                  Additional physical forces for more accurate support placement: suction, area
                  growth, gravity, hydrodynamic drag, and thermal shrinkage.
                </Typography>
                <Stack
                  direction={{ xs: 'column', md: 'row' }}
                  spacing={1}
                  className={styles.addRow}
                >
                  <TextField
                    size="small"
                    type="number"
                    label="Suction multiplier"
                    value={autoSupportSuctionMultiplier}
                    onChange={(e) => setAutoSupportSuctionMultiplier(e.target.value)}
                    helperText="Force multiplier for enclosed regions (1-10, default 3)"
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Area growth threshold"
                    value={autoSupportAreaGrowthThreshold}
                    onChange={(e) => setAutoSupportAreaGrowthThreshold(e.target.value)}
                    helperText="Layer area increase ratio to trigger (0.1-5, default 0.5)"
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Area growth multiplier"
                    value={autoSupportAreaGrowthMultiplier}
                    onChange={(e) => setAutoSupportAreaGrowthMultiplier(e.target.value)}
                    helperText="Force multiplier when growth exceeds threshold (1-5, default 1.5)"
                  />
                </Stack>
                <Stack
                  direction={{ xs: 'column', md: 'row' }}
                  spacing={1}
                  className={styles.addRow}
                >
                  <FormControlLabel
                    control={
                      <Switch
                        checked={autoSupportGravityEnabled}
                        onChange={(event) => setAutoSupportGravityEnabled(event.target.checked)}
                      />
                    }
                    label="Gravity loading enabled"
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Drag coefficient multiplier"
                    value={autoSupportDragCoefficientMultiplier}
                    onChange={(e) => setAutoSupportDragCoefficientMultiplier(e.target.value)}
                    helperText="Lateral drag on thin features (0-5, default 0.5)"
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Min feature width (mm)"
                    value={autoSupportMinFeatureWidthMm}
                    onChange={(e) => setAutoSupportMinFeatureWidthMm(e.target.value)}
                    helperText="Features narrower than this get drag force (0.1-10, default 1)"
                  />
                </Stack>
                <Stack
                  direction={{ xs: 'column', md: 'row' }}
                  spacing={1}
                  className={styles.addRow}
                >
                  <TextField
                    size="small"
                    type="number"
                    label="Shrinkage percent"
                    value={autoSupportShrinkagePercent}
                    onChange={(e) => setAutoSupportShrinkagePercent(e.target.value)}
                    helperText="Resin volumetric shrinkage (0-15%, default 5)"
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Shrinkage edge bias"
                    value={autoSupportShrinkageEdgeBias}
                    onChange={(e) => setAutoSupportShrinkageEdgeBias(e.target.value)}
                    helperText="How much to bias edge support placement (0-1, default 0.7)"
                  />
                </Stack>
                <Stack direction={{ xs: 'column', sm: 'row' }} spacing={1}>
                  <Button
                    variant="contained"
                    disabled={updateAppConfigMutation.isPending || !appConfigValid}
                    onClick={saveConfig}
                  >
                    Save
                  </Button>
                  <Button
                    variant="outlined"
                    disabled={
                      createAutoSupportSettingsPreviewMutation.isPending || !autoSupportConfigValid
                    }
                    onClick={generateAutoSupportPreview}
                  >
                    {createAutoSupportSettingsPreviewMutation.isPending
                      ? 'Generating selected preview...'
                      : 'Regenerate selected support preview'}
                  </Button>
                  <FormControlLabel
                    control={
                      <Switch
                        checked={showPreviewForceMarkers}
                        onChange={(event) => setShowPreviewForceMarkers(event.target.checked)}
                      />
                    }
                    label="Show force markers"
                  />
                </Stack>

                {createAutoSupportSettingsPreviewMutation.isError && (
                  <Typography color="error">Failed to generate support preview.</Typography>
                )}

                {previewScenarios.length > 0 ? (
                  <Stack spacing={1}>
                    <Tabs
                      value={activePreviewScenarioId}
                      onChange={(_, value: string) => setActivePreviewScenarioId(value)}
                      variant="scrollable"
                      allowScrollButtonsMobile
                      className={styles.previewTabs}
                    >
                      {previewScenarios.map((scenario) => (
                        <Tab
                          key={scenario.scenarioId}
                          value={scenario.scenarioId}
                          label={scenario.name}
                        />
                      ))}
                    </Tabs>

                    {activePreviewScenario ? (
                      <AutoSupportPreviewViewport
                        previewId={previewIdByScenarioId[activePreviewScenario.scenarioId] ?? null}
                        scenario={activePreviewScenario}
                        showForceMarkers={showPreviewForceMarkers}
                      />
                    ) : (
                      <Typography color="text.secondary">
                        Select a preview tab to continue.
                      </Typography>
                    )}
                  </Stack>
                ) : (
                  <Typography color="text.secondary">
                    Regenerate selected previews to render scenarios.
                  </Typography>
                )}
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
                      pixelWidth: String(printer.pixelWidth),
                      pixelHeight: String(printer.pixelHeight),
                      ctbSettings: toPrinterCtbSettingsDraft(printer),
                    };
                    const width = Number(edit.bedWidthMm);
                    const depth = Number(edit.bedDepthMm);
                    const pixelWidth = Number(edit.pixelWidth);
                    const pixelHeight = Number(edit.pixelHeight);
                    const ctbSettings = parsePrinterCtbSettingsDraft(edit.ctbSettings);
                    const canSaveCustom =
                      !printer.isBuiltIn &&
                      edit.name.trim().length > 0 &&
                      Number.isFinite(width) &&
                      width > 0 &&
                      Number.isFinite(depth) &&
                      depth > 0 &&
                      Number.isFinite(pixelWidth) &&
                      pixelWidth > 0 &&
                      Number.isFinite(pixelHeight) &&
                      pixelHeight > 0 &&
                      ctbSettings !== null &&
                      (edit.name.trim() !== printer.name ||
                        width !== printer.bedWidthMm ||
                        depth !== printer.bedDepthMm ||
                        pixelWidth !== printer.pixelWidth ||
                        pixelHeight !== printer.pixelHeight ||
                        !printerCtbSettingsEqual(ctbSettings, printer));

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
                        <TextField
                          size="small"
                          type="number"
                          label="Resolution X (px)"
                          value={edit.pixelWidth}
                          disabled={printer.isBuiltIn}
                          onChange={(e) =>
                            setPrinterEdits((current) => ({
                              ...current,
                              [printer.id]: {
                                ...edit,
                                pixelWidth: e.target.value,
                              },
                            }))
                          }
                        />
                        <TextField
                          size="small"
                          type="number"
                          label="Resolution Y (px)"
                          value={edit.pixelHeight}
                          disabled={printer.isBuiltIn}
                          onChange={(e) =>
                            setPrinterEdits((current) => ({
                              ...current,
                              [printer.id]: {
                                ...edit,
                                pixelHeight: e.target.value,
                              },
                            }))
                          }
                        />
                        {PRINTER_CTB_FIELDS.map((field) => (
                          <TextField
                            key={`${printer.id}-${field.key}`}
                            size="small"
                            type="number"
                            label={field.label}
                            value={edit.ctbSettings[field.key]}
                            disabled={printer.isBuiltIn}
                            onChange={(e) =>
                              setPrinterEdits((current) => ({
                                ...current,
                                [printer.id]: {
                                  ...edit,
                                  ctbSettings: {
                                    ...edit.ctbSettings,
                                    [field.key]: e.target.value,
                                  },
                                },
                              }))
                            }
                            inputProps={{ min: field.min, step: field.step }}
                          />
                        ))}
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
                                  pixelWidth: Number(edit.pixelWidth),
                                  pixelHeight: Number(edit.pixelHeight),
                                  ctbSettings: ctbSettings!,
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
                  <TextField
                    size="small"
                    type="number"
                    label="Resolution X (px)"
                    value={newPrinterPixelWidth}
                    onChange={(e) => setNewPrinterPixelWidth(e.target.value)}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Resolution Y (px)"
                    value={newPrinterPixelHeight}
                    onChange={(e) => setNewPrinterPixelHeight(e.target.value)}
                  />
                  {PRINTER_CTB_FIELDS.map((field) => (
                    <TextField
                      key={`new-printer-${field.key}`}
                      size="small"
                      type="number"
                      label={field.label}
                      value={newPrinterCtbSettings[field.key]}
                      onChange={(e) =>
                        setNewPrinterCtbSettings((current) => ({
                          ...current,
                          [field.key]: e.target.value,
                        }))
                      }
                      inputProps={{ min: field.min, step: field.step }}
                    />
                  ))}
                  <Button
                    variant="contained"
                    disabled={
                      createPrinterMutation.isPending ||
                      !newPrinterName.trim() ||
                      Number(newPrinterWidthMm) <= 0 ||
                      Number(newPrinterDepthMm) <= 0 ||
                      Number(newPrinterPixelWidth) <= 0 ||
                      Number(newPrinterPixelHeight) <= 0 ||
                      parsePrinterCtbSettingsDraft(newPrinterCtbSettings) === null
                    }
                    onClick={() => {
                      const parsedNewPrinterCtbSettings =
                        parsePrinterCtbSettingsDraft(newPrinterCtbSettings);
                      if (!parsedNewPrinterCtbSettings) return;

                      createPrinterMutation.mutate(
                        {
                          name: newPrinterName.trim(),
                          bedWidthMm: Number(newPrinterWidthMm),
                          bedDepthMm: Number(newPrinterDepthMm),
                          pixelWidth: Number(newPrinterPixelWidth),
                          pixelHeight: Number(newPrinterPixelHeight),
                          ctbSettings: parsedNewPrinterCtbSettings,
                        },
                        {
                          onSuccess: () => {
                            setNewPrinterName('');
                            setNewPrinterCtbSettings(DEFAULT_PRINTER_CTB_SETTINGS_DRAFT);
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

          {currentSection === 'ai' && (
            <Stack className={styles.globalSettingsSection}>
              <Stack spacing={2}>
                <Stack direction="row" spacing={1}>
                  <FormControlLabel
                    control={
                      <Switch
                        checked={tagGenerationEnabled}
                        onChange={(e) => setTagGenerationEnabled(e.target.checked)}
                      />
                    }
                    label="Enable AI tag generation"
                  />
                </Stack>

                <Stack direction="row" spacing={1}>
                  <FormControlLabel
                    control={
                      <Switch
                        checked={aiDescriptionEnabled}
                        onChange={(e) => setAiDescriptionEnabled(e.target.checked)}
                      />
                    }
                    label="Enable AI description generation"
                  />
                </Stack>

                <TextField
                  select
                  size="small"
                  label="Provider"
                  value={tagGenerationProvider}
                  onChange={(e) =>
                    setTagGenerationProvider(e.target.value === 'ollama' ? 'ollama' : 'internal')
                  }
                >
                  <MenuItem value="internal">Internal</MenuItem>
                  <MenuItem value="ollama">Ollama</MenuItem>
                </TextField>

                {tagGenerationProvider === 'ollama' && (
                  <TextField
                    size="small"
                    label="Endpoint"
                    value={tagGenerationEndpoint}
                    onChange={(e) => setTagGenerationEndpoint(e.target.value)}
                    error={!tagGenerationEndpoint.trim()}
                    helperText={!tagGenerationEndpoint.trim() ? 'Endpoint is required.' : undefined}
                  />
                )}

                <Stack spacing={1}>
                  <TextField
                    size="small"
                    label="Model"
                    InputLabelProps={{ shrink: true }}
                    value={tagGenerationModelOverride}
                    placeholder={appConfig.tagGenerationModelDefault}
                    onChange={(e) => setTagGenerationModelOverride(e.target.value)}
                    helperText={
                      !tagGenerationModelOverride.trim()
                        ? 'Using the built-in model from backend defaults.'
                        : undefined
                    }
                  />
                  <Button variant="outlined" onClick={() => setTagGenerationModelOverride('')}>
                    Reset model to default
                  </Button>
                </Stack>

                <TextField
                  size="small"
                  type="number"
                  label="Timeout (ms)"
                  value={tagGenerationTimeoutMs}
                  onChange={(e) => setTagGenerationTimeoutMs(e.target.value)}
                  error={
                    !Number.isInteger(timeoutValue) || timeoutValue < 1000 || timeoutValue > 300000
                  }
                  helperText={
                    !Number.isInteger(timeoutValue) || timeoutValue < 1000 || timeoutValue > 300000
                      ? 'Must be an integer between 1000 and 300000.'
                      : undefined
                  }
                />

                <TextField
                  size="small"
                  type="number"
                  label="Max tags"
                  value={tagGenerationMaxTags}
                  onChange={(e) => setTagGenerationMaxTags(e.target.value)}
                  error={!Number.isInteger(maxTagsValue) || maxTagsValue < 1 || maxTagsValue > 64}
                  helperText={
                    !Number.isInteger(maxTagsValue) || maxTagsValue < 1 || maxTagsValue > 64
                      ? 'Must be an integer between 1 and 64.'
                      : undefined
                  }
                />

                <TextField
                  size="small"
                  type="number"
                  label="Min confidence"
                  value={tagGenerationMinConfidence}
                  onChange={(e) => setTagGenerationMinConfidence(e.target.value)}
                  error={
                    !Number.isFinite(minConfidenceValue) ||
                    minConfidenceValue < 0 ||
                    minConfidenceValue > 1
                  }
                  helperText={
                    !Number.isFinite(minConfidenceValue) ||
                    minConfidenceValue < 0 ||
                    minConfidenceValue > 1
                      ? 'Must be a number between 0 and 1.'
                      : undefined
                  }
                />

                <Stack spacing={1}>
                  <TextField
                    size="small"
                    label="Tag generation prompt template"
                    InputLabelProps={{ shrink: true }}
                    value={tagGenerationPromptOverride}
                    placeholder={appConfig.tagGenerationPromptTemplateDefault}
                    onChange={(e) => setTagGenerationPromptOverride(e.target.value)}
                    helperText={
                      !tagGenerationPromptOverride.trim()
                        ? 'Using the built-in prompt. Supports {{maxTags}} and {{allowedTags}} placeholders.'
                        : 'Supports {{maxTags}} and {{allowedTags}} placeholders.'
                    }
                    multiline
                    minRows={4}
                    fullWidth
                  />
                  <Button variant="outlined" onClick={() => setTagGenerationPromptOverride('')}>
                    Reset to default
                  </Button>
                </Stack>

                <Stack spacing={1}>
                  <TextField
                    size="small"
                    label="Description generation prompt template"
                    InputLabelProps={{ shrink: true }}
                    value={descriptionGenerationPromptOverride}
                    placeholder={appConfig.descriptionGenerationPromptTemplateDefault}
                    onChange={(e) => setDescriptionGenerationPromptOverride(e.target.value)}
                    helperText={
                      !descriptionGenerationPromptOverride.trim()
                        ? 'Using the built-in prompt. Supports {{modelName}} and {{fullPath}} placeholders.'
                        : 'Supports {{modelName}} and {{fullPath}} placeholders.'
                    }
                    multiline
                    minRows={4}
                    fullWidth
                  />
                  <Button
                    variant="outlined"
                    onClick={() => setDescriptionGenerationPromptOverride('')}
                  >
                    Reset to default
                  </Button>
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
