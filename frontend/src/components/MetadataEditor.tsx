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
import IconButton from '@mui/material/IconButton'
import Tooltip from '@mui/material/Tooltip'
import Dialog from '@mui/material/Dialog'
import DialogTitle from '@mui/material/DialogTitle'
import DialogContent from '@mui/material/DialogContent'
import DialogActions from '@mui/material/DialogActions'
import Divider from '@mui/material/Divider'
import { useDirectoryConfig, useUpdateDirectoryConfig } from '../lib/queries'
import { ConfigValidationError } from '../lib/api'
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

function InheritedHint({ value, inheritedRule }: { value?: string | boolean | null | undefined; inheritedRule?: string | null }) {
  const [copied, setCopied] = useState(false)

  if (value == null && !inheritedRule) return null

  const textToCopy = inheritedRule ?? String(value)

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(textToCopy)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    } catch (err) {
      console.error('Failed to copy to clipboard:', err)
    }
  }

  return (
    <Stack direction="row" alignItems="flex-start" spacing={1} className={styles.hintContainer}>
      <Typography variant="caption" color="text.disabled" component="div" className={styles.hint}>
        {inheritedRule ? (
          <>
            Inherited rule:<br />
            <code style={{ display: 'block', marginTop: '4px', fontFamily: 'monospace', fontSize: '0.75rem', whiteSpace: 'pre-wrap', wordBreak: 'break-word' }}>{inheritedRule}</code>
          </>
        ) : (
          <>Inherited: {String(value)}</>
        )}
      </Typography>
      <Tooltip title={copied ? 'Copied!' : 'Copy to clipboard'}>
        <IconButton
          size="small"
          onClick={handleCopy}
          className={styles.copyBtn}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
            <path d="M16 1H4c-1.1 0-2 .9-2 2v14h2V3h12V1zm3 4H8c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h11c1.1 0 2-.9 2-2V7c0-1.1-.9-2-2-2zm0 16H8V7h11v14z"/>
          </svg>
        </IconButton>
      </Tooltip>
    </Stack>
  )
}

function RulesHelpDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const code = (text: string) => (
    <Box component="code" sx={{ display: 'block', fontFamily: 'monospace', fontSize: '0.78rem', bgcolor: 'action.hover', borderRadius: 1, px: 1.5, py: 1, my: 0.5, whiteSpace: 'pre', overflowX: 'auto' }}>
      {text}
    </Box>
  )
  const inline = (text: string) => (
    <Box component="code" sx={{ fontFamily: 'monospace', fontSize: '0.8em', bgcolor: 'action.hover', borderRadius: '3px', px: 0.6, py: 0.1 }}>
      {text}
    </Box>
  )
  const section = (title: string) => (
    <Typography variant="subtitle2" sx={{ mt: 2, mb: 0.5, fontWeight: 600 }}>{title}</Typography>
  )
  const para = (text: React.ReactNode) => (
    <Typography variant="body2" color="text.primary" sx={{ mb: 0.5 }}>{text}</Typography>
  )

  return (
    <Dialog open={open} onClose={onClose} maxWidth="sm" fullWidth scroll="paper">
      <DialogTitle sx={{ pb: 1 }}>Rules System</DialogTitle>
      <DialogContent dividers>
        {para('Rules let you automatically derive a field\'s value from each model\'s file path, rather than setting a fixed value. Rules defined on a folder are inherited by all subfolders and models within it.')}
        {para(<>A rule is written in YAML. The only required key is {inline('rule:')} which names the rule type. All other keys are options specific to that rule.</>)}

        <Divider sx={{ my: 1.5 }} />

        {section('rule: filename')}
        {para('Sets the field to the model\'s filename, title-cased and without extension by default.')}
        <Typography variant="caption" color="text.primary" sx={{ display: 'block', mb: 0.5 }}>Options:</Typography>
        {para(<>{inline('include_extension: true')} — include the file extension in the value</>)}

        <Typography variant="caption" color="text.primary" sx={{ display: 'block', mt: 1, mb: 0.25 }}>Examples:</Typography>
        {code('rule: filename')}
        {para(<>A file named {inline('my-dragon.stl')} → {inline('My-Dragon')}</>)}
        {code('rule: filename\ninclude_extension: true')}
        {para(<>A file named {inline('my-dragon.stl')} → {inline('My-Dragon.stl')}</>)}

        <Divider sx={{ my: 1.5 }} />

        {section('rule: regex')}
        {para('Applies a regular expression to a value derived from the file path.')}
        <Typography variant="caption" color="text.primary" sx={{ display: 'block', mb: 0.5 }}>Options:</Typography>
        {para(<>{inline('source: full_path | folder | filename')} — what to match against (default: {inline('full_path')})</>)}
        {para(<>{inline('expression: <pattern>')} — a regex or sed-style substitution ({inline('s|pattern|replacement|flags')})</>)}
        {para(<>{inline('values: \{ EnumValue: "pattern" \}')} — for select fields: try each pattern in order, return the matching key</>)}

        <Typography variant="caption" color="text.primary" sx={{ display: 'block', mt: 1, mb: 0.25 }}>Plain regex — returns first capture group, or full match:</Typography>
        {code('rule: regex\nsource: folder\nexpression: \'([^/]+)/[^/]+$\'')}
        {para(<>For a file at {inline('Artists/Sculptor Name/dragon.stl')} → {inline('Sculptor Name')}</>)}

        <Typography variant="caption" color="text.primary" sx={{ display: 'block', mt: 1, mb: 0.25 }}>Sed-style substitution — transform the matched value:</Typography>
        {code('rule: regex\nsource: folder\nexpression: \'s|.*/([^/]+)/[^/]+$|\\1|\'')}
        {para(<>Same result as above but using a substitution expression</>)}

        <Typography variant="caption" color="text.primary" sx={{ display: 'block', mt: 1, mb: 0.25 }}>Enum (select) field — map patterns to values:</Typography>
        {code('rule: regex\nsource: full_path\nvalues:\n  Bust: \'(?i)bust\'\n  Miniature: \'(?i)mini\'')
        }
        {para('Returns the first key whose pattern matches the path.')}

        <Divider sx={{ my: 1.5 }} />

        {section('Inheritance')}
        {para('Rules set on a parent folder cascade down to all subfolders and models unless overridden. When a subfolder defines its own rule for the same field, it replaces the parent\'s rule for that subtree.')}
        {para('If a folder has a plain value set for a field, that value takes precedence over any inherited rule.')}
      </DialogContent>
      <DialogActions>
        <Button size="small" onClick={onClose}>Close</Button>
      </DialogActions>
    </Dialog>
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
  const [rulesHelpOpen, setRulesHelpOpen] = useState(false)
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
      // Also set mode to 'rule' for fields with inherited rules (if not already set locally)
      if (detail.parentResolvedRules) {
        for (const yamlName of Object.keys(detail.parentResolvedRules)) {
          if (!(yamlName in modes)) {
            modes[yamlName] = 'rule'
          }
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

    try {
      await mutation.mutateAsync({ ...cleanFields, fieldRules: fieldRulesMap })
      committedRef.current = f
      setSavedIndicator(true)
      setTimeout(() => setSavedIndicator(false), 2000)
    } catch (err) {
      if (err instanceof ConfigValidationError) {
        setRuleErrors(prev => ({ ...prev, ...err.fieldErrors }))
      }
    }
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
      <RulesHelpDialog open={rulesHelpOpen} onClose={() => setRulesHelpOpen(false)} />
      <Typography variant="subtitle2" color="text.secondary" className={styles.sectionLabel}>
        Metadata — local values override inherited ones
      </Typography>

      {FIELDS.map(field => {
        const isRuleMode = fieldModes[field.yamlName] === 'rule'
        const ruleText = fieldRuleTexts[field.yamlName] ?? ''
        const ruleError = ruleErrors[field.yamlName] ?? null
        const parentValue = p ? (p[field.key] as string | boolean | null) : null
        const inheritedRule = detail?.parentResolvedRules?.[field.yamlName] ?? null
        const localValue = fields[field.key] as string | boolean | null
        const hasLocalValue = isRuleMode ? ruleText.trim() !== '' : localValue != null
        const hasInheritedValue = parentValue != null || inheritedRule != null
        const canReset = hasLocalValue && hasInheritedValue

        const handleReset = () => {
          // Reset the field to default (null)
          setFieldValue(field.key, null as never)
          // Also clear rule mode if active
          if (isRuleMode) {
            setFieldModes(prev => ({ ...prev, [field.yamlName]: 'value' }))
            setFieldRuleTexts(prev => ({ ...prev, [field.yamlName]: '' }))
          }
          // Save the cleared state
          const cleared = { ...fields, [field.key]: null }
          doSave(cleared)
        }

        return (
          <Box key={field.key}>
            {/* Label + mode toggle + reset button */}
            <Stack direction="row" alignItems="center" justifyContent="space-between" className={styles.fieldHeader}>
              <Typography variant="caption" color="text.secondary">{field.label}</Typography>
              <Stack direction="row" alignItems="center" gap={1}>
                {isRuleMode && (
                  <Tooltip title="Rules documentation">
                    <IconButton
                      size="small"
                      onClick={() => setRulesHelpOpen(true)}
                      className={styles.helpBtn}
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
                        <path d="M11 18h2v-2h-2v2zm1-16C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 18c-4.41 0-8-3.59-8-8s3.59-8 8-8 8 3.59 8 8-3.59 8-8 8zm0-14c-2.21 0-4 1.79-4 4h2c0-1.1.9-2 2-2s2 .9 2 2c0 2-3 1.75-3 5h2c0-2.25 3-2.5 3-5 0-2.21-1.79-4-4-4z"/>
                      </svg>
                    </IconButton>
                  </Tooltip>
                )}
                <Chip
                  label={isRuleMode ? 'Rule' : 'Value'}
                  size="small"
                  variant={isRuleMode ? 'filled' : 'outlined'}
                  color={isRuleMode ? 'warning' : 'default'}
                  onClick={() => toggleMode(field.yamlName)}
                  className={styles.modeChip}
                />
                <Tooltip title={canReset ? 'Reset to inherited value' : 'No inherited value to reset to'}>
                  <span>
                    <IconButton
                      size="small"
                      disabled={!canReset}
                      onClick={handleReset}
                      className={styles.resetBtn}
                    >
                      <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
                        <path d="M7 10c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2zm10-8c-4.4 0-8.3 2.5-10.2 6.1C4.8 7 2 9.5 2 12.5c0 3.3 2.7 6 6 6 1 0 1.9-.2 2.8-.7 1.3 1.3 3.1 2.1 5.2 2.1 4.4 0 8-3.6 8-8s-3.6-8-8-8zM8 16c-2.2 0-4-1.8-4-4s1.8-4 4-4 4 1.8 4 4-1.8 4-4 4zm12-8c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2z"/>
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

            {isRuleMode ? (
              <InheritedHint inheritedRule={inheritedRule} />
            ) : (
              <InheritedHint value={parentValue} />
            )}
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
