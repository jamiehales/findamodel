import { useEffect, useState } from 'react';
import Button from '@mui/material/Button';
import CircularProgress from '@mui/material/CircularProgress';
import FormControl from '@mui/material/FormControl';
import InputLabel from '@mui/material/InputLabel';
import MenuItem from '@mui/material/MenuItem';
import Select from '@mui/material/Select';
import Stack from '@mui/material/Stack';
import TextField from '@mui/material/TextField';
import Typography from '@mui/material/Typography';
import type { Model } from '../lib/api';
import {
  useMetadataDictionaryOverview,
  useModelMetadata,
  useUpdateModelMetadata,
} from '../lib/queries';
import styles from './ModelMetadataEditor.module.css';

interface Props {
  model: Model;
  onClose: () => void;
}

interface FormState {
  name: string;
  partName: string;
  creator: string;
  collection: string;
  subcollection: string;
  category: string;
  type: string;
  material: string;
  supported: '' | 'true' | 'false';
}

function toText(v: string | null | undefined): string {
  return v ?? '';
}

function toSupported(v: boolean | null | undefined): '' | 'true' | 'false' {
  if (v === true) return 'true';
  if (v === false) return 'false';
  return '';
}

export default function ModelMetadataEditor({ model, onClose }: Props) {
  const mutation = useUpdateModelMetadata(model.id);
  const { data: override } = useModelMetadata(model.id);
  const { data: metadataDictionary } = useMetadataDictionaryOverview();
  const [error, setError] = useState<string | null>(null);
  const [savedIndicator, setSavedIndicator] = useState(false);

  const [form, setForm] = useState<FormState>({
    name: toText(model.name),
    partName: toText(model.partName),
    creator: toText(model.creator),
    collection: toText(model.collection),
    subcollection: toText(model.subcollection),
    category: toText(model.category),
    type: toText(model.type),
    material: toText(model.material),
    supported: toSupported(model.supported),
  });

  useEffect(() => {
    setForm({
      name: toText(override?.name ?? model.name),
      partName: toText(override?.partName ?? model.partName),
      creator: toText(override?.creator ?? model.creator),
      collection: toText(override?.collection ?? model.collection),
      subcollection: toText(override?.subcollection ?? model.subcollection),
      category: toText(override?.category ?? model.category),
      type: toText(override?.type ?? model.type),
      material: toText(override?.material ?? model.material),
      supported: toSupported(override?.supported ?? model.supported),
    });
  }, [model, override]);

  function setField<K extends keyof FormState>(key: K, value: FormState[K]) {
    setForm((prev) => ({ ...prev, [key]: value }));
  }

  function asNullable(value: string): string | null {
    const trimmed = value.trim();
    return trimmed.length > 0 ? trimmed : null;
  }

  async function onSave() {
    if (mutation.isPending) return;
    setError(null);

    try {
      await mutation.mutateAsync({
        name: asNullable(form.name),
        partName: asNullable(form.partName),
        creator: asNullable(form.creator),
        collection: asNullable(form.collection),
        subcollection: asNullable(form.subcollection),
        category: asNullable(form.category),
        type: asNullable(form.type),
        material: asNullable(form.material),
        supported: form.supported === '' ? null : form.supported === 'true',
      });
      setSavedIndicator(true);
      setTimeout(() => setSavedIndicator(false), 2000);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save model metadata');
    }
  }

  return (
    <Stack className={styles.form}>
      <TextField
        label="Model Name"
        size="small"
        value={form.name}
        onChange={(e) => setField('name', e.target.value)}
        InputProps={{ className: styles.fieldInput }}
      />

      <TextField
        label="Part Name"
        size="small"
        value={form.partName}
        onChange={(e) => setField('partName', e.target.value)}
        InputProps={{ className: styles.fieldInput }}
      />

      <TextField
        label="Creator"
        size="small"
        value={form.creator}
        onChange={(e) => setField('creator', e.target.value)}
        InputProps={{ className: styles.fieldInput }}
      />

      <TextField
        label="Collection"
        size="small"
        value={form.collection}
        onChange={(e) => setField('collection', e.target.value)}
        InputProps={{ className: styles.fieldInput }}
      />

      <TextField
        label="Subcollection"
        size="small"
        value={form.subcollection}
        onChange={(e) => setField('subcollection', e.target.value)}
        InputProps={{ className: styles.fieldInput }}
      />

      <FormControl size="small" fullWidth>
        <InputLabel id="model-metadata-category-label">Category</InputLabel>
        <Select
          labelId="model-metadata-category-label"
          label="Category"
          value={form.category}
          onChange={(e) => setField('category', String(e.target.value))}
          className={styles.selectSmall}
        >
          <MenuItem value="">(none)</MenuItem>
          {(metadataDictionary?.category.configured ?? []).map((v) => (
            <MenuItem key={v.id} value={v.value}>
              {v.value}
            </MenuItem>
          ))}
        </Select>
      </FormControl>

      <FormControl size="small" fullWidth>
        <InputLabel id="model-metadata-type-label">Type</InputLabel>
        <Select
          labelId="model-metadata-type-label"
          label="Type"
          value={form.type}
          onChange={(e) => setField('type', String(e.target.value))}
          className={styles.selectSmall}
        >
          <MenuItem value="">(none)</MenuItem>
          {(metadataDictionary?.type.configured ?? []).map((v) => (
            <MenuItem key={v.id} value={v.value}>
              {v.value}
            </MenuItem>
          ))}
        </Select>
      </FormControl>

      <FormControl size="small" fullWidth>
        <InputLabel id="model-metadata-material-label">Material</InputLabel>
        <Select
          labelId="model-metadata-material-label"
          label="Material"
          value={form.material}
          onChange={(e) => setField('material', String(e.target.value))}
          className={styles.selectSmall}
        >
          <MenuItem value="">(none)</MenuItem>
          {(metadataDictionary?.material.configured ?? []).map((v) => (
            <MenuItem key={v.id} value={v.value}>
              {v.value}
            </MenuItem>
          ))}
        </Select>
      </FormControl>

      <FormControl size="small" fullWidth>
        <InputLabel id="model-metadata-supported-label">Supported</InputLabel>
        <Select
          labelId="model-metadata-supported-label"
          label="Supported"
          value={form.supported}
          onChange={(e) => setField('supported', e.target.value as '' | 'true' | 'false')}
          className={styles.selectSmall}
        >
          <MenuItem value="">(none)</MenuItem>
          <MenuItem value="true">True</MenuItem>
          <MenuItem value="false">False</MenuItem>
        </Select>
      </FormControl>

      <Stack className={styles.actions}>
        {error && (
          <Typography variant="caption" color="error" className={styles.inlineError}>
            {error}
          </Typography>
        )}
        {savedIndicator && (
          <Typography variant="caption" color="success.main">
            Saved
          </Typography>
        )}
        <Button onClick={onClose}>Close</Button>
        <Button onClick={onSave} variant="contained" disabled={mutation.isPending}>
          {mutation.isPending ? <CircularProgress size={16} /> : 'Save'}
        </Button>
      </Stack>
    </Stack>
  );
}
