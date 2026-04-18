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
  { key: 'ai', label: 'AI Settings', path: '/settings/ai' },
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
  const [autoSupportMaxSupportsPerIsland, setAutoSupportMaxSupportsPerIsland] = useState('6');
  const [autoSupportResinStrength, setAutoSupportResinStrength] = useState('1');
  const [autoSupportResinDensityGPerMl, setAutoSupportResinDensityGPerMl] = useState('1.25');
  const [autoSupportPeelForceMultiplier, setAutoSupportPeelForceMultiplier] = useState('0.15');
  const [autoSupportMicroTipRadiusMm, setAutoSupportMicroTipRadiusMm] = useState('0.4');
  const [autoSupportLightTipRadiusMm, setAutoSupportLightTipRadiusMm] = useState('0.7');
  const [autoSupportMediumTipRadiusMm, setAutoSupportMediumTipRadiusMm] = useState('1');
  const [autoSupportHeavyTipRadiusMm, setAutoSupportHeavyTipRadiusMm] = useState('1.5');
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
  const [printerEdits, setPrinterEdits] = useState<
    Record<
      string,
      {
        name: string;
        bedWidthMm: string;
        bedDepthMm: string;
        pixelWidth: string;
        pixelHeight: string;
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
      setAutoSupportMaxSupportsPerIsland(String(appConfig.autoSupportMaxSupportsPerIsland));
      setAutoSupportResinStrength(String(appConfig.autoSupportResinStrength));
      setAutoSupportResinDensityGPerMl(String(appConfig.autoSupportResinDensityGPerMl));
      setAutoSupportPeelForceMultiplier(String(appConfig.autoSupportPeelForceMultiplier));
      setAutoSupportMicroTipRadiusMm(String(appConfig.autoSupportMicroTipRadiusMm));
      setAutoSupportLightTipRadiusMm(String(appConfig.autoSupportLightTipRadiusMm));
      setAutoSupportMediumTipRadiusMm(String(appConfig.autoSupportMediumTipRadiusMm));
      setAutoSupportHeavyTipRadiusMm(String(appConfig.autoSupportHeavyTipRadiusMm));
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
        }
      > = {};
      for (const printer of printers) {
        next[printer.id] = current[printer.id] ?? {
          name: printer.name,
          bedWidthMm: String(printer.bedWidthMm),
          bedDepthMm: String(printer.bedDepthMm),
          pixelWidth: String(printer.pixelWidth),
          pixelHeight: String(printer.pixelHeight),
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
  const autoSupportMaxSupportsPerIslandValue = Number(autoSupportMaxSupportsPerIsland);
  const autoSupportResinStrengthValue = Number(autoSupportResinStrength);
  const autoSupportResinDensityValue = Number(autoSupportResinDensityGPerMl);
  const autoSupportPeelForceMultiplierValue = Number(autoSupportPeelForceMultiplier);
  const autoSupportMicroTipRadiusValue = Number(autoSupportMicroTipRadiusMm);
  const autoSupportLightTipRadiusValue = Number(autoSupportLightTipRadiusMm);
  const autoSupportMediumTipRadiusValue = Number(autoSupportMediumTipRadiusMm);
  const autoSupportHeavyTipRadiusValue = Number(autoSupportHeavyTipRadiusMm);
  const autoSupportV2VoxelSizeValue = Number(autoSupportV2VoxelSizeMm);
  const autoSupportV2CoarseVoxelSizeValue = Number(autoSupportV2CoarseVoxelSizeMm);
  const autoSupportV2FineVoxelSizeValue = Number(autoSupportV2FineVoxelSizeMm);
  const autoSupportV2RefinementMarginValue = Number(autoSupportV2RefinementMarginMm);
  const autoSupportV2RefinementMaxRegionsValue = Number(autoSupportV2RefinementMaxRegions);
  const autoSupportV2RiskForceMarginRatioValue = Number(autoSupportV2RiskForceMarginRatio);
  const autoSupportV2MinRegionVolumeMm3Value = Number(autoSupportV2MinRegionVolumeMm3);
  const previewGenerationVersionLimit = instanceStats?.previewGenerationVersion;

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
    Number.isFinite(autoSupportMaxSupportDistanceValue) &&
    autoSupportMaxSupportDistanceValue >= autoSupportMergeDistanceValue &&
    Number.isFinite(autoSupportUnsupportedIslandVolumeThresholdValue) &&
    autoSupportUnsupportedIslandVolumeThresholdValue >= 0.01 &&
    autoSupportUnsupportedIslandVolumeThresholdValue <= 100000 &&
    Number.isFinite(autoSupportPullForceThresholdValue) &&
    autoSupportPullForceThresholdValue >= 0.1 &&
    Number.isFinite(autoSupportSphereRadiusValue) &&
    autoSupportSphereRadiusValue >= 0.1 &&
    Number.isInteger(autoSupportMaxSupportsPerIslandValue) &&
    autoSupportMaxSupportsPerIslandValue >= 1 &&
    autoSupportMaxSupportsPerIslandValue <= 64 &&
    Number.isFinite(autoSupportResinStrengthValue) &&
    autoSupportResinStrengthValue >= 0.1 &&
    autoSupportResinStrengthValue <= 10 &&
    Number.isFinite(autoSupportResinDensityValue) &&
    autoSupportResinDensityValue >= 0.1 &&
    autoSupportResinDensityValue <= 10 &&
    Number.isFinite(autoSupportPeelForceMultiplierValue) &&
    autoSupportPeelForceMultiplierValue >= 0.01 &&
    autoSupportPeelForceMultiplierValue <= 5 &&
    Number.isFinite(autoSupportMicroTipRadiusValue) &&
    autoSupportMicroTipRadiusValue >= 0.1 &&
    autoSupportMicroTipRadiusValue <= 3 &&
    Number.isFinite(autoSupportLightTipRadiusValue) &&
    autoSupportLightTipRadiusValue >= 0.1 &&
    autoSupportLightTipRadiusValue <= 5 &&
    Number.isFinite(autoSupportMediumTipRadiusValue) &&
    autoSupportMediumTipRadiusValue >= 0.1 &&
    autoSupportMediumTipRadiusValue <= 7 &&
    Number.isFinite(autoSupportHeavyTipRadiusValue) &&
    autoSupportHeavyTipRadiusValue >= 0.1 &&
    autoSupportHeavyTipRadiusValue <= 10 &&
    Number.isFinite(autoSupportV2VoxelSizeValue) &&
    autoSupportV2VoxelSizeValue >= 0.1 &&
    autoSupportV2VoxelSizeValue <= 10 &&
    Number.isFinite(autoSupportV2CoarseVoxelSizeValue) &&
    autoSupportV2CoarseVoxelSizeValue >= 0.1 &&
    autoSupportV2CoarseVoxelSizeValue <= 10 &&
    Number.isFinite(autoSupportV2FineVoxelSizeValue) &&
    autoSupportV2FineVoxelSizeValue >= 0.1 &&
    autoSupportV2FineVoxelSizeValue <= autoSupportV2CoarseVoxelSizeValue &&
    Number.isFinite(autoSupportV2RefinementMarginValue) &&
    autoSupportV2RefinementMarginValue >= 0.1 &&
    autoSupportV2RefinementMarginValue <= 20 &&
    Number.isFinite(autoSupportV2RefinementMaxRegionsValue) &&
    autoSupportV2RefinementMaxRegionsValue >= 1 &&
    autoSupportV2RefinementMaxRegionsValue <= 128 &&
    Number.isFinite(autoSupportV2RiskForceMarginRatioValue) &&
    autoSupportV2RiskForceMarginRatioValue >= 0 &&
    autoSupportV2RiskForceMarginRatioValue <= 1 &&
    Number.isFinite(autoSupportV2MinRegionVolumeMm3Value) &&
    autoSupportV2MinRegionVolumeMm3Value >= 0.1 &&
    autoSupportV2MinRegionVolumeMm3Value <= 10000;

  const currentSection: SettingsSectionKey = useMemo(() => {
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
      autoSupportMaxSupportsPerIsland: autoSupportMaxSupportsPerIslandValue,
      autoSupportResinStrength: autoSupportResinStrengthValue,
      autoSupportResinDensityGPerMl: autoSupportResinDensityValue,
      autoSupportPeelForceMultiplier: autoSupportPeelForceMultiplierValue,
      autoSupportMicroTipRadiusMm: autoSupportMicroTipRadiusValue,
      autoSupportLightTipRadiusMm: autoSupportLightTipRadiusValue,
      autoSupportMediumTipRadiusMm: autoSupportMediumTipRadiusValue,
      autoSupportHeavyTipRadiusMm: autoSupportHeavyTipRadiusValue,
      autoSupportV2VoxelSizeMm: autoSupportV2VoxelSizeValue,
      autoSupportV2OptimizationEnabled: autoSupportV2OptimizationEnabled,
      autoSupportV2CoarseVoxelSizeMm: autoSupportV2CoarseVoxelSizeValue,
      autoSupportV2FineVoxelSizeMm: autoSupportV2FineVoxelSizeValue,
      autoSupportV2RefinementMarginMm: autoSupportV2RefinementMarginValue,
      autoSupportV2RefinementMaxRegions: autoSupportV2RefinementMaxRegionsValue,
      autoSupportV2RiskForceMarginRatio: autoSupportV2RiskForceMarginRatioValue,
      autoSupportV2MinRegionVolumeMm3: autoSupportV2MinRegionVolumeMm3Value,
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
                <Typography variant="h6">Auto support preview tuning</Typography>
                <Typography color="text.secondary">
                  These values control how aggressively support-point spheres are generated for
                  unsupported models.
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
                    label="Min support distance (mm)"
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
                <Stack
                  direction={{ xs: 'column', md: 'row' }}
                  spacing={1}
                  className={styles.addRow}
                >
                  <TextField
                    size="small"
                    type="number"
                    label="Max support distance (mm)"
                    value={autoSupportMaxSupportDistanceMm}
                    onChange={(e) => setAutoSupportMaxSupportDistanceMm(e.target.value)}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Pull-force threshold (legacy)"
                    value={autoSupportPullForceThreshold}
                    onChange={(e) => setAutoSupportPullForceThreshold(e.target.value)}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Marker sphere radius (mm, legacy)"
                    value={autoSupportSphereRadiusMm}
                    onChange={(e) => setAutoSupportSphereRadiusMm(e.target.value)}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Max supports per island"
                    value={autoSupportMaxSupportsPerIsland}
                    onChange={(e) => setAutoSupportMaxSupportsPerIsland(e.target.value)}
                  />
                </Stack>
                <Typography variant="h6">Support tip sizing</Typography>
                <Typography color="text.secondary">
                  Per-size tip radii control the contact area of each support type. Pull force
                  capacity is computed as pi * radius^2 * resin strength. Supports near the model
                  base automatically use heavy tips.
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
                    label="Micro tip radius (mm)"
                    value={autoSupportMicroTipRadiusMm}
                    onChange={(e) => setAutoSupportMicroTipRadiusMm(e.target.value)}
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
                <Typography variant="h6">Method 2 settings</Typography>
                <Typography color="text.secondary">
                  Settings specific to the force-based voxel algorithm (method 2). Smaller voxels
                  improve detail capture but increase processing cost.
                </Typography>
                <Stack
                  direction={{ xs: 'column', md: 'row' }}
                  spacing={1}
                  className={styles.addRow}
                >
                  <TextField
                    size="small"
                    type="number"
                    label="Voxel size (mm)"
                    value={autoSupportV2VoxelSizeMm}
                    onChange={(e) => setAutoSupportV2VoxelSizeMm(e.target.value)}
                    helperText="Resolution of the voxel grid for method 2 (0.1 to 10, default 2)"
                  />
                </Stack>
                <FormControlLabel
                  control={
                    <Switch
                      checked={autoSupportV2OptimizationEnabled}
                      onChange={(e) => setAutoSupportV2OptimizationEnabled(e.target.checked)}
                    />
                  }
                  label="Two-stage optimization"
                />
                <Typography color="text.secondary">
                  When enabled, uses a coarse pass to identify problem regions, then refines only
                  those areas at higher resolution. Improves performance on large models.
                </Typography>
                <Stack
                  direction={{ xs: 'column', md: 'row' }}
                  spacing={1}
                  className={styles.addRow}
                >
                  <TextField
                    size="small"
                    type="number"
                    label="Coarse voxel size (mm)"
                    value={autoSupportV2CoarseVoxelSizeMm}
                    onChange={(e) => setAutoSupportV2CoarseVoxelSizeMm(e.target.value)}
                    helperText="Voxel size for the initial coarse pass (0.1 to 10, default 4)"
                    disabled={!autoSupportV2OptimizationEnabled}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Fine voxel size (mm)"
                    value={autoSupportV2FineVoxelSizeMm}
                    onChange={(e) => setAutoSupportV2FineVoxelSizeMm(e.target.value)}
                    helperText="Voxel size for regional refinement (0.1 to coarse size, default 0.5)"
                    disabled={!autoSupportV2OptimizationEnabled}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Refinement margin (mm)"
                    value={autoSupportV2RefinementMarginMm}
                    onChange={(e) => setAutoSupportV2RefinementMarginMm(e.target.value)}
                    helperText="Margin around detected problem regions (0.1 to 20, default 2)"
                    disabled={!autoSupportV2OptimizationEnabled}
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
                    label="Max refinement regions"
                    value={autoSupportV2RefinementMaxRegions}
                    onChange={(e) => setAutoSupportV2RefinementMaxRegions(e.target.value)}
                    helperText="Maximum number of regions to refine (1 to 128, default 12)"
                    disabled={!autoSupportV2OptimizationEnabled}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Risk force margin ratio"
                    value={autoSupportV2RiskForceMarginRatio}
                    onChange={(e) => setAutoSupportV2RiskForceMarginRatio(e.target.value)}
                    helperText="Force margin ratio for risk detection (0 to 1, default 0.2)"
                    disabled={!autoSupportV2OptimizationEnabled}
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Min region volume (mm3)"
                    value={autoSupportV2MinRegionVolumeMm3}
                    onChange={(e) => setAutoSupportV2MinRegionVolumeMm3(e.target.value)}
                    helperText="Minimum volume for a refinement region (0.1 to 10000, default 8)"
                    disabled={!autoSupportV2OptimizationEnabled}
                  />
                </Stack>
                <Typography variant="h6">Cumulative force settings</Typography>
                <Typography color="text.secondary">
                  Controls how resin weight and peel forces accumulate across layers. Higher density
                  or peel multiplier values increase per-support loads, triggering more or heavier
                  supports.
                </Typography>
                <Stack
                  direction={{ xs: 'column', md: 'row' }}
                  spacing={1}
                  className={styles.addRow}
                >
                  <TextField
                    size="small"
                    type="number"
                    label="Unsupported volume threshold (mm^3)"
                    value={autoSupportUnsupportedIslandVolumeThresholdMm3}
                    onChange={(e) =>
                      setAutoSupportUnsupportedIslandVolumeThresholdMm3(e.target.value)
                    }
                    helperText="Place support once unsupported cumulative volume exceeds this"
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Resin density (g/ml)"
                    value={autoSupportResinDensityGPerMl}
                    onChange={(e) => setAutoSupportResinDensityGPerMl(e.target.value)}
                    helperText="Default 1.25 (1000g per 800ml)"
                  />
                  <TextField
                    size="small"
                    type="number"
                    label="Peel force multiplier"
                    value={autoSupportPeelForceMultiplier}
                    onChange={(e) => setAutoSupportPeelForceMultiplier(e.target.value)}
                    helperText="Scales cross-sectional area to peel force (default 0.15)"
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
                      pixelWidth: String(printer.pixelWidth),
                      pixelHeight: String(printer.pixelHeight),
                    };
                    const width = Number(edit.bedWidthMm);
                    const depth = Number(edit.bedDepthMm);
                    const pixelWidth = Number(edit.pixelWidth);
                    const pixelHeight = Number(edit.pixelHeight);
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
                      (edit.name.trim() !== printer.name ||
                        width !== printer.bedWidthMm ||
                        depth !== printer.bedDepthMm ||
                        pixelWidth !== printer.pixelWidth ||
                        pixelHeight !== printer.pixelHeight);

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
                  <Button
                    variant="contained"
                    disabled={
                      createPrinterMutation.isPending ||
                      !newPrinterName.trim() ||
                      Number(newPrinterWidthMm) <= 0 ||
                      Number(newPrinterDepthMm) <= 0 ||
                      Number(newPrinterPixelWidth) <= 0 ||
                      Number(newPrinterPixelHeight) <= 0
                    }
                    onClick={() => {
                      createPrinterMutation.mutate(
                        {
                          name: newPrinterName.trim(),
                          bedWidthMm: Number(newPrinterWidthMm),
                          bedDepthMm: Number(newPrinterDepthMm),
                          pixelWidth: Number(newPrinterPixelWidth),
                          pixelHeight: Number(newPrinterPixelHeight),
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
