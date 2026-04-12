import { useEffect, useMemo, useState } from 'react';
import { Link } from 'react-router-dom';
import {
  Box,
  Button,
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
  useMetadataDictionaryOverview,
  useUpdateAppConfig,
  useUpdateMetadataDictionaryValue,
} from '../lib/queries';
import type { MetadataDictionaryField } from '../lib/api';
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

export default function SettingsPage() {
  const { data: appConfig, isPending: appConfigPending, isError: appConfigError } = useAppConfig();
  const updateAppConfigMutation = useUpdateAppConfig();
  const [defaultRaftHeightMm, setDefaultRaftHeightMm] = useState('');
  const [theme, setTheme] = useState<string>('nord');
  const { data, isPending, isError } = useMetadataDictionaryOverview();

  useEffect(() => {
    if (appConfig) {
      setDefaultRaftHeightMm(String(appConfig.defaultRaftHeightMm));
      setTheme(appConfig.theme);
    }
  }, [appConfig]);

  if (isPending || appConfigPending) return <LoadingView />;

  if (isError || appConfigError || !data || !appConfig) {
    return <ErrorView message="Failed to load settings." />;
  }

  return (
    <PageLayout variant="medium" spacing={2}>
      <Typography component="h1" variant="page-title">
        Settings
      </Typography>

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
              disabled={
                updateAppConfigMutation.isPending ||
                !defaultRaftHeightMm.trim() ||
                Number(defaultRaftHeightMm) < 0 ||
                !Number.isFinite(Number(defaultRaftHeightMm))
              }
              onClick={() =>
                updateAppConfigMutation.mutate({
                  defaultRaftHeightMm: Number(defaultRaftHeightMm),
                  theme,
                })
              }
            >
              Save
            </Button>
          </Stack>
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
    </PageLayout>
  );
}
