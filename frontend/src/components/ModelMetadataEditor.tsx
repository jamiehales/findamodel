import { useEffect, useState } from 'react';
import Button from '@mui/material/Button';
import CircularProgress from '@mui/material/CircularProgress';
import FormControl from '@mui/material/FormControl';
import IconButton from '@mui/material/IconButton';
import MenuItem from '@mui/material/MenuItem';
import Select from '@mui/material/Select';
import Stack from '@mui/material/Stack';
import TextField from '@mui/material/TextField';
import Tooltip from '@mui/material/Tooltip';
import Typography from '@mui/material/Typography';
import type { Model, ModelMetadata } from '../lib/api';
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

type FormState = ModelMetadata;

function InheritedHint({ value }: { value?: string | boolean | number | null | undefined }) {
  const [copied, setCopied] = useState(false);

  if (value == null) return null;

  const textToCopy = String(value);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(textToCopy);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy to clipboard:', err);
    }
  };

  return (
    <Stack direction="row" alignItems="flex-start" spacing={1} className={styles.hintContainer}>
      <Typography variant="caption" color="text.disabled" component="div" className={styles.hint}>
        Inherited: {String(value)}
      </Typography>
      <Tooltip title={copied ? 'Copied!' : 'Copy to clipboard'}>
        <IconButton size="small" onClick={handleCopy} className={styles.copyBtn}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
            <path d="M16 1H4c-1.1 0-2 .9-2 2v14h2V3h12V1zm3 4H8c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h11c1.1 0 2-.9 2-2V7c0-1.1-.9-2-2-2zm0 16H8V7h11v14z" />
          </svg>
        </IconButton>
      </Tooltip>
    </Stack>
  );
}

interface FieldDef {
  key: keyof FormState;
  label: string;
  fieldType: 'text' | 'select' | 'bool';
  optionsField?: 'category' | 'type' | 'material';
}

const FIELDS: FieldDef[] = [
  { key: 'name', label: 'Model Name', fieldType: 'text' },
  { key: 'partName', label: 'Part Name', fieldType: 'text' },
  { key: 'creator', label: 'Creator', fieldType: 'text' },
  { key: 'collection', label: 'Collection', fieldType: 'text' },
  { key: 'subcollection', label: 'Subcollection', fieldType: 'text' },
  { key: 'category', label: 'Category', fieldType: 'select', optionsField: 'category' },
  { key: 'type', label: 'Type', fieldType: 'select', optionsField: 'type' },
  { key: 'material', label: 'Material', fieldType: 'select', optionsField: 'material' },
  { key: 'supported', label: 'Supported', fieldType: 'bool' },
];

const EMPTY_FORM: FormState = {
  name: null,
  partName: null,
  creator: null,
  collection: null,
  subcollection: null,
  category: null,
  type: null,
  material: null,
  supported: null,
};

export default function ModelMetadataEditor({ model, onClose }: Props) {
  const mutation = useUpdateModelMetadata(model.id);
  const { data: detail } = useModelMetadata(model.id);
  const { data: metadataDictionary } = useMetadataDictionaryOverview();
  const [error, setError] = useState<string | null>(null);
  const [savedIndicator, setSavedIndicator] = useState(false);

  const [form, setForm] = useState<FormState>(EMPTY_FORM);

  useEffect(() => {
    if (detail) {
      setForm({ ...detail.localValues });
    }
  }, [detail]);

  function setField<K extends keyof FormState>(key: K, value: FormState[K]) {
    setForm((prev) => ({ ...prev, [key]: value }));
  }

  async function onSave() {
    if (mutation.isPending) return;
    setError(null);

    try {
      await mutation.mutateAsync({
        name: form.name,
        partName: form.partName,
        creator: form.creator,
        collection: form.collection,
        subcollection: form.subcollection,
        category: form.category,
        type: form.type,
        material: form.material,
        supported: form.supported,
      });
      setSavedIndicator(true);
      setTimeout(() => setSavedIndicator(false), 2000);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save model metadata');
    }
  }

  function getSelectOptions(field: FieldDef, localValue: string | null): string[] {
    if (field.optionsField == null) return [];
    const configured =
      metadataDictionary?.[field.optionsField].configured.map((v) => v.value) ?? [];
    const values = [...configured];
    if (localValue && !values.includes(localValue)) values.push(localValue);
    return values;
  }

  return (
    <Stack className={styles.form}>
      {FIELDS.map((field) => {
        const localValue = form[field.key] as string | boolean | null;
        const inheritedValue =
          detail?.inheritedValues != null
            ? (detail.inheritedValues[field.key] as string | boolean | null)
            : null;
        const hasLocalValue = localValue != null;
        const hasInheritedValue = inheritedValue != null;
        const canReset = hasLocalValue && hasInheritedValue;
        const selectOptions =
          field.fieldType === 'select'
            ? getSelectOptions(field, typeof localValue === 'string' ? localValue : null)
            : [];

        const handleReset = () => setField(field.key, null as never);

        return (
          <Stack key={field.key}>
            <Stack
              direction="row"
              alignItems="center"
              justifyContent="space-between"
              className={styles.fieldHeader}
            >
              <Typography variant="caption" color="text.secondary">
                {field.label}
              </Typography>
              <Tooltip
                title={canReset ? 'Reset to inherited value' : 'No inherited value to reset to'}
              >
                <span>
                  <IconButton
                    size="small"
                    disabled={!canReset}
                    onClick={handleReset}
                    className={styles.resetBtn}
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
                      <path d="M12 5V1L7 6l5 5V7c3.31 0 6 2.69 6 6s-2.69 6-6 6-6-2.69-6-6H4c0 4.42 3.58 8 8 8s8-3.58 8-8-3.58-8-8-8z" />
                    </svg>
                  </IconButton>
                </span>
              </Tooltip>
            </Stack>

            {field.fieldType === 'text' && (
              <TextField
                size="small"
                fullWidth
                value={typeof localValue === 'string' ? localValue : ''}
                placeholder={typeof inheritedValue === 'string' ? inheritedValue : undefined}
                onChange={(e) => setField(field.key, (e.target.value || null) as never)}
                InputProps={{ className: styles.fieldInput }}
              />
            )}

            {field.fieldType === 'select' && (
              <FormControl size="small" fullWidth>
                <Select
                  displayEmpty
                  value={typeof localValue === 'string' ? localValue : ''}
                  onChange={(e) => setField(field.key, (e.target.value || null) as never)}
                  className={styles.selectSmall}
                  renderValue={(v) =>
                    v ? (
                      String(v)
                    ) : (
                      <em style={{ color: 'inherit', opacity: 0.5 }}>
                        Not set{inheritedValue ? ` (${inheritedValue})` : ''}
                      </em>
                    )
                  }
                >
                  <MenuItem value="">
                    <em>Not set</em>
                  </MenuItem>
                  {selectOptions.map((o) => (
                    <MenuItem key={o} value={o}>
                      {o}
                    </MenuItem>
                  ))}
                </Select>
              </FormControl>
            )}

            {field.fieldType === 'bool' && (
              <FormControl size="small" fullWidth>
                <Select
                  displayEmpty
                  value={localValue === null ? '' : String(localValue)}
                  onChange={(e) => {
                    const v = e.target.value;
                    setField(field.key, (v === '' ? null : v === 'true') as never);
                  }}
                  className={styles.selectSmall}
                  renderValue={(v) =>
                    v !== '' ? (
                      v === 'true' ? (
                        'True'
                      ) : (
                        'False'
                      )
                    ) : (
                      <em style={{ color: 'inherit', opacity: 0.5 }}>
                        Not set{inheritedValue != null ? ` (${String(inheritedValue)})` : ''}
                      </em>
                    )
                  }
                >
                  <MenuItem value="">
                    <em>Not set</em>
                  </MenuItem>
                  <MenuItem value="true">True</MenuItem>
                  <MenuItem value="false">False</MenuItem>
                </Select>
              </FormControl>
            )}

            <InheritedHint value={inheritedValue} />
          </Stack>
        );
      })}

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
