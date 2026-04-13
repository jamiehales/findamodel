import { useState, useEffect } from 'react';
import Box from '@mui/material/Box';
import Stack from '@mui/material/Stack';
import TextField from '@mui/material/TextField';
import Typography from '@mui/material/Typography';
import Button from '@mui/material/Button';
import Chip from '@mui/material/Chip';
import CircularProgress from '@mui/material/CircularProgress';
import IconButton from '@mui/material/IconButton';
import Tooltip from '@mui/material/Tooltip';
import Divider from '@mui/material/Divider';
import {
  useDirectoryConfig,
  useMetadataDictionaryOverview,
  useUpdateDirectoryConfig,
} from '../lib/queries';
import { ConfigValidationError } from '../lib/api';
import type { MetadataFields } from '../lib/api';
import AppDialog from './AppDialog';
import { SHARED_FIELDS, type SharedFieldDef } from './metadata/fieldDefs';
import InheritedHint from './metadata/InheritedHint';
import MetadataBoolField from './metadata/MetadataBoolField';
import MetadataSelectField from './metadata/MetadataSelectField';
import MetadataTextField from './metadata/MetadataTextField';
import TagEditor from './metadata/TagEditor';
import styles from './MetadataEditor.module.css';

type FieldMode = 'value' | 'rule';

interface FieldDef {
  key: keyof Omit<MetadataFields, 'fieldRules'>;
  yamlName: string;
  label: string;
  fieldType: 'text' | 'select' | 'bool' | 'number';
  optionsField?: 'category' | 'type' | 'material';
  supportsRules?: boolean;
}

const FIELDS: FieldDef[] = SHARED_FIELDS.filter(
  (f): f is SharedFieldDef & { yamlName: string } => f.yamlName != null,
).map((f) => ({
  key: f.key as keyof Omit<MetadataFields, 'fieldRules'>,
  yamlName: f.yamlName,
  label: f.label,
  fieldType: f.fieldType,
  optionsField: f.optionsField,
  supportsRules: f.supportsRules,
}));

function validateRuleYaml(text: string): string | null {
  const lines = text.trim().split('\n');
  let hasRule = false;
  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith('#')) continue;
    const colonIdx = trimmed.indexOf(':');
    if (colonIdx < 0) return `Invalid YAML: expected "key: value" on line: "${trimmed}"`;
    const key = trimmed.slice(0, colonIdx).trim();
    if (!key) return 'Invalid YAML: empty key';
    if (key === 'rule') hasRule = true;
    const value = trimmed.slice(colonIdx + 1).trim();
    if (value && !value.startsWith("'") && !value.startsWith('"')) {
      if (value.startsWith('[') || value.startsWith('{')) {
        return `Value "${value}" must be quoted because it starts with a YAML special character - wrap it in single quotes, e.g. '${value}'`;
      }
    }
  }
  if (!hasRule) return 'Must include a "rule:" key (e.g. rule: filename)';
  return null;
}

function RulesHelpDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const code = (text: string) => (
    <Box
      component="code"
      sx={{
        display: 'block',
        fontFamily: 'monospace',
        fontSize: '0.78rem',
        bgcolor: 'action.hover',
        borderRadius: 1,
        px: 1.5,
        py: 1,
        my: 0.5,
        whiteSpace: 'pre',
        overflowX: 'auto',
      }}
    >
      {text}
    </Box>
  );
  const inline = (text: string) => (
    <Box
      component="code"
      sx={{
        fontFamily: 'monospace',
        fontSize: '0.8em',
        bgcolor: 'action.hover',
        borderRadius: '3px',
        px: 0.6,
        py: 0.1,
      }}
    >
      {text}
    </Box>
  );
  const section = (title: string) => (
    <Typography variant="subtitle2" sx={{ mt: 2, mb: 0.5, fontWeight: 600 }}>
      {title}
    </Typography>
  );
  const para = (text: React.ReactNode) => (
    <Typography variant="body2" color="text.primary" sx={{ mb: 0.5 }}>
      {text}
    </Typography>
  );

  return (
    <AppDialog
      open={open}
      onClose={onClose}
      title="Rules System"
      maxWidth="sm"
      fullWidth
      scroll="paper"
      contentDividers
      actions={
        <Button size="small" onClick={onClose}>
          Close
        </Button>
      }
    >
      {para(
        "Rules let you automatically derive a field's value from each model's file path, rather than setting a fixed value. Rules defined on a folder are inherited by all subfolders and models within it.",
      )}
      {para(
        <>
          A rule is written in YAML. The only required key is {inline('rule:')} which names the rule
          type. All other keys are options specific to that rule.
        </>,
      )}

      <Divider sx={{ my: 1.5 }} />

      {section('rule: filename')}
      {para(
        "Sets the field to the model's filename, title-cased and without extension by default.",
      )}
      <Typography variant="caption" color="text.primary" sx={{ display: 'block', mb: 0.5 }}>
        Options:
      </Typography>
      {para(<>{inline('include_extension: true')} - include the file extension in the value</>)}

      <Typography variant="caption" color="text.primary" sx={{ display: 'block', mt: 1, mb: 0.25 }}>
        Examples:
      </Typography>
      {code('rule: filename')}
      {para(
        <>
          A file named {inline('my-dragon.stl')} → {inline('My-Dragon')}
        </>,
      )}
      {code('rule: filename\ninclude_extension: true')}
      {para(
        <>
          A file named {inline('my-dragon.stl')} → {inline('My-Dragon.stl')}
        </>,
      )}

      <Divider sx={{ my: 1.5 }} />

      {section('rule: regex')}
      {para('Applies a regular expression to a value derived from the file path.')}
      <Typography variant="caption" color="text.primary" sx={{ display: 'block', mb: 0.5 }}>
        Options:
      </Typography>
      {para(
        <>
          {inline('source: full_path | folder | filename')} - what to match against (default:{' '}
          {inline('full_path')})
        </>,
      )}
      {para(
        <>
          {inline('expression: <pattern>')} - a regex or sed-style substitution (
          {inline('s|pattern|replacement|flags')}). Quote patterns that start with {inline('[')} or{' '}
          {inline('{')} (e.g. {inline("expression: '[^n]supported'")})
        </>,
      )}
      {para(
        <>
          {inline('values: \{ EnumValue: "pattern" \}')} - for select fields: try each pattern in
          order, return the matching key
        </>,
      )}

      <Typography variant="caption" color="text.primary" sx={{ display: 'block', mt: 1, mb: 0.25 }}>
        Plain regex - returns first capture group, or full match:
      </Typography>
      {code("rule: regex\nsource: folder\nexpression: '([^/]+)/[^/]+$'")}
      {para(
        <>
          For a file at {inline('Artists/Sculptor Name/dragon.stl')} → {inline('Sculptor Name')}
        </>,
      )}

      <Typography variant="caption" color="text.primary" sx={{ display: 'block', mt: 1, mb: 0.25 }}>
        Sed-style substitution - transform the matched value:
      </Typography>
      {code("rule: regex\nsource: folder\nexpression: 's|.*/([^/]+)/[^/]+$|\\1|'")}
      {para(<>Same result as above but using a substitution expression</>)}

      <Typography variant="caption" color="text.primary" sx={{ display: 'block', mt: 1, mb: 0.25 }}>
        Enum (select) field - map patterns to values:
      </Typography>
      {code("rule: regex\nsource: full_path\nvalues:\n  Bust: '(?i)bust'\n  Miniature: '(?i)mini'")}
      {para('Returns the first key whose pattern matches the path.')}

      <Divider sx={{ my: 1.5 }} />

      {section('Inheritance')}
      {para(
        "Rules set on a parent folder cascade down to all subfolders and models unless overridden. When a subfolder defines its own rule for the same field, it replaces the parent's rule for that subtree.",
      )}
      {para(
        'If a folder has a plain value set for a field, that value takes precedence over any inherited rule.',
      )}
    </AppDialog>
  );
}

interface Props {
  path: string;
  onClose?: () => void;
}

function normalizeRuleMap(
  ruleContents: Record<string, string> | null | undefined,
  localRuleFields: string[] | null | undefined,
) {
  const normalized: Record<string, string> = {};
  for (const yamlName of localRuleFields ?? []) {
    const text = (ruleContents?.[yamlName] ?? '').trim();
    if (text) normalized[yamlName] = text;
  }
  return normalized;
}

function areRuleMapsEqual(a: Record<string, string>, b: Record<string, string>) {
  const aKeys = Object.keys(a).sort();
  const bKeys = Object.keys(b).sort();
  if (aKeys.length !== bKeys.length) return false;
  for (let i = 0; i < aKeys.length; i += 1) {
    const key = aKeys[i];
    if (key !== bKeys[i]) return false;
    if (a[key] !== b[key]) return false;
  }
  return true;
}

export default function MetadataEditor({ path, onClose }: Props) {
  const { data: detail, isLoading } = useDirectoryConfig(path);
  const { data: metadataDictionary } = useMetadataDictionaryOverview();
  const mutation = useUpdateDirectoryConfig(path);

  const [fields, setFields] = useState<MetadataFields>({
    creator: null,
    collection: null,
    subcollection: null,
    tags: null,
    category: null,
    type: null,
    material: null,
    supported: null,
    raftHeightMm: null,
    modelName: null,
    partName: null,
  });
  const [fieldModes, setFieldModes] = useState<Record<string, FieldMode>>({});
  const [fieldRuleTexts, setFieldRuleTexts] = useState<Record<string, string>>({});
  const [ruleErrors, setRuleErrors] = useState<Record<string, string | null>>({});
  const [savedIndicator, setSavedIndicator] = useState(false);
  const [rulesHelpOpen, setRulesHelpOpen] = useState(false);

  useEffect(() => {
    if (detail) {
      const loaded = { ...detail.localValues } as MetadataFields;
      setFields(loaded);

      const modes: Record<string, FieldMode> = {};
      const ruleTexts: Record<string, string> = {};
      if (detail.localRuleFields) {
        for (const yamlName of detail.localRuleFields) {
          modes[yamlName] = 'rule';
          ruleTexts[yamlName] = detail.localRuleContents?.[yamlName] ?? '';
        }
      }
      // Also set mode to 'rule' for fields with inherited rules (if not already set locally)
      if (detail.parentResolvedRules) {
        for (const yamlName of Object.keys(detail.parentResolvedRules)) {
          if (!(yamlName in modes)) {
            modes[yamlName] = 'rule';
          }
        }
      }
      setFieldModes(modes);
      setFieldRuleTexts(ruleTexts);
      setRuleErrors({});
    }
  }, [detail]);

  function setFieldValue<K extends keyof MetadataFields>(key: K, value: MetadataFields[K]) {
    setFields((prev) => ({ ...prev, [key]: value }));
  }

  function toggleMode(yamlName: string) {
    setFieldModes((prev) => {
      const current = prev[yamlName] ?? 'value';
      return { ...prev, [yamlName]: current === 'value' ? 'rule' : 'value' };
    });
    setRuleErrors((prev) => ({ ...prev, [yamlName]: null }));
  }

  function handleRuleChange(yamlName: string, text: string) {
    setFieldRuleTexts((prev) => ({ ...prev, [yamlName]: text }));
    const error = text.trim() ? validateRuleYaml(text) : null;
    setRuleErrors((prev) => ({ ...prev, [yamlName]: error }));
  }

  function hasValidationErrors(): boolean {
    return FIELDS.some((f) => fieldModes[f.yamlName] === 'rule' && !!ruleErrors[f.yamlName]);
  }

  function hasChanges(currentFields: MetadataFields): boolean {
    if (!detail) return false;

    for (const field of FIELDS) {
      const current = currentFields[field.key] as string | boolean | number | null;
      const original = detail.localValues[field.key] as string | boolean | number | null;
      if (current !== original) return true;
    }

    const currentRuleMap: Record<string, string> = {};
    for (const field of FIELDS) {
      if (field.supportsRules === false) continue;
      if (fieldModes[field.yamlName] === 'rule') {
        const text = (fieldRuleTexts[field.yamlName] ?? '').trim();
        if (text) currentRuleMap[field.yamlName] = text;
      }
    }

    const originalRuleMap = normalizeRuleMap(detail.localRuleContents, detail.localRuleFields);
    if (!areRuleMapsEqual(currentRuleMap, originalRuleMap)) return true;

    const currentTags = currentFields.tags ?? [];
    const originalTags = detail.localValues.tags ?? [];
    if (currentTags.length !== originalTags.length) return true;
    for (let i = 0; i < currentTags.length; i += 1) {
      if (currentTags[i] !== originalTags[i]) return true;
    }

    return false;
  }

  async function doSave(f: MetadataFields) {
    if (mutation.isPending) return;

    // Validate all rule fields before saving
    const newErrors: Record<string, string | null> = {};
    let hasErrors = false;
    for (const field of FIELDS) {
      if (field.supportsRules === false) continue;
      if (fieldModes[field.yamlName] === 'rule') {
        const text = (fieldRuleTexts[field.yamlName] ?? '').trim();
        const error = text ? validateRuleYaml(text) : null;
        newErrors[field.yamlName] = error;
        if (error) hasErrors = true;
      }
    }
    setRuleErrors((prev) => ({ ...prev, ...newErrors }));
    if (hasErrors) return;

    // Build fieldRules: only fields currently in rule mode with non-empty content
    const fieldRulesMap: Record<string, string> = {};
    for (const field of FIELDS) {
      if (field.supportsRules === false) continue;
      if (fieldModes[field.yamlName] === 'rule') {
        const text = (fieldRuleTexts[field.yamlName] ?? '').trim();
        if (text) fieldRulesMap[field.yamlName] = text;
      }
    }

    // Clear plain values for any field in rule mode (backend uses rule instead)
    const cleanFields = { ...f };
    for (const field of FIELDS) {
      if (field.supportsRules === false) continue;
      if (fieldModes[field.yamlName] === 'rule') {
        (cleanFields as Record<string, unknown>)[field.key] = null;
      }
    }

    try {
      await mutation.mutateAsync({
        ...cleanFields,
        tags: cleanFields.tags ?? null,
        fieldRules: fieldRulesMap,
      });
      setSavedIndicator(true);
      setTimeout(() => setSavedIndicator(false), 2000);
      onClose?.();
    } catch (err) {
      if (err instanceof ConfigValidationError) {
        setRuleErrors((prev) => ({ ...prev, ...err.fieldErrors }));
      }
    }
  }

  function getSelectOptions(
    field: FieldDef,
    localValue: string | boolean | number | null,
  ): string[] {
    if (field.optionsField == null) return [];

    const configured =
      metadataDictionary?.[field.optionsField].configured.map((v) => v.value) ?? [];
    const values = [...configured];
    const local = typeof localValue === 'string' ? localValue : null;
    if (local && !values.includes(local)) values.push(local);
    return values;
  }

  const p = detail?.parentResolvedValues ?? null;
  const isDirty = hasChanges(fields);

  if (isLoading) {
    return (
      <Box className={styles.loadingBox}>
        <CircularProgress size={24} />
      </Box>
    );
  }

  return (
    <Box className={styles.form}>
      <RulesHelpDialog open={rulesHelpOpen} onClose={() => setRulesHelpOpen(false)} />

      <Box>
        <Stack
          direction="row"
          alignItems="center"
          justifyContent="space-between"
          className={styles.fieldHeader}
        >
          <Typography variant="caption" color="text.secondary">
            Tags
          </Typography>
        </Stack>
        <TagEditor
          localTags={fields.tags}
          inheritedTags={p?.tags ?? null}
          tagOptions={[
            ...(metadataDictionary?.tags.configured.map((v) => v.value) ?? []),
            ...(metadataDictionary?.tags.observed ?? []),
          ].filter((v, i, arr) => arr.indexOf(v) === i)}
          onChange={(tags) => setFieldValue('tags', tags)}
        />
      </Box>

      {FIELDS.map((field) => {
        const supportsRules = field.supportsRules !== false;
        const isRuleMode = supportsRules && fieldModes[field.yamlName] === 'rule';
        const ruleText = fieldRuleTexts[field.yamlName] ?? '';
        const ruleError = ruleErrors[field.yamlName] ?? null;
        const parentValue = p ? (p[field.key] as string | boolean | number | null) : null;
        const inheritedRule = detail?.parentResolvedRules?.[field.yamlName] ?? null;
        const localValue = fields[field.key] as string | boolean | number | null;
        const selectOptions = getSelectOptions(field, localValue);
        const hasLocalValue = isRuleMode ? ruleText.trim() !== '' : localValue != null;
        const hasInheritedValue = parentValue != null || inheritedRule != null;
        const canReset = hasLocalValue && hasInheritedValue;

        const handleReset = () => {
          // Reset the field to default (null)
          setFieldValue(field.key, null as never);
          // Also clear rule mode if active
          if (isRuleMode) {
            setFieldModes((prev) => ({ ...prev, [field.yamlName]: 'value' }));
            setFieldRuleTexts((prev) => ({ ...prev, [field.yamlName]: '' }));
          }
          // Save the cleared state
          setFields((prev) => ({ ...prev, [field.key]: null }));
        };

        return (
          <Box key={field.key}>
            {/* Label + mode toggle + reset button */}
            <Stack
              direction="row"
              alignItems="center"
              justifyContent="space-between"
              className={styles.fieldHeader}
            >
              <Typography variant="caption" color="text.secondary">
                {field.label}
              </Typography>
              <Stack direction="row" alignItems="center" gap={1}>
                {isRuleMode && (
                  <Tooltip title="Rules documentation">
                    <IconButton
                      size="small"
                      onClick={() => setRulesHelpOpen(true)}
                      className={styles.helpBtn}
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
                        <path d="M11 18h2v-2h-2v2zm1-16C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 18c-4.41 0-8-3.59-8-8s3.59-8 8-8 8 3.59 8 8-3.59 8-8 8zm0-14c-2.21 0-4 1.79-4 4h2c0-1.1.9-2 2-2s2 .9 2 2c0 2-3 1.75-3 5h2c0-2.25 3-2.5 3-5 0-2.21-1.79-4-4-4z" />
                      </svg>
                    </IconButton>
                  </Tooltip>
                )}
                <Chip
                  label={isRuleMode ? 'Rule' : 'Value'}
                  size="small"
                  variant={isRuleMode ? 'filled' : 'outlined'}
                  color={isRuleMode ? 'warning' : 'default'}
                  onClick={supportsRules ? () => toggleMode(field.yamlName) : undefined}
                  className={styles.modeChip}
                  clickable={supportsRules}
                />
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
                        <path d="M7 10c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2zm10-8c-4.4 0-8.3 2.5-10.2 6.1C4.8 7 2 9.5 2 12.5c0 3.3 2.7 6 6 6 1 0 1.9-.2 2.8-.7 1.3 1.3 3.1 2.1 5.2 2.1 4.4 0 8-3.6 8-8s-3.6-8-8-8zM8 16c-2.2 0-4-1.8-4-4s1.8-4 4-4 4 1.8 4 4-1.8 4-4 4zm12-8c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2z" />
                      </svg>
                    </IconButton>
                  </span>
                </Tooltip>
              </Stack>
            </Stack>

            {/* Rule textarea */}
            {isRuleMode && (
              <TextField
                multiline
                minRows={3}
                fullWidth
                size="small"
                value={ruleText}
                onChange={(e) => handleRuleChange(field.yamlName, e.target.value)}
                error={!!ruleError}
                helperText={ruleError ?? undefined}
                placeholder={'rule: filename\nindex: -2'}
                slotProps={{ input: { className: styles.ruleInput } }}
              />
            )}

            {/* Text input */}
            {!isRuleMode && field.fieldType === 'text' && (
              <MetadataTextField
                value={(fields[field.key] as string | null) ?? null}
                inheritedValue={(parentValue as string | null) ?? null}
                onChange={(v) => setFieldValue(field.key, v as never)}
                inputClassName={styles.fieldInput}
                hintContainerClassName={styles.hintContainer}
                hintClassName={styles.hint}
                copyBtnClassName={styles.copyBtn}
              />
            )}

            {/* Select input */}
            {!isRuleMode && field.fieldType === 'select' && (
              <MetadataSelectField
                value={(fields[field.key] as string | null) ?? null}
                inheritedValue={(parentValue as string | null) ?? null}
                options={selectOptions}
                onChange={(v) => setFieldValue(field.key, v as never)}
                selectClassName={styles.selectSmall}
                hintContainerClassName={styles.hintContainer}
                hintClassName={styles.hint}
                copyBtnClassName={styles.copyBtn}
              />
            )}

            {/* Bool input */}
            {!isRuleMode && field.fieldType === 'bool' && (
              <MetadataBoolField
                value={(fields[field.key] as boolean | null) ?? null}
                inheritedValue={(parentValue as boolean | null) ?? null}
                onChange={(v) => setFieldValue(field.key, v as never)}
                selectClassName={styles.selectSmall}
                hintContainerClassName={styles.hintContainer}
                hintClassName={styles.hint}
                copyBtnClassName={styles.copyBtn}
              />
            )}

            {!isRuleMode && field.fieldType === 'number' && (
              <TextField
                size="small"
                fullWidth
                type="number"
                value={fields.raftHeightMm ?? ''}
                placeholder={
                  typeof parentValue === 'number' && Number.isFinite(parentValue)
                    ? String(parentValue)
                    : undefined
                }
                onChange={(e) => {
                  const raw = e.target.value.trim();
                  const parsed = raw === '' ? null : Number(raw);
                  const isValid = parsed != null && Number.isFinite(parsed) && parsed >= 0;
                  setFieldValue(
                    field.key,
                    raw === '' || isValid ? (parsed as never) : (null as never),
                  );
                }}
                InputProps={{ className: styles.fieldInput }}
              />
            )}

            {isRuleMode ? (
              <InheritedHint
                inheritedRule={inheritedRule}
                className={styles.hintContainer}
                hintClassName={styles.hint}
                copyBtnClassName={styles.copyBtn}
              />
            ) : (
              field.fieldType === 'number' && (
                <InheritedHint
                  value={parentValue}
                  className={styles.hintContainer}
                  hintClassName={styles.hint}
                  copyBtnClassName={styles.copyBtn}
                />
              )
            )}
          </Box>
        );
      })}

      <Box className={styles.actions}>
        {mutation.isError && (
          <Typography variant="caption" color="error.main" className={styles.inlineError}>
            Failed to save - please try again.
          </Typography>
        )}
        {onClose && <Button onClick={onClose}>Close</Button>}
        <Button
          variant={isDirty ? 'affirmative' : 'outlined'}
          onClick={() => doSave(fields)}
          disabled={mutation.isPending || hasValidationErrors() || !isDirty}
          className={styles.saveBtn}
        >
          {mutation.isPending ? (
            <CircularProgress size={14} color="inherit" />
          ) : savedIndicator ? (
            'Saved!'
          ) : (
            'Save'
          )}
        </Button>
      </Box>
    </Box>
  );
}
