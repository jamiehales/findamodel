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
import styles from './PrintingListsManagePage.module.css'

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
    <Box className={styles.page}>
      <Button variant="back" onClick={() => navigate('/')}>
        ← Back
      </Button>

      <Box className={styles.content}>
        <Typography component="h1" className={styles.title}>
          Printing lists
        </Typography>

        {/* Create new list */}
        <Box component="form" onSubmit={handleCreate} className={styles.createForm}>
          <TextField
            size="small"
            placeholder="New list name…"
            value={newName}
            onChange={e => setNewName(e.target.value)}
            className={styles.createTextField}
          />
          <Button
            type="submit"
            disabled={!newName.trim() || createList.isPending}
            startIcon={createList.isPending ? <CircularProgress size={14} color="inherit" /> : null}
            className={`${styles.pill} ${styles.pillCreate}`}
          >
            Create
          </Button>
        </Box>

        {/* List of printing lists */}
        {isPending ? (
          <Box className={styles.loadingBox}>
            <CircularProgress size={32} className={styles.spinner} />
          </Box>
        ) : !lists || lists.length === 0 ? (
          <Typography color="text.secondary">No printing lists yet. Create one above.</Typography>
        ) : (
          <Box className={styles.listStack}>
            {lists.map(list => (
              <Box
                key={list.id}
                className={`${styles.listItem} ${list.isActive ? styles.listItemActive : styles.listItemDefault}`}
              >
                {/* Name / rename */}
                <Box className={styles.listItemName}>
                  {editingId === list.id ? (
                    <Box
                      component="form"
                      onSubmit={e => { e.preventDefault(); handleRename(list.id) }}
                      className={styles.editForm}
                    >
                      <TextField
                        size="small"
                        autoFocus
                        value={editingName}
                        onChange={e => setEditingName(e.target.value)}
                        onKeyDown={e => { if (e.key === 'Escape') setEditingId(null) }}
                        className={styles.editTextField}
                      />
                      <Button
                        type="submit"
                        size="small"
                        disabled={renameList.isPending}
                        className={`${styles.pill} ${styles.pillSave}`}
                      >
                        Save
                      </Button>
                      <Button
                        size="small"
                        onClick={() => setEditingId(null)}
                        className={`${styles.pill} ${styles.pillCancel}`}
                      >
                        Cancel
                      </Button>
                    </Box>
                  ) : (
                    <Box className={styles.nameMeta}>
                      <Typography
                        className={`${styles.nameText} ${list.isActive ? styles.nameTextActive : styles.nameTextDefault}${!list.isDefault ? ` ${styles.nameTextClickable}` : ''}`}
                        onClick={() => { if (!list.isDefault) startEdit(list.id, list.name) }}
                      >
                        {list.name}
                      </Typography>
                      {list.isActive && (
                        <span className={styles.activeBadge}>active</span>
                      )}
                      {list.isDefault && (
                        <span className={styles.defaultBadge}>default</span>
                      )}
                      {list.ownerUsername && (
                        <Typography component="span" className={styles.ownerText}>
                          {list.ownerUsername}
                        </Typography>
                      )}
                      <Typography component="span" className={styles.countText}>
                        {list.itemCount} {list.itemCount === 1 ? 'item' : 'items'}
                      </Typography>
                    </Box>
                  )}
                </Box>

                {/* Actions */}
                {editingId !== list.id && (
                  <Box className={styles.actions}>
                    {!list.isActive && (
                      <Button
                        size="small"
                        disabled={activateList.isPending}
                        onClick={() => activateList.mutate(list.id)}
                        className={`${styles.pill} ${styles.pillActivate}`}
                      >
                        Set active
                      </Button>
                    )}
                    {list.itemCount > 0 && (
                      <Button
                        size="small"
                        onClick={() => navigate(`/printing-list/${list.id}`)}
                        className={`${styles.pill} ${styles.pillView}`}
                      >
                        View
                      </Button>
                    )}
                    {!list.isDefault && (
                      <Button
                        size="small"
                        disabled={deleteList.isPending}
                        onClick={() => deleteList.mutate(list.id)}
                        className={`${styles.pill} ${styles.pillDelete}`}
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
