import Box from '@mui/material/Box'
import Button from '@mui/material/Button'
import CircularProgress from '@mui/material/CircularProgress'
import TextField from '@mui/material/TextField'
import Typography from '@mui/material/Typography'
import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import {
  usePrintingLists,
  useCreatePrintingList,
  useRenamePrintingList,
  useDeletePrintingList,
  useActivatePrintingList,
} from '../lib/queries'

const pillSx = {
  borderRadius: '999px',
  px: '1rem',
  py: '0.4rem',
  fontSize: '0.85rem',
  fontWeight: 500,
  textTransform: 'none' as const,
  minWidth: 0,
}

function PrintingListsManagePage() {
  const navigate = useNavigate()
  const { data: lists, isPending } = usePrintingLists()
  const createList = useCreatePrintingList()
  const renameList = useRenamePrintingList()
  const deleteList = useDeletePrintingList()
  const activateList = useActivatePrintingList()

  const [newName, setNewName] = useState('')
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editingName, setEditingName] = useState('')

  function handleCreate(e: React.FormEvent) {
    e.preventDefault()
    const name = newName.trim()
    if (!name) return
    createList.mutate(name, { onSuccess: () => setNewName('') })
  }

  function startEdit(id: string, currentName: string) {
    setEditingId(id)
    setEditingName(currentName)
  }

  function handleRename(id: string) {
    const name = editingName.trim()
    if (!name) return
    renameList.mutate({ id, name }, { onSuccess: () => setEditingId(null) })
  }

  return (
    <Box sx={{ minHeight: '100vh', pb: '3rem' }}>
      <Button
        onClick={() => navigate('/')}
        sx={{
          position: 'fixed',
          top: 'calc(env(safe-area-inset-top, 0px) + 0.75rem)',
          left: '1rem',
          background: 'rgba(15,23,42,0.7)',
          backdropFilter: 'blur(8px)',
          color: '#e2e8f0',
          border: '1px solid rgba(255,255,255,0.12)',
          borderRadius: '999px',
          px: '1rem',
          py: '0.5rem',
          fontSize: '0.9rem',
          fontWeight: 500,
          textTransform: 'none',
          zIndex: 10,
          minWidth: 0,
          '&:hover': { background: 'rgba(30,41,59,0.9)' },
        }}
      >
        ← Back
      </Button>

      <Box sx={{ pt: '5rem', px: '1.25rem', maxWidth: 640, mx: 'auto', display: 'flex', flexDirection: 'column', gap: '2rem' }}>
        <Typography
          component="h1"
          sx={{ fontSize: { xs: '2rem', sm: '2.5rem' }, fontWeight: 700, letterSpacing: '-0.02em', color: '#f1f5f9', lineHeight: 1.2 }}
        >
          Printing lists
        </Typography>

        {/* Create new list */}
        <Box component="form" onSubmit={handleCreate} sx={{ display: 'flex', gap: '0.75rem', alignItems: 'center' }}>
          <TextField
            size="small"
            placeholder="New list name…"
            value={newName}
            onChange={e => setNewName(e.target.value)}
            sx={{
              flex: 1,
              '& .MuiOutlinedInput-root': {
                borderRadius: '999px',
                color: '#e2e8f0',
                '& fieldset': { borderColor: 'rgba(255,255,255,0.15)' },
                '&:hover fieldset': { borderColor: 'rgba(255,255,255,0.3)' },
                '&.Mui-focused fieldset': { borderColor: '#6366f1' },
              },
              '& input': { color: '#e2e8f0', '&::placeholder': { color: '#64748b' } },
            }}
          />
          <Button
            type="submit"
            disabled={!newName.trim() || createList.isPending}
            startIcon={createList.isPending ? <CircularProgress size={14} color="inherit" /> : null}
            sx={{
              ...pillSx,
              background: 'rgba(99,102,241,0.85)',
              color: '#fff',
              border: '1px solid rgba(255,255,255,0.12)',
              '&:hover': { background: 'rgba(79,82,211,0.9)' },
              '&:disabled': { background: 'rgba(99,102,241,0.3)', color: 'rgba(255,255,255,0.4)' },
            }}
          >
            Create
          </Button>
        </Box>

        {/* List of printing lists */}
        {isPending ? (
          <Box sx={{ display: 'flex', justifyContent: 'center', pt: '2rem' }}>
            <CircularProgress size={32} sx={{ color: '#6366f1' }} />
          </Box>
        ) : !lists || lists.length === 0 ? (
          <Typography sx={{ color: 'text.secondary' }}>No printing lists yet. Create one above.</Typography>
        ) : (
          <Box sx={{ display: 'flex', flexDirection: 'column', gap: '0.75rem' }}>
            {lists.map(list => (
              <Box
                key={list.id}
                sx={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: '0.75rem',
                  p: '0.875rem 1rem',
                  borderRadius: '12px',
                  background: list.isActive ? 'rgba(99,102,241,0.12)' : 'rgba(255,255,255,0.04)',
                  border: `1px solid ${list.isActive ? 'rgba(99,102,241,0.4)' : 'rgba(255,255,255,0.08)'}`,
                  flexWrap: 'wrap',
                }}
              >
                {/* Name / rename */}
                <Box sx={{ flex: 1, minWidth: 0 }}>
                  {editingId === list.id ? (
                    <Box component="form" onSubmit={e => { e.preventDefault(); handleRename(list.id) }} sx={{ display: 'flex', gap: '0.5rem', alignItems: 'center' }}>
                      <TextField
                        size="small"
                        autoFocus
                        value={editingName}
                        onChange={e => setEditingName(e.target.value)}
                        onKeyDown={e => { if (e.key === 'Escape') setEditingId(null) }}
                        sx={{
                          flex: 1,
                          '& .MuiOutlinedInput-root': {
                            borderRadius: '8px',
                            color: '#e2e8f0',
                            '& fieldset': { borderColor: 'rgba(255,255,255,0.15)' },
                            '&.Mui-focused fieldset': { borderColor: '#6366f1' },
                          },
                          '& input': { color: '#e2e8f0', py: '0.35rem' },
                        }}
                      />
                      <Button type="submit" size="small" disabled={renameList.isPending} sx={{ ...pillSx, color: '#818cf8', border: '1px solid rgba(99,102,241,0.3)' }}>Save</Button>
                      <Button size="small" onClick={() => setEditingId(null)} sx={{ ...pillSx, color: '#64748b' }}>Cancel</Button>
                    </Box>
                  ) : (
                    <Box sx={{ display: 'flex', alignItems: 'center', gap: '0.5rem', flexWrap: 'wrap' }}>
                      <Typography
                        sx={{
                          fontWeight: 600,
                          color: list.isActive ? '#818cf8' : '#e2e8f0',
                          fontSize: '0.95rem',
                          cursor: list.isDefault ? 'default' : 'pointer',
                          '&:hover': list.isDefault ? {} : { textDecoration: 'underline' },
                        }}
                        onClick={() => { if (!list.isDefault) startEdit(list.id, list.name) }}
                      >
                        {list.name}
                      </Typography>
                      {list.isActive && (
                        <Typography component="span" sx={{ fontSize: '0.75rem', color: '#818cf8', background: 'rgba(99,102,241,0.15)', px: '0.5rem', py: '0.1rem', borderRadius: '999px' }}>
                          active
                        </Typography>
                      )}
                      {list.isDefault && (
                        <Typography component="span" sx={{ fontSize: '0.75rem', color: '#94a3b8', background: 'rgba(255,255,255,0.06)', px: '0.5rem', py: '0.1rem', borderRadius: '999px' }}>
                          default
                        </Typography>
                      )}
                      {list.ownerUsername && (
                        <Typography component="span" sx={{ fontSize: '0.75rem', color: '#64748b' }}>
                          {list.ownerUsername}
                        </Typography>
                      )}
                      <Typography component="span" sx={{ fontSize: '0.75rem', color: '#64748b' }}>
                        {list.itemCount} {list.itemCount === 1 ? 'item' : 'items'}
                      </Typography>
                    </Box>
                  )}
                </Box>

                {/* Actions */}
                {editingId !== list.id && (
                  <Box sx={{ display: 'flex', gap: '0.5rem', flexShrink: 0, alignItems: 'center' }}>
                    {!list.isActive && (
                      <Button
                        size="small"
                        disabled={activateList.isPending}
                        onClick={() => activateList.mutate(list.id)}
                        sx={{ ...pillSx, color: '#818cf8', border: '1px solid rgba(99,102,241,0.3)', '&:hover': { background: 'rgba(99,102,241,0.1)' } }}
                      >
                        Set active
                      </Button>
                    )}
                    {list.itemCount > 0 && (
                      <Button
                        size="small"
                        onClick={() => navigate(`/printing-list/${list.id}`)}
                        sx={{ ...pillSx, color: '#94a3b8', border: '1px solid rgba(255,255,255,0.1)', '&:hover': { background: 'rgba(255,255,255,0.06)', color: '#e2e8f0' } }}
                      >
                        View
                      </Button>
                    )}
                    {!list.isDefault && (
                      <Button
                        size="small"
                        disabled={deleteList.isPending}
                        onClick={() => deleteList.mutate(list.id)}
                        sx={{ ...pillSx, color: '#f87171', border: '1px solid rgba(248,113,113,0.25)', '&:hover': { background: 'rgba(248,113,113,0.1)' } }}
                      >
                        Delete
                      </Button>
                    )}
                  </Box>
                )}
              </Box>
            ))}
          </Box>
        )}
      </Box>
    </Box>
  )
}

export default PrintingListsManagePage
