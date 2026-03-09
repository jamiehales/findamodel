import { useState, useEffect, useRef } from 'react'
import Box from '@mui/material/Box'
import TextField from '@mui/material/TextField'
import Select from '@mui/material/Select'
import MenuItem from '@mui/material/MenuItem'
import FormControl from '@mui/material/FormControl'
import InputLabel from '@mui/material/InputLabel'
import FormControlLabel from '@mui/material/FormControlLabel'
import Checkbox from '@mui/material/Checkbox'
import Typography from '@mui/material/Typography'
import Button from '@mui/material/Button'
import CircularProgress from '@mui/material/CircularProgress'
import { useDirectoryConfig, useUpdateDirectoryConfig } from '../lib/queries'
import type { MetadataFields } from '../lib/api'
import styles from './MetadataEditor.module.css'

interface Props {
  path: string
  onClose: () => void
}

const CATEGORIES = ['Bust', 'Miniature', 'Uncategorized']
const TYPES = ['Whole', 'Part']

function InheritedHint({ value }: { value: string | boolean | null | undefined }) {
  if (value == null) return null
  return (
    <Typography variant="caption" color="text.disabled" component="p" className={styles.subtitle}>
      Inherited: {String(value)}
    </Typography>
  )
}

function RuleHint() {
  return (
    <Typography variant="caption" color="warning.main" component="p" className={styles.subtitle}>
      Controlled by a rule — edit the YAML to change
    </Typography>
  )
}

export default function MetadataEditor({ path, onClose }: Props) {
  const { data: detail, isLoading } = useDirectoryConfig(path)
  const mutation = useUpdateDirectoryConfig(path)

  const [fields, setFields] = useState<MetadataFields>({
    creator: null, collection: null, subcollection: null,
    category: null, type: null, supported: null, modelName: null,
  })
  const [savedIndicator, setSavedIndicator] = useState(false)
  const committedRef = useRef(fields)

  // Populate form when data loads
  useEffect(() => {
    if (detail) {
      const loaded = { ...detail.localValues }
      setFields(loaded)
      committedRef.current = loaded
    }
  }, [detail])

  function set<K extends keyof MetadataFields>(key: K, value: MetadataFields[K]) {
    setFields(prev => ({ ...prev, [key]: value }))
  }

  async function saveFields(f: MetadataFields) {
    if (mutation.isPending) return
    await mutation.mutateAsync(f)
    committedRef.current = f
    setSavedIndicator(true)
    setTimeout(() => setSavedIndicator(false), 2000)
  }

  function handleCommit() {
    const c = committedRef.current
    const changed = (Object.keys(fields) as (keyof MetadataFields)[]).some(k => fields[k] !== c[k])
    if (changed) saveFields(fields)
  }

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === 'Enter') handleCommit()
  }

  const p = detail?.parentResolvedValues ?? null
  const ruleFields = detail?.localRuleFields ?? null
  function isRule(yamlFieldName: string) {
    return ruleFields != null && ruleFields.includes(yamlFieldName)
  }

  if (isLoading) {
    return (
      <Box className={styles.loadingBox}>
        <CircularProgress size={24} />
      </Box>
    )
  }

  return (
    <Box className={styles.form}>
      <Typography variant="subtitle2" color="text.secondary" className={styles.subtitle}>
        Metadata — local values override inherited ones
      </Typography>

      {/* Model Name */}
      <Box>
        <TextField
          label="Model Name"
          size="small"
          fullWidth
          disabled={isRule('model_name')}
          value={fields.modelName ?? ''}
          placeholder={p?.modelName ?? undefined}
          onChange={e => set('modelName', e.target.value || null)}
          onBlur={handleCommit}
          onKeyDown={handleKeyDown}
          slotProps={{ input: { className: styles.fieldInput } }}
        />
        {isRule('model_name') ? <RuleHint /> : <InheritedHint value={p?.modelName} />}
      </Box>

      {/* Creator */}
      <Box>
        <TextField
          label="Creator"
          size="small"
          fullWidth
          disabled={isRule('creator')}
          value={fields.creator ?? ''}
          placeholder={p?.creator ?? undefined}
          onChange={e => set('creator', e.target.value || null)}
          onBlur={handleCommit}
          onKeyDown={handleKeyDown}
          slotProps={{ input: { className: styles.fieldInput } }}
        />
        {isRule('creator') ? <RuleHint /> : <InheritedHint value={p?.creator} />}
      </Box>

      {/* Collection */}
      <Box>
        <TextField
          label="Collection"
          size="small"
          fullWidth
          disabled={isRule('collection')}
          value={fields.collection ?? ''}
          placeholder={p?.collection ?? undefined}
          onChange={e => set('collection', e.target.value || null)}
          onBlur={handleCommit}
          onKeyDown={handleKeyDown}
          slotProps={{ input: { className: styles.fieldInput } }}
        />
        {isRule('collection') ? <RuleHint /> : <InheritedHint value={p?.collection} />}
      </Box>

      {/* Subcollection */}
      <Box>
        <TextField
          label="Subcollection"
          size="small"
          fullWidth
          disabled={isRule('subcollection')}
          value={fields.subcollection ?? ''}
          placeholder={p?.subcollection ?? undefined}
          onChange={e => set('subcollection', e.target.value || null)}
          onBlur={handleCommit}
          onKeyDown={handleKeyDown}
          slotProps={{ input: { className: styles.fieldInput } }}
        />
        {isRule('subcollection') ? <RuleHint /> : <InheritedHint value={p?.subcollection} />}
      </Box>

      {/* Category */}
      <Box>
        <FormControl size="small" fullWidth disabled={isRule('category')}>
          <InputLabel>Category</InputLabel>
          <Select
            label="Category"
            value={fields.category ?? ''}
            onChange={e => {
              const val = e.target.value || null
              const next = { ...fields, category: val }
              setFields(next)
              saveFields(next)
            }}
            className={styles.selectSmall}
          >
            <MenuItem value=""><em>Not set</em></MenuItem>
            {CATEGORIES.map(c => <MenuItem key={c} value={c}>{c}</MenuItem>)}
          </Select>
        </FormControl>
        {isRule('category') ? <RuleHint /> : <InheritedHint value={p?.category} />}
      </Box>

      {/* Type */}
      <Box>
        <FormControl size="small" fullWidth disabled={isRule('type')}>
          <InputLabel>Type</InputLabel>
          <Select
            label="Type"
            value={fields.type ?? ''}
            onChange={e => {
              const val = e.target.value || null
              const next = { ...fields, type: val }
              setFields(next)
              saveFields(next)
            }}
            className={styles.selectSmall}
          >
            <MenuItem value=""><em>Not set</em></MenuItem>
            {TYPES.map(t => <MenuItem key={t} value={t}>{t}</MenuItem>)}
          </Select>
        </FormControl>
        {isRule('type') ? <RuleHint /> : <InheritedHint value={p?.type} />}
      </Box>

      {/* Supported */}
      <Box>
        <FormControlLabel
          control={
            <Checkbox
              size="small"
              checked={fields.supported ?? false}
              indeterminate={fields.supported == null}
              onChange={() => {
                // cycle: null → true → false → null
                const val = fields.supported == null ? true : fields.supported ? false : null
                const next = { ...fields, supported: val }
                setFields(next)
                saveFields(next)
              }}
            />
          }
          label={<Typography className={styles.labelSmall}>Supported</Typography>}
        />
        <InheritedHint value={p?.supported} />
      </Box>

      {/* Actions */}
      <Box className={styles.actions}>
        <Button size="small" onClick={onClose} className={styles.closeBtn}>
          Close
        </Button>
        <Button
          size="small"
          variant="contained"
          onClick={handleCommit}
          disabled={mutation.isPending}
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
