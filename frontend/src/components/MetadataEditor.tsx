import { useState, useEffect, useRef } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import TextField from '@mui/material/TextField'
import Select from '@mui/material/Select'
import MenuItem from '@mui/material/MenuItem'
import FormControl from '@mui/material/FormControl'
import Checkbox from '@mui/material/Checkbox'
import Typography from '@mui/material/Typography'
import Button from '@mui/material/Button'
import Chip from '@mui/material/Chip'
import CircularProgress from '@mui/material/CircularProgress'
import { useDirectoryConfig, useUpdateDirectoryConfig } from '../lib/queries'
import type { MetadataFields } from '../lib/api'
import styles from './MetadataEditor.module.css'

type FieldMode = 'value' | 'rule'

interface FieldDef {
  key: keyof Omit<MetadataFields, 'fieldRules'>
  yamlName: string
  label: string
  fieldType: 'text' | 'select' | 'bool'
  options?: readonly string[]
}

const FIELDS: FieldDef[] = [
  { key: 'modelName',     yamlName: 'model_name',   label: 'Model Name',    fieldType: 'text' },
  { key: 'creator',       yamlName: 'creator',       label: 'Creator',       fieldType: 'text' },
  { key: 'collection',    yamlName: 'collection',    label: 'Collection',    fieldType: 'text' },
  { key: 'subcollection', yamlName: 'subcollection', label: 'Subcollection', fieldType: 'text' },
  { key: 'category',      yamlName: 'category',      label: 'Category',      fieldType: 'select', options: ['Bust', 'Miniature', 'Uncategorized'] },
  { key: 'type',          yamlName: 'type',          label: 'Type',          fieldType: 'select', options: ['Whole', 'Part'] },
  { key: 'supported',     yamlName: 'supported',     label: 'Supported',     fieldType: 'bool' },
] as const

function validateRuleYaml(text: string): string | null {
  const lines = text.trim().split('\n')
  let hasRule = false
  for (const line of lines) {
    const trimmed = line.trim()
    if (!trimmed || trimmed.startsWith('#')) continue
    const colonIdx = trimmed.indexOf(':')
    if (colonIdx < 0) return `Invalid YAML: expected "key: value" on line: "${trimmed}"`
    const key = trimmed.slice(0, colonIdx).trim()
    if (!key) return 'Invalid YAML: empty key'
    if (key === 'rule') hasRule = true
  }
  if (!hasRule) return 'Must include a "rule:" key (e.g. rule: filename)'
  return null
}

function InheritedHint({ value }: { value: string | boolean | null | undefined }) {
  if (value == null) return null
  return (
    <Typography variant="caption" color="text.disabled" component="p" className={styles.hint}>
      Inherited: {String(value)}
    </Typography>
  )
}

interface Props {
  path: string
  onClose?: () => void
}

export default function MetadataEditor({ path, onClose }: Props) {
  const { data: detail, isLoading } = useDirectoryConfig(path)
  const mutation = useUpdateDirectoryConfig(path)

  const [fields, setFields] = useState<MetadataFields>({
    creator: null, collection: null, subcollection: null,
    category: null, type: null, supported: null, modelName: null,
  })
  const [fieldModes, setFieldModes] = useState<Record<string, FieldMode>>({})
  const [fieldRuleTexts, setFieldRuleTexts] = useState<Record<string, string>>({})
  const [ruleErrors, setRuleErrors] = useState<Record<string, string | null>>({})
  const [savedIndicator, setSavedIndicator] = useState(false)
  const committedRef = useRef(fields)

  useEffect(() => {
    if (detail) {
      const loaded = { ...detail.localValues } as MetadataFields
      setFields(loaded)
      committedRef.current = loaded

      const modes: Record<string, FieldMode> = {}
      const ruleTexts: Record<string, string> = {}
      if (detail.localRuleFields) {
        for (const yamlName of detail.localRuleFields) {
          modes[yamlName] = 'rule'
          ruleTexts[yamlName] = detail.localRuleContents?.[yamlName] ?? ''
        }
      }
      setFieldModes(modes)
      setFieldRuleTexts(ruleTexts)
      setRuleErrors({})
    }
  }, [detail])

  function setFieldValue<K extends keyof MetadataFields>(key: K, value: MetadataFields[K]) {
    setFields(prev => ({ ...prev, [key]: value }))
  }

  function toggleMode(yamlName: string) {
    setFieldModes(prev => {
      const current = prev[yamlName] ?? 'value'
      return { ...prev, [yamlName]: current === 'value' ? 'rule' : 'value' }
    })
    setRuleErrors(prev => ({ ...prev, [yamlName]: null }))
  }

  function handleRuleChange(yamlName: string, text: string) {
    setFieldRuleTexts(prev => ({ ...prev, [yamlName]: text }))
    const error = text.trim() ? validateRuleYaml(text) : null
    setRuleErrors(prev => ({ ...prev, [yamlName]: error }))
  }

  function hasValidationErrors(): boolean {
    return FIELDS.some(f => fieldModes[f.yamlName] === 'rule' && !!ruleErrors[f.yamlName])
  }

  async function doSave(f: MetadataFields) {
    if (mutation.isPending) return

    // Validate all rule fields before saving
    const newErrors: Record<string, string | null> = {}
    let hasErrors = false
    for (const field of FIELDS) {
      if (fieldModes[field.yamlName] === 'rule') {
        const text = (fieldRuleTexts[field.yamlName] ?? '').trim()
        const error = text ? validateRuleYaml(text) : null
        newErrors[field.yamlName] = error
        if (error) hasErrors = true
      }
    }
    setRuleErrors(prev => ({ ...prev, ...newErrors }))
    if (hasErrors) return

    // Build fieldRules: only fields currently in rule mode with non-empty content
    const fieldRulesMap: Record<string, string> = {}
    for (const field of FIELDS) {
      if (fieldModes[field.yamlName] === 'rule') {
        const text = (fieldRuleTexts[field.yamlName] ?? '').trim()
        if (text) fieldRulesMap[field.yamlName] = text
      }
    }

    // Clear plain values for any field in rule mode (backend uses rule instead)
    const cleanFields = { ...f }
    for (const field of FIELDS) {
      if (fieldModes[field.yamlName] === 'rule') {
        (cleanFields as Record<string, unknown>)[field.key] = null
      }
    }

    await mutation.mutateAsync({ ...cleanFields, fieldRules: fieldRulesMap })
    committedRef.current = f
    setSavedIndicator(true)
    setTimeout(() => setSavedIndicator(false), 2000)
  }

  function handleValueCommit() {
    const c = committedRef.current
    const changed = (Object.keys(fields) as (keyof MetadataFields)[])
      .filter(k => k !== 'fieldRules')
      .some(k => fields[k] !== c[k])
    if (changed) doSave(fields)
  }

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === 'Enter') handleValueCommit()
  }

  const p = detail?.parentResolvedValues ?? null

  if (isLoading) {
    return (
      <Box className={styles.loadingBox}>
        <CircularProgress size={24} />
      </Box>
    )
  }

  return (
    <Box className={styles.form}>
      <Typography variant="subtitle2" color="text.secondary" className={styles.sectionLabel}>
        Metadata — local values override inherited ones
      </Typography>

      {FIELDS.map(field => {
        const isRuleMode = fieldModes[field.yamlName] === 'rule'
        const ruleText = fieldRuleTexts[field.yamlName] ?? ''
        const ruleError = ruleErrors[field.yamlName] ?? null
        const parentValue = p ? (p[field.key] as string | boolean | null) : null

        return (
          <Box key={field.key}>
            {/* Label + mode toggle */}
            <Stack direction="row" alignItems="center" justifyContent="space-between" className={styles.fieldHeader}>
              <Typography variant="caption" color="text.secondary">{field.label}</Typography>
              <Chip
                label={isRuleMode ? 'Rule' : 'Value'}
                size="small"
                variant={isRuleMode ? 'filled' : 'outlined'}
                color={isRuleMode ? 'warning' : 'default'}
                onClick={() => toggleMode(field.yamlName)}
                className={styles.modeChip}
              />
            </Stack>

            {/* Rule textarea */}
            {isRuleMode && (
              <TextField
                multiline
                minRows={3}
                fullWidth
                size="small"
                value={ruleText}
                onChange={e => handleRuleChange(field.yamlName, e.target.value)}
                error={!!ruleError}
                helperText={ruleError ?? undefined}
                placeholder={'rule: filename\nindex: -2'}
                slotProps={{ input: { className: styles.ruleInput } }}
              />
            )}

            {/* Text input */}
            {!isRuleMode && field.fieldType === 'text' && (
              <TextField
                size="small"
                fullWidth
                value={(fields[field.key] as string | null) ?? ''}
                placeholder={(parentValue as string | null) ?? undefined}
                onChange={e => setFieldValue(field.key, (e.target.value || null) as never)}
                onBlur={handleValueCommit}
                onKeyDown={handleKeyDown}
                slotProps={{ input: { className: styles.fieldInput } }}
              />
            )}

            {/* Select input */}
            {!isRuleMode && field.fieldType === 'select' && (
              <FormControl size="small" fullWidth>
                <Select
                  displayEmpty
                  value={(fields[field.key] as string | null) ?? ''}
                  onChange={e => {
                    const val = (e.target.value || null) as never
                    const next = { ...fields, [field.key]: val }
                    setFields(next)
                    doSave(next)
                  }}
                  className={styles.selectSmall}
                  renderValue={v => v ? String(v) : <em style={{ color: 'inherit', opacity: 0.5 }}>Not set{parentValue ? ` (${parentValue})` : ''}</em>}
                >
                  <MenuItem value=""><em>Not set</em></MenuItem>
                  {field.options?.map(o => <MenuItem key={o} value={o}>{o}</MenuItem>)}
                </Select>
              </FormControl>
            )}

            {/* Bool checkbox */}
            {!isRuleMode && field.fieldType === 'bool' && (
              <Checkbox
                size="small"
                checked={fields.supported ?? false}
                indeterminate={fields.supported == null}
                onChange={() => {
                  const val = fields.supported == null ? true : fields.supported ? false : null
                  const next = { ...fields, supported: val }
                  setFields(next)
                  doSave(next)
                }}
                className={styles.checkbox}
              />
            )}

            {!isRuleMode && <InheritedHint value={parentValue} />}
          </Box>
        )
      })}

      <Box className={styles.actions}>
        {onClose && (
          <Button size="small" onClick={onClose} className={styles.closeBtn}>
            Close
          </Button>
        )}
        <Button
          size="small"
          variant="contained"
          onClick={() => doSave(fields)}
          disabled={mutation.isPending || hasValidationErrors()}
          className={styles.saveBtn}
        >
          {mutation.isPending ? (
            <CircularProgress size={14} color="inherit" />
          ) : savedIndicator ? 'Saved!' : 'Save'}
        </Button>
      </Box>

      {mutation.isError && (
        <Typography variant="caption" color="error.main">
          Failed to save — please try again.
        </Typography>
      )}
    </Box>
  )
}
