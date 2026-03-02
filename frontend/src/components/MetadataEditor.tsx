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

export default function MetadataEditor({ path, onClose }: Props) {
  const { data: detail, isLoading } = useDirectoryConfig(path)
  const mutation = useUpdateDirectoryConfig(path)

  const [fields, setFields] = useState<MetadataFields>({
    creator: null, collection: null, subcollection: null,
    category: null, type: null, supported: null,
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

      {/* Creator */}
      <Box>
        <TextField
          label="Creator"
          size="small"
          fullWidth
          value={fields.creator ?? ''}
          placeholder={p?.creator ?? undefined}
          onChange={e => set('creator', e.target.value || null)}
          onBlur={handleCommit}
          onKeyDown={handleKeyDown}
          slotProps={{ input: { className: styles.fieldInput } }}
        />
        <InheritedHint value={p?.creator} />
      </Box>

      {/* Collection */}
      <Box>
        <TextField
          label="Collection"
          size="small"
          fullWidth
          value={fields.collection ?? ''}
          placeholder={p?.collection ?? undefined}
          onChange={e => set('collection', e.target.value || null)}
          onBlur={handleCommit}
          onKeyDown={handleKeyDown}
          slotProps={{ input: { className: styles.fieldInput } }}
        />
        <InheritedHint value={p?.collection} />
      </Box>

      {/* Subcollection */}
      <Box>
        <TextField
          label="Subcollection"
          size="small"
          fullWidth
          value={fields.subcollection ?? ''}
          placeholder={p?.subcollection ?? undefined}
          onChange={e => set('subcollection', e.target.value || null)}
          onBlur={handleCommit}
          onKeyDown={handleKeyDown}
          slotProps={{ input: { className: styles.fieldInput } }}
        />
        <InheritedHint value={p?.subcollection} />
      </Box>

      {/* Category */}
      <Box>
        <FormControl size="small" fullWidth>
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
        <InheritedHint value={p?.category} />
      </Box>

      {/* Type */}
      <Box>
        <FormControl size="small" fullWidth>
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
        <InheritedHint value={p?.type} />
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
