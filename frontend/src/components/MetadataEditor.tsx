import { useState, useEffect } from 'react';
import { ChevronDown, CircleHelp, RotateCcw } from 'lucide-react';
import {
  Button,
  IconButton,
  Label,
  Menu,
  MenuContent,
  MenuItem,
  MenuLabel,
  MenuRadioGroup,
  MenuRadioItem,
  MenuSeparator,
  MenuTrigger,
  SegmentedControl,
  SegmentedControlItem,
  Input,
  Spinner,
  Textarea,
  TextLink,
  Tooltip,
} from '@jamiehales/ui-kit/mostlymaking';
import {
  useDirectoryConfig,
  useMetadataDictionaryOverview,
  useUpdateDirectoryConfig,
} from '../lib/queries';
import { ConfigValidationError } from '../lib/api';
import type { MetadataFields } from '../lib/api';
import AppDialog, { AppSheet } from './AppDialog';
import { SHARED_FIELDS, type SharedFieldDef } from './metadata/fieldDefs';
import MetadataBoolField from './metadata/MetadataBoolField';
import MetadataSelectField from './metadata/MetadataSelectField';
import MetadataTextField from './metadata/MetadataTextField';
import TagEditor from './metadata/TagEditor';
import RulePreview from './metadata/RulePreview';
import {
  applyTrim,
  RULE_CASES,
  RULE_TRIMS,
  ruleCase,
  rulePresetGroups,
  supportsCase,
  withRuleCase,
  type RuleCase,
} from './metadata/rulePresets';
import styles from './metadata/MetadataForm.module.css';

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

const TAGS_YAML_NAME = 'tags';
const DEFAULT_REGEX_RULE_PLACEHOLDER = "source: full_path\nexpression: '...'";

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
  if (hasRule && !/^\s*rule\s*:\s*regex\s*$/im.test(text)) {
    return 'Only "rule: regex" is supported. You can also omit the rule key and regex will be assumed.';
  }
  return null;
}

function getRulePlaceholder(inheritedRule?: string | null) {
  return inheritedRule?.trim() ? inheritedRule : DEFAULT_REGEX_RULE_PLACEHOLDER;
}

function RulesHelpDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const code = (text: string) => <code className={styles.code}>{text}</code>;
  const inline = (text: string) => <code className={styles.inlineCode}>{text}</code>;

  return (
    <AppDialog
      open={open}
      onClose={onClose}
      title="Rules"
      actions={
        <Button variant="ghost" onClick={onClose}>
          Close
        </Button>
      }
    >
      <div className={styles.help}>
        <p>
          Rules derive a field's value from each model's file path instead of setting a fixed value.
          Rules on a folder apply to all of its subfolders and models.
        </p>
        <p>
          Rules match against the model's path from the library root, starting with a slash, e.g.{' '}
          {inline('/Creator/Collection/model.stl')}.
        </p>
        <p>
          Rules are regex-based. The {inline('rule: regex')} key is optional; regex is assumed when
          it's left out.
        </p>
        <hr />
        <h3>rule: regex</h3>
        <p>Applies a regular expression to a value taken from the file path. Options:</p>
        <p>
          {inline('source: full_path | folder | filename')}: what to match against (default{' '}
          {inline('full_path')})
        </p>
        <p>
          {inline('expression: <pattern>')}: a regex or a sed-style substitution (
          {inline('s|pattern|replacement|flags')}). Quote patterns that start with {inline('[')} or{' '}
          {inline('{')}, e.g. {inline("expression: '[^n]supported'")}
        </p>
        <p>
          {inline('case: title | sentence | upper | lower')}: re-cases the result of{' '}
          {inline('expression')}
        </p>
        <p>
          {inline('values: { ValueKey: "pattern" }')}: for select fields, the first matching key;
          for {inline('supported')}, the first matching {inline('Supported')} or{' '}
          {inline('Unsupported')} key; for {inline('tags')}, every matching key is added as a tag
        </p>
        <p>Plain regex, returning the first capture group or the whole match:</p>
        {code("rule: regex\nexpression: '.*/([^/]+)/[^/]+$'")}
        <p>
          For {inline('Artists/Sculptor Name/dragon.stl')} this gives {inline('Sculptor Name')}.
        </p>
        <p>The same with a sed-style substitution:</p>
        {code("rule: regex\nexpression: 's|.*/([^/]+)/[^/]+$|\\1|'")}
        <p>Select field, mapping patterns to values:</p>
        {code("source: full_path\nvalues:\n  Bust: 'bust'\n  Miniature: 'mini'")}
        <p>Supported, unsupported, or unknown when neither matches:</p>
        {code("values:\n  Unsupported: 'un-?supported|nosupp'\n  Supported: 'supported'")}
        <p>Tags, adding every key whose pattern matches:</p>
        {code("source: full_path\nvalues:\n  Supported: 'supported'\n  Miniature: 'mini'")}
        <hr />
        <h3>Inheritance</h3>
        <p>
          Rules on a folder apply to everything below it until a subfolder sets its own rule for the
          same field. A plain value on a folder takes precedence over an inherited rule.
        </p>
        <hr />
        <p>
          <TextLink
            href="https://jamiehales.github.io/findamodel/rules/"
            target="_blank"
            rel="noreferrer"
          >
            Rules overview
          </TextLink>
          {' · '}
          <TextLink
            href="https://jamiehales.github.io/findamodel/rules/regex-rule.html"
            target="_blank"
            rel="noreferrer"
          >
            Regex rule reference
          </TextLink>
        </p>
      </div>
    </AppDialog>
  );
}

interface Props {
  path: string;
  open: boolean;
  onClose: () => void;
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

export default function MetadataEditor({ path, open, onClose }: Props) {
  const { data: detail, isLoading } = useDirectoryConfig(path, open);
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
  const [rulesHelpOpen, setRulesHelpOpen] = useState(false);

  useEffect(() => {
    if (detail && open) {
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
  }, [detail, open]);

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
    if (fieldModes[TAGS_YAML_NAME] === 'rule' && !!ruleErrors[TAGS_YAML_NAME]) return true;
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
    if (fieldModes[TAGS_YAML_NAME] === 'rule') {
      const tagsRuleText = (fieldRuleTexts[TAGS_YAML_NAME] ?? '').trim();
      if (tagsRuleText) currentRuleMap[TAGS_YAML_NAME] = tagsRuleText;
    }

    for (const field of FIELDS) {
      if (field.supportsRules === false) continue;
      if (fieldModes[field.yamlName] === 'rule') {
        const text = (fieldRuleTexts[field.yamlName] ?? '').trim();
        if (text) currentRuleMap[field.yamlName] = text;
      }
    }

    const originalRuleMap = normalizeRuleMap(detail.localRuleContents, detail.localRuleFields);
    if (!areRuleMapsEqual(currentRuleMap, originalRuleMap)) return true;

    if (fieldModes[TAGS_YAML_NAME] !== 'rule') {
      const currentTags = currentFields.tags ?? [];
      const originalTags = detail.localValues.tags ?? [];
      if (currentTags.length !== originalTags.length) return true;
      for (let i = 0; i < currentTags.length; i += 1) {
        if (currentTags[i] !== originalTags[i]) return true;
      }
    }

    return false;
  }

  async function doSave(f: MetadataFields) {
    if (mutation.isPending) return;

    // Validate all rule fields before saving
    const newErrors: Record<string, string | null> = {};
    let hasErrors = false;
    if (fieldModes[TAGS_YAML_NAME] === 'rule') {
      const tagsRuleText = (fieldRuleTexts[TAGS_YAML_NAME] ?? '').trim();
      const tagsRuleError = tagsRuleText ? validateRuleYaml(tagsRuleText) : null;
      newErrors[TAGS_YAML_NAME] = tagsRuleError;
      if (tagsRuleError) hasErrors = true;
    }

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
    if (fieldModes[TAGS_YAML_NAME] === 'rule') {
      const tagsRuleText = (fieldRuleTexts[TAGS_YAML_NAME] ?? '').trim();
      if (tagsRuleText) fieldRulesMap[TAGS_YAML_NAME] = tagsRuleText;
    }

    for (const field of FIELDS) {
      if (field.supportsRules === false) continue;
      if (fieldModes[field.yamlName] === 'rule') {
        const text = (fieldRuleTexts[field.yamlName] ?? '').trim();
        if (text) fieldRulesMap[field.yamlName] = text;
      }
    }

    // Clear plain values for any field in rule mode (backend uses rule instead)
    const cleanFields = { ...f };
    if (fieldModes[TAGS_YAML_NAME] === 'rule') {
      cleanFields.tags = null;
    }

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
      onClose();
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
  const isTagsRuleMode = fieldModes[TAGS_YAML_NAME] === 'rule';
  const tagsRuleText = (fieldRuleTexts[TAGS_YAML_NAME] ?? '').trim();
  const inheritedTagsRule = detail?.parentResolvedRules?.[TAGS_YAML_NAME] ?? null;
  const hasLocalTagsValue = isTagsRuleMode ? tagsRuleText !== '' : (fields.tags?.length ?? 0) > 0;
  const hasInheritedTagsValue =
    (p?.tags?.length ?? 0) > 0 || (inheritedTagsRule?.trim().length ?? 0) > 0;
  const canResetTags = hasLocalTagsValue && hasInheritedTagsValue;

  const footer = (
    <>
      {mutation.isError && (
        <span role="alert" className={`${styles.error} ${styles.footerNote}`}>
          Couldn't save. Try again.
        </span>
      )}
      <Button variant="ghost" onClick={onClose}>
        Cancel
      </Button>
      <Button
        variant="primary"
        onClick={() => doSave(fields)}
        disabled={mutation.isPending || hasValidationErrors() || !isDirty}
      >
        {mutation.isPending ? <Spinner label="Saving" /> : null}
        Save
      </Button>
    </>
  );

  function fieldTools({
    label,
    supportsRules,
    isRuleMode,
    yamlName,
    optionsField,
    canReset,
    onReset,
  }: {
    label: string;
    supportsRules: boolean;
    isRuleMode: boolean;
    yamlName: string;
    optionsField?: FieldDef['optionsField'];
    canReset: boolean;
    onReset: () => void;
  }) {
    return (
      <div className={styles.fieldTools}>
        {isRuleMode && presetsMenu(label, yamlName, optionsField)}
        {isRuleMode && (
          <Tooltip content="Rules help">
            <IconButton
              variant="ghost"
              size="sm"
              aria-label="Rules help"
              onClick={() => setRulesHelpOpen(true)}
            >
              <CircleHelp size={14} strokeWidth={1.5} />
            </IconButton>
          </Tooltip>
        )}
        {supportsRules && (
          <SegmentedControl
            aria-label={`${label} source`}
            value={isRuleMode ? 'rule' : 'value'}
            onValueChange={(next) => {
              if ((next === 'rule') !== isRuleMode) toggleMode(yamlName);
            }}
            className={styles.modeToggle}
          >
            <SegmentedControlItem value="value">Value</SegmentedControlItem>
            <SegmentedControlItem value="rule">Rule</SegmentedControlItem>
          </SegmentedControl>
        )}
        {canReset && (
          <Tooltip content="Use inherited value">
            <IconButton
              variant="ghost"
              size="sm"
              aria-label={`Reset ${label} to inherited value`}
              onClick={onReset}
            >
              <RotateCcw size={14} strokeWidth={1.5} />
            </IconButton>
          </Tooltip>
        )}
      </div>
    );
  }

  function presetsMenu(label: string, yamlName: string, optionsField?: FieldDef['optionsField']) {
    const groups = rulePresetGroups(yamlName, path, {
      selectValues: optionsField
        ? metadataDictionary?.[optionsField].configured.map((v) => v.value)
        : undefined,
      tagValues: metadataDictionary?.tags.configured.map((v) => v.value),
    });
    if (groups.length === 0) return null;
    const ruleText = fieldRuleTexts[yamlName] ?? '';

    return (
      <Menu>
        <MenuTrigger asChild>
          <Button variant="ghost" size="sm" aria-label={`${label} rule presets`}>
            Presets
            <ChevronDown size={14} strokeWidth={1.5} aria-hidden="true" />
          </Button>
        </MenuTrigger>
        <MenuContent align="end" className={styles.presetMenu}>
          {groups.map((group, i) => (
            <div key={group.label} role="group" aria-label={group.label}>
              {i > 0 && <MenuSeparator />}
              <MenuLabel>{group.label}</MenuLabel>
              {group.presets.map((preset) => (
                <MenuItem key={preset.id} onSelect={() => handleRuleChange(yamlName, preset.yaml)}>
                  {preset.label}
                  {preset.detail && <span className={styles.presetDetail}>{preset.detail}</span>}
                </MenuItem>
              ))}
            </div>
          ))}
          {groups.some((g) => g.presets.some((p) => p.id.startsWith('level-'))) && (
            <>
              <MenuSeparator />
              <MenuLabel>Tidy folder name</MenuLabel>
              {RULE_TRIMS.map((trim) => {
                const trimmed = applyTrim(ruleText, trim.id);
                return (
                  <MenuItem
                    key={trim.id}
                    disabled={trimmed == null}
                    onSelect={() => trimmed != null && handleRuleChange(yamlName, trimmed)}
                  >
                    {trim.label}
                  </MenuItem>
                );
              })}
            </>
          )}
          {supportsCase(yamlName) && (
            <>
              <MenuSeparator />
              <MenuLabel>Letter case</MenuLabel>
              <MenuRadioGroup
                value={ruleCase(ruleText)}
                onValueChange={(value) =>
                  ruleText.trim() !== '' &&
                  handleRuleChange(yamlName, withRuleCase(ruleText, value as RuleCase))
                }
              >
                {RULE_CASES.map((c) => (
                  <MenuRadioItem key={c.id} value={c.id} disabled={ruleText.trim() === ''}>
                    {c.label}
                  </MenuRadioItem>
                ))}
              </MenuRadioGroup>
            </>
          )}
        </MenuContent>
      </Menu>
    );
  }

  function ruleField(
    id: string,
    yamlName: string,
    text: string,
    error: string | null,
    inherited: string | null,
  ) {
    return (
      <>
        <Textarea
          id={id}
          rows={3}
          value={text}
          onChange={(e) => handleRuleChange(yamlName, e.target.value)}
          aria-invalid={error ? true : undefined}
          placeholder={getRulePlaceholder(inherited)}
          spellCheck={false}
          autoCorrect="off"
          autoCapitalize="off"
          className={styles.ruleInput}
        />
        {error ? (
          <p role="alert" className={styles.error}>
            {error}
          </p>
        ) : (
          <RulePreview
            path={path}
            field={yamlName}
            rule={text.trim() || (inherited ?? '')}
            enabled={open}
          />
        )}
      </>
    );
  }

  return (
    <AppSheet open={open} onClose={onClose} title="Edit metadata" footer={footer}>
      <RulesHelpDialog open={rulesHelpOpen} onClose={() => setRulesHelpOpen(false)} />
      {isLoading ? (
        <div className={styles.form}>
          <Spinner />
        </div>
      ) : (
        <div className={styles.form}>
          <div className={styles.field}>
            <div className={styles.fieldHeader}>
              <Label htmlFor="folder-meta-tags">Tags</Label>
              {fieldTools({
                label: 'Tags',
                supportsRules: true,
                isRuleMode: isTagsRuleMode,
                yamlName: TAGS_YAML_NAME,
                canReset: canResetTags,
                onReset: () => {
                  setFieldValue('tags', null);
                  if (isTagsRuleMode) {
                    setFieldModes((prev) => ({ ...prev, [TAGS_YAML_NAME]: 'value' }));
                    setFieldRuleTexts((prev) => ({ ...prev, [TAGS_YAML_NAME]: '' }));
                  }
                },
              })}
            </div>
            {isTagsRuleMode ? (
              ruleField(
                'folder-meta-tags',
                TAGS_YAML_NAME,
                fieldRuleTexts[TAGS_YAML_NAME] ?? '',
                ruleErrors[TAGS_YAML_NAME] ?? null,
                inheritedTagsRule,
              )
            ) : (
              <TagEditor
                id="folder-meta-tags"
                localTags={fields.tags}
                inheritedTags={p?.tags ?? null}
                tagOptions={[
                  ...(metadataDictionary?.tags.configured.map((v) => v.value) ?? []),
                  ...(metadataDictionary?.tags.observed ?? []),
                ].filter((v, i, arr) => arr.indexOf(v) === i)}
                onChange={(tags) => setFieldValue('tags', tags)}
              />
            )}
          </div>

          {FIELDS.map((field) => {
            const id = `folder-meta-${field.key}`;
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

            const handleReset = () => {
              setFieldValue(field.key, null as never);
              if (isRuleMode) {
                setFieldModes((prev) => ({ ...prev, [field.yamlName]: 'value' }));
                setFieldRuleTexts((prev) => ({ ...prev, [field.yamlName]: '' }));
              }
            };

            return (
              <div key={field.key} className={styles.field}>
                <div className={styles.fieldHeader}>
                  <Label htmlFor={id}>{field.label}</Label>
                  {fieldTools({
                    label: field.label,
                    supportsRules,
                    isRuleMode,
                    yamlName: field.yamlName,
                    optionsField: field.optionsField,
                    canReset: hasLocalValue && hasInheritedValue,
                    onReset: handleReset,
                  })}
                </div>

                {isRuleMode && ruleField(id, field.yamlName, ruleText, ruleError, inheritedRule)}

                {!isRuleMode && field.fieldType === 'text' && (
                  <MetadataTextField
                    id={id}
                    value={(fields[field.key] as string | null) ?? null}
                    inheritedValue={(parentValue as string | null) ?? null}
                    onChange={(v) => setFieldValue(field.key, v as never)}
                  />
                )}

                {!isRuleMode && field.fieldType === 'select' && (
                  <MetadataSelectField
                    id={id}
                    value={(fields[field.key] as string | null) ?? null}
                    inheritedValue={(parentValue as string | null) ?? null}
                    options={selectOptions}
                    onChange={(v) => setFieldValue(field.key, v as never)}
                  />
                )}

                {!isRuleMode && field.fieldType === 'bool' && (
                  <MetadataBoolField
                    id={id}
                    value={(fields[field.key] as boolean | null) ?? null}
                    inheritedValue={(parentValue as boolean | null) ?? null}
                    onChange={(v) => setFieldValue(field.key, v as never)}
                  />
                )}

                {!isRuleMode && field.fieldType === 'number' && (
                  <Input
                    id={id}
                    type="number"
                    min={0}
                    step="any"
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
                  />
                )}
              </div>
            );
          })}
        </div>
      )}
    </AppSheet>
  );
}
