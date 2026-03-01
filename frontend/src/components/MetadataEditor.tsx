import { useState, useEffect } from 'react'
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

interface Props {
  path: string
  onClose: () => void
}

const CATEGORIES = ['Bust', 'Miniature', 'Uncategorized']
const TYPES = ['Whole', 'Part']

function InheritedHint({ value }: { value: string | boolean | null | undefined }) {
  if (value == null) return null
  return (
    <Typography variant="caption" sx={{ color: 'text.disabled', display: 'block', mt: '2px' }}>
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

  // Populate form when data loads
  useEffect(() => {
    if (detail) {
      setFields({ ...detail.localValues })
    }
  }, [detail])

  function set<K extends keyof MetadataFields>(key: K, value: MetadataFields[K]) {
    setFields(prev => ({ ...prev, [key]: value }))
  }

  async function handleSave() {
    await mutation.mutateAsync(fields)
    setSavedIndicator(true)
    setTimeout(() => setSavedIndicator(false), 2000)
  }

  const p = detail?.parentResolvedValues ?? null

  if (isLoading) {
    return (
      <Box sx={{ display: 'flex', justifyContent: 'center', p: 2 }}>
        <CircularProgress size={24} />
      </Box>
    )
  }

  return (
    <Box sx={{ display: 'flex', flexDirection: 'column', gap: 1.5, p: 2 }}>
      <Typography variant="subtitle2" sx={{ color: 'text.secondary', mb: 0.5 }}>
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
          slotProps={{ input: { sx: { fontSize: '0.875rem' } } }}
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
          slotProps={{ input: { sx: { fontSize: '0.875rem' } } }}
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
          slotProps={{ input: { sx: { fontSize: '0.875rem' } } }}
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
            onChange={e => set('category', e.target.value || null)}
            sx={{ fontSize: '0.875rem' }}
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
            onChange={e => set('type', e.target.value || null)}
            sx={{ fontSize: '0.875rem' }}
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
                const next = fields.supported == null ? true : fields.supported ? false : null
                set('supported', next)
              }}
            />
          }
          label={<Typography sx={{ fontSize: '0.875rem' }}>Supported</Typography>}
        />
        <InheritedHint value={p?.supported} />
      </Box>

      {/* Actions */}
      <Box sx={{ display: 'flex', gap: 1, justifyContent: 'flex-end', mt: 0.5 }}>
        <Button size="small" onClick={onClose} sx={{ textTransform: 'none', color: 'text.secondary' }}>
          Close
        </Button>
        <Button
          size="small"
          variant="contained"
          onClick={handleSave}
          disabled={mutation.isPending}
          sx={{
            textTransform: 'none',
            bgcolor: '#6366f1',
            '&:hover': { bgcolor: '#4f46e5' },
            minWidth: 80,
          }}
        >
          {mutation.isPending ? (
            <CircularProgress size={14} sx={{ color: 'white' }} />
          ) : savedIndicator ? 'Saved!' : 'Save'}
        </Button>
      </Box>

      {mutation.isError && (
        <Typography variant="caption" sx={{ color: 'error.main' }}>
          Failed to save — please try again.
        </Typography>
      )}
    </Box>
  )
}
