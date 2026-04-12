import { useEffect, useState } from 'react';
import Button from '@mui/material/Button';
import CircularProgress from '@mui/material/CircularProgress';
import IconButton from '@mui/material/IconButton';
import Stack from '@mui/material/Stack';
import TextField from '@mui/material/TextField';
import Tooltip from '@mui/material/Tooltip';
import Typography from '@mui/material/Typography';
import type { Model, ModelMetadata } from '../lib/api';
import { SHARED_FIELDS, type SharedFieldDef } from './metadata/fieldDefs';
import InheritedHint from './metadata/InheritedHint';
import MetadataBoolField from './metadata/MetadataBoolField';
import MetadataSelectField from './metadata/MetadataSelectField';
import MetadataTextField from './metadata/MetadataTextField';
import TagEditor from './metadata/TagEditor';
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

type SharedModelKey = keyof FormState | 'modelName';

const MODEL_FIELDS: SharedFieldDef[] = SHARED_FIELDS;

const EMPTY_FORM: FormState = {
  name: null,
  partName: null,
  creator: null,
  collection: null,
  subcollection: null,
  tags: null,
  category: null,
  type: null,
  material: null,
  supported: null,
  raftHeightMm: null,
};

function toModelKey(key: SharedModelKey): keyof FormState {
  return key === 'modelName' ? 'name' : key;
}

function getModelValue(data: FormState, key: SharedModelKey): string | boolean | number | null {
  const modelKey = toModelKey(key);
  return data[modelKey] as string | boolean | number | null;
}

function hasFormChanges(current: FormState, original: FormState): boolean {
  const currentTags = current.tags ?? [];
  const originalTags = original.tags ?? [];

  return (
    current.name !== original.name ||
    current.partName !== original.partName ||
    current.creator !== original.creator ||
    current.collection !== original.collection ||
    current.subcollection !== original.subcollection ||
    currentTags.length !== originalTags.length ||
    currentTags.some((v, i) => v !== originalTags[i]) ||
    current.category !== original.category ||
    current.type !== original.type ||
    current.material !== original.material ||
    current.supported !== original.supported ||
    current.raftHeightMm !== original.raftHeightMm
  );
}

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

  function setFieldBySharedKey(key: SharedModelKey, value: string | boolean | number | null) {
    const modelKey = toModelKey(key);
    setForm((prev) => ({ ...prev, [modelKey]: value }));
  }

  async function onSave() {
    if (mutation.isPending || !detail) return;
    setError(null);

    try {
      const parsedTags = form.tags ?? null;

      await mutation.mutateAsync({
        name: form.name,
        partName: form.partName,
        creator: form.creator,
        collection: form.collection,
        subcollection: form.subcollection,
        tags: parsedTags,
        category: form.category,
        type: form.type,
        material: form.material,
        supported: form.supported,
        raftHeightMm: form.raftHeightMm,
      });
      setSavedIndicator(true);
      setTimeout(() => setSavedIndicator(false), 2000);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to save model metadata');
    }
  }

  const isDirty = detail ? hasFormChanges(form, detail.localValues) : false;

  function getSelectOptions(field: SharedFieldDef, localValue: string | null): string[] {
    if (field.optionsField == null) return [];
    const configured =
      metadataDictionary?.[field.optionsField].configured.map((v) => v.value) ?? [];
    const values = [...configured];
    if (localValue && !values.includes(localValue)) values.push(localValue);
    return values;
  }

  return (
    <Stack className={styles.form}>
      {MODEL_FIELDS.map((field) => {
        const fieldKey = field.key as SharedModelKey;
        const localValue = getModelValue(form, fieldKey);
        const inheritedValue =
          detail?.inheritedValues != null
            ? (getModelValue(detail.inheritedValues, fieldKey) as string | boolean | number | null)
            : null;
        const hasLocalValue = localValue != null;
        const hasInheritedValue = inheritedValue != null;
        const canReset = hasLocalValue && hasInheritedValue;
        const selectOptions =
          field.fieldType === 'select'
            ? getSelectOptions(field, typeof localValue === 'string' ? localValue : null)
            : [];

        const handleReset = () => setFieldBySharedKey(fieldKey, null);

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
              <MetadataTextField
                value={typeof localValue === 'string' ? localValue : null}
                inheritedValue={typeof inheritedValue === 'string' ? inheritedValue : null}
                onChange={(v) => setFieldBySharedKey(fieldKey, v)}
                inputClassName={styles.fieldInput}
                hintContainerClassName={styles.hintContainer}
                hintClassName={styles.hint}
                copyBtnClassName={styles.copyBtn}
              />
            )}

            {field.fieldType === 'select' && (
              <MetadataSelectField
                value={typeof localValue === 'string' ? localValue : null}
                inheritedValue={typeof inheritedValue === 'string' ? inheritedValue : null}
                options={selectOptions}
                onChange={(v) => setFieldBySharedKey(fieldKey, v)}
                selectClassName={styles.selectSmall}
                hintContainerClassName={styles.hintContainer}
                hintClassName={styles.hint}
                copyBtnClassName={styles.copyBtn}
              />
            )}

            {field.fieldType === 'bool' && (
              <MetadataBoolField
                value={typeof localValue === 'boolean' ? localValue : null}
                inheritedValue={typeof inheritedValue === 'boolean' ? inheritedValue : null}
                onChange={(v) => setFieldBySharedKey(fieldKey, v)}
                selectClassName={styles.selectSmall}
                hintContainerClassName={styles.hintContainer}
                hintClassName={styles.hint}
                copyBtnClassName={styles.copyBtn}
              />
            )}

            {field.fieldType === 'number' && (
              <>
                <TextField
                  size="small"
                  fullWidth
                  type="number"
                  value={typeof localValue === 'number' ? localValue : ''}
                  placeholder={
                    typeof inheritedValue === 'number' && Number.isFinite(inheritedValue)
                      ? String(inheritedValue)
                      : undefined
                  }
                  onChange={(e) => {
                    const raw = e.target.value.trim();
                    const parsed = raw === '' ? null : Number(raw);
                    const valid = parsed == null || (Number.isFinite(parsed) && parsed >= 0);
                    setFieldBySharedKey(fieldKey, valid ? parsed : null);
                  }}
                  InputProps={{ className: styles.fieldInput }}
                />
                <InheritedHint
                  value={typeof inheritedValue === 'number' ? inheritedValue : null}
                  className={styles.hintContainer}
                  hintClassName={styles.hint}
                  copyBtnClassName={styles.copyBtn}
                />
              </>
            )}
          </Stack>
        );
      })}

      <Stack>
        <Typography variant="caption" color="text.secondary" className={styles.fieldHeader}>
          Tags
        </Typography>
        <TagEditor
          localTags={form.tags}
          inheritedTags={detail?.inheritedValues?.tags ?? null}
          tagOptions={[
            ...(metadataDictionary?.tags.configured.map((v) => v.value) ?? []),
            ...(metadataDictionary?.tags.observed ?? []),
          ].filter((v, i, arr) => arr.indexOf(v) === i)}
          onChange={(tags) => setForm((prev) => ({ ...prev, tags }))}
        />
      </Stack>

      <Stack direction="row" className={styles.actions}>
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
        <Button
          onClick={onSave}
          variant={isDirty ? 'affirmative' : 'outlined'}
          disabled={mutation.isPending || !isDirty}
        >
          {mutation.isPending ? <CircularProgress size={16} /> : 'Save'}
        </Button>
      </Stack>
    </Stack>
  );
}
