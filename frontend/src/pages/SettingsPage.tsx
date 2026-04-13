import { useEffect, useMemo, useState } from 'react';
import { Link } from 'react-router-dom';
import {
  Box,
  Button,
  FormControlLabel,
  MenuItem,
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
  useCreateMetadataDictionaryValue,
  useDeleteMetadataDictionaryValue,
  useInstanceStats,
  useMetadataDictionaryOverview,
  useUpdateAppConfig,
  useUpdateMetadataDictionaryValue,
} from '../lib/queries';
import type { InstanceStats, MetadataDictionaryField } from '../lib/api';
import ErrorView from '../components/ErrorView';
import LoadingView from '../components/LoadingView';
import PageLayout from '../components/layouts/PageLayout';
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
    <Box className={styles.section}>
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
    </Box>
  );
}

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
      <Typography variant="h5">Instance Stats</Typography>
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
  const { data: appConfig, isPending: appConfigPending, isError: appConfigError } = useAppConfig();
  const {
    data: instanceStats,
    isPending: instanceStatsPending,
    isError: instanceStatsError,
  } = useInstanceStats();
  const updateAppConfigMutation = useUpdateAppConfig();
  const [defaultRaftHeightMm, setDefaultRaftHeightMm] = useState('');
  const [theme, setTheme] = useState<string>('nord');
  const [tagGenerationEnabled, setTagGenerationEnabled] = useState(false);
  const [aiDescriptionEnabled, setAiDescriptionEnabled] = useState(false);
  const [tagGenerationProvider, setTagGenerationProvider] = useState<'internal' | 'ollama'>(
    'internal',
  );
  const [tagGenerationEndpoint, setTagGenerationEndpoint] = useState('http://localhost:11434');
  const [tagGenerationModel, setTagGenerationModel] = useState('qwen2.5vl:7b');
  const [tagGenerationTimeoutMs, setTagGenerationTimeoutMs] = useState('60000');
  const [tagGenerationMaxTags, setTagGenerationMaxTags] = useState('12');
  const [tagGenerationMinConfidence, setTagGenerationMinConfidence] = useState('0.45');
  const { data, isPending, isError } = useMetadataDictionaryOverview();

  useEffect(() => {
    if (appConfig) {
      setDefaultRaftHeightMm(String(appConfig.defaultRaftHeightMm));
      setTheme(appConfig.theme);
      setTagGenerationEnabled(appConfig.tagGenerationEnabled);
      setAiDescriptionEnabled(appConfig.aiDescriptionEnabled);
      setTagGenerationProvider(
        appConfig.tagGenerationProvider === 'ollama' ? 'ollama' : 'internal',
      );
      setTagGenerationEndpoint(appConfig.tagGenerationEndpoint);
      setTagGenerationModel(appConfig.tagGenerationModel);
      setTagGenerationTimeoutMs(String(appConfig.tagGenerationTimeoutMs));
      setTagGenerationMaxTags(String(appConfig.tagGenerationMaxTags));
      setTagGenerationMinConfidence(String(appConfig.tagGenerationMinConfidence));
    }
  }, [appConfig]);

  const raftHeightValue = Number(defaultRaftHeightMm);
  const timeoutValue = Number(tagGenerationTimeoutMs);
  const maxTagsValue = Number(tagGenerationMaxTags);
  const minConfidenceValue = Number(tagGenerationMinConfidence);

  const appConfigValid =
    defaultRaftHeightMm.trim().length > 0 &&
    Number.isFinite(raftHeightValue) &&
    raftHeightValue >= 0 &&
    tagGenerationEndpoint.trim().length > 0 &&
    tagGenerationModel.trim().length > 0 &&
    Number.isInteger(timeoutValue) &&
    timeoutValue >= 1000 &&
    timeoutValue <= 300000 &&
    Number.isInteger(maxTagsValue) &&
    maxTagsValue >= 1 &&
    maxTagsValue <= 64 &&
    Number.isFinite(minConfidenceValue) &&
    minConfidenceValue >= 0 &&
    minConfidenceValue <= 1;

  if (isPending || appConfigPending) return <LoadingView />;

  if (isError || appConfigError || !data || !appConfig) {
    return <ErrorView message="Failed to load settings." />;
  }

  return (
    <PageLayout variant="medium" spacing={2}>
      <Typography component="h1" variant="page-title">
        Settings
      </Typography>

      <Stack direction="row" spacing={1}>
        <Button component={Link} to="/settings/logs" variant="outlined">
          View Application Logs
        </Button>
      </Stack>

      <Box className={styles.globalSettingsSection}>
        <Typography variant="h5">Default Values</Typography>
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
            <Button
              variant="contained"
              disabled={updateAppConfigMutation.isPending || !appConfigValid}
              onClick={() =>
                updateAppConfigMutation.mutate({
                  defaultRaftHeightMm: raftHeightValue,
                  theme,
                  tagGenerationEnabled,
                  aiDescriptionEnabled,
                  tagGenerationProvider,
                  tagGenerationEndpoint: tagGenerationEndpoint.trim(),
                  tagGenerationModel: tagGenerationModel.trim(),
                  tagGenerationTimeoutMs: timeoutValue,
                  tagGenerationMaxTags: maxTagsValue,
                  tagGenerationMinConfidence: minConfidenceValue,
                })
              }
            >
              Save
            </Button>
          </Stack>
        </Stack>
      </Box>

      <Box className={styles.globalSettingsSection}>
        <Typography variant="h5">Tag Generation</Typography>
        <Stack spacing={2}>
          <Stack direction="row" spacing={1}>
            <FormControlLabel
              control={
                <Switch
                  checked={tagGenerationEnabled}
                  onChange={(e) => setTagGenerationEnabled(e.target.checked)}
                />
              }
              label="Enable tag generation"
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

          <TextField
            size="small"
            label="Model"
            value={tagGenerationModel}
            onChange={(e) => setTagGenerationModel(e.target.value)}
            error={!tagGenerationModel.trim()}
            helperText={!tagGenerationModel.trim() ? 'Model is required.' : undefined}
          />

          <TextField
            size="small"
            type="number"
            label="Timeout (ms)"
            value={tagGenerationTimeoutMs}
            onChange={(e) => setTagGenerationTimeoutMs(e.target.value)}
            error={!Number.isInteger(timeoutValue) || timeoutValue < 1000 || timeoutValue > 300000}
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
        </Stack>
      </Box>

      <Stack
        direction="row"
        spacing={2}
        divider={<Divider orientation="vertical" flexItem />}
        className={styles.sectionsRow}
      >
        <FieldSection field="category" data={data.category} />
        <FieldSection field="type" data={data.type} />
        <FieldSection field="material" data={data.material} />
        <FieldSection field="tags" data={data.tags} />
      </Stack>

      <InstanceStatsSection
        stats={instanceStats}
        isPending={instanceStatsPending}
        isError={instanceStatsError}
      />
    </PageLayout>
  );
}
