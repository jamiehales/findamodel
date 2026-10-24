import { useEffect, useState } from 'react';
import { RotateCcw } from 'lucide-react';
import {
  Button,
  IconButton,
  Input,
  Label,
  Spinner,
  Tooltip,
} from '@jamiehales/ui-kit/mostlymaking';
import type { Model, ModelMetadata } from '../lib/api';
import { SHARED_FIELDS, type SharedFieldDef } from './metadata/fieldDefs';
import MetadataBoolField from './metadata/MetadataBoolField';
import MetadataSelectField from './metadata/MetadataSelectField';
import MetadataTextField from './metadata/MetadataTextField';
import TagEditor from './metadata/TagEditor';
import {
  useMetadataDictionaryOverview,
  useModelMetadata,
  useUpdateModelMetadata,
} from '../lib/queries';
import { AppSheet } from './AppDialog';
import styles from './metadata/MetadataForm.module.css';

interface Props {
  model: Model;
  open: boolean;
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

export default function ModelMetadataEditor({ model, open, onClose }: Props) {
  const mutation = useUpdateModelMetadata(model.id);
  const { data: detail } = useModelMetadata(model.id);
  const { data: metadataDictionary } = useMetadataDictionaryOverview();
  const [error, setError] = useState<string | null>(null);

  const [form, setForm] = useState<FormState>(EMPTY_FORM);
  const [initialForm, setInitialForm] = useState<FormState>(EMPTY_FORM);

  useEffect(() => {
    if (detail && open) {
      const hydrated: FormState = {
        ...detail.localValues,
      };
      setForm(hydrated);
      setInitialForm(hydrated);
      setError(null);
    }
  }, [detail, open]);

  function setFieldBySharedKey(key: SharedModelKey, value: string | boolean | number | null) {
    const modelKey = toModelKey(key);
    setForm((prev) => ({ ...prev, [modelKey]: value }));
  }

  async function onSave() {
    if (mutation.isPending || !detail) return;
    setError(null);

    try {
      await mutation.mutateAsync({
        name: form.name,
        partName: form.partName,
        creator: form.creator,
        collection: form.collection,
        subcollection: form.subcollection,
        tags: form.tags ?? null,
        category: form.category,
        type: form.type,
        material: form.material,
        supported: form.supported,
        raftHeightMm: form.raftHeightMm,
      });
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Couldn't save. Try again.");
    }
  }

  const isDirty = detail ? hasFormChanges(form, initialForm) : false;

  function getSelectOptions(field: SharedFieldDef, localValue: string | null): string[] {
    if (field.optionsField == null) return [];
    const configured =
      metadataDictionary?.[field.optionsField].configured.map((v) => v.value) ?? [];
    const values = [...configured];
    if (localValue && !values.includes(localValue)) values.push(localValue);
    return values;
  }

  return (
    <AppSheet
      open={open}
      onClose={onClose}
      title="Edit metadata"
      footer={
        <>
          {error && (
            <span role="alert" className={`${styles.error} ${styles.footerNote}`}>
              {error}
            </span>
          )}
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={onSave} disabled={mutation.isPending || !isDirty}>
            {mutation.isPending ? <Spinner label="Saving" /> : null}
            Save
          </Button>
        </>
      }
    >
      <div className={styles.form}>
        {MODEL_FIELDS.map((field) => {
          const fieldKey = field.key as SharedModelKey;
          const id = `model-meta-${field.key}`;
          const localValue = getModelValue(form, fieldKey);
          const inheritedValue =
            detail?.inheritedValues != null
              ? (getModelValue(detail.inheritedValues, fieldKey) as
                  | string
                  | boolean
                  | number
                  | null)
              : null;
          const canReset = localValue != null && inheritedValue != null;
          const selectOptions =
            field.fieldType === 'select'
              ? getSelectOptions(field, typeof localValue === 'string' ? localValue : null)
              : [];

          return (
            <div key={field.key} className={styles.field}>
              <div className={styles.fieldHeader}>
                <Label htmlFor={id}>{field.label}</Label>
                {canReset && (
                  <Tooltip content="Use inherited value">
                    <IconButton
                      variant="ghost"
                      size="sm"
                      aria-label={`Reset ${field.label} to inherited value`}
                      onClick={() => setFieldBySharedKey(fieldKey, null)}
                    >
                      <RotateCcw size={14} strokeWidth={1.5} />
                    </IconButton>
                  </Tooltip>
                )}
              </div>

              {field.fieldType === 'text' && (
                <MetadataTextField
                  id={id}
                  value={typeof localValue === 'string' ? localValue : null}
                  inheritedValue={typeof inheritedValue === 'string' ? inheritedValue : null}
                  onChange={(v) => setFieldBySharedKey(fieldKey, v)}
                />
              )}

              {field.fieldType === 'select' && (
                <MetadataSelectField
                  id={id}
                  value={typeof localValue === 'string' ? localValue : null}
                  inheritedValue={typeof inheritedValue === 'string' ? inheritedValue : null}
                  options={selectOptions}
                  onChange={(v) => setFieldBySharedKey(fieldKey, v)}
                />
              )}

              {field.fieldType === 'bool' && (
                <MetadataBoolField
                  id={id}
                  value={typeof localValue === 'boolean' ? localValue : null}
                  inheritedValue={typeof inheritedValue === 'boolean' ? inheritedValue : null}
                  onChange={(v) => setFieldBySharedKey(fieldKey, v)}
                />
              )}

              {field.fieldType === 'number' && (
                <Input
                  id={id}
                  type="number"
                  min={0}
                  step="any"
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
                />
              )}
            </div>
          );
        })}

        <div className={styles.field}>
          <Label htmlFor="model-meta-tags">Tags</Label>
          <TagEditor
            id="model-meta-tags"
            localTags={form.tags}
            inheritedTags={detail?.inheritedValues?.tags ?? null}
            tagOptions={[
              ...(metadataDictionary?.tags.configured.map((v) => v.value) ?? []),
              ...(metadataDictionary?.tags.observed ?? []),
            ].filter((v, i, arr) => arr.indexOf(v) === i)}
            onChange={(tags) => setForm((prev) => ({ ...prev, tags }))}
          />
        </div>
      </div>
    </AppSheet>
  );
}
