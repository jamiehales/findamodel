import Box from '@mui/material/Box';
import Button from '@mui/material/Button';
import CircularProgress from '@mui/material/CircularProgress';
import TextField from '@mui/material/TextField';
import Typography from '@mui/material/Typography';
import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  usePrintingLists,
  useCreatePrintingList,
  useRenamePrintingList,
  useDeletePrintingList,
  useActivatePrintingList,
} from '../lib/queries';
import ConfirmDialog from '../components/ConfirmDialog';
import LoadingView from '../components/LoadingView';
import PageLayout from '../components/layouts/PageLayout';
import styles from './PrintingListsManagePage.module.css';

function PrintingListsManagePage() {
  const navigate = useNavigate();
  const { data: lists, isPending } = usePrintingLists();
  const createList = useCreatePrintingList();
  const renameList = useRenamePrintingList();
  const deleteList = useDeletePrintingList();
  const activateList = useActivatePrintingList();

  const [newName, setNewName] = useState('');
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState('');
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; name: string } | null>(null);

  function handleCreate(e: React.FormEvent) {
    e.preventDefault();
    const name = newName.trim();
    if (!name) return;
    createList.mutate(name, { onSuccess: () => setNewName('') });
  }

  function startEdit(id: string, currentName: string) {
    setEditingId(id);
    setEditingName(currentName);
  }

  function handleRename(id: string) {
    const name = editingName.trim();
    if (!name) return;
    renameList.mutate({ id, name }, { onSuccess: () => setEditingId(null) });
  }

  function handleDelete(id: string, name: string) {
    setDeleteTarget({ id, name });
  }

  function handleConfirmDelete() {
    if (!deleteTarget) return;
    deleteList.mutate(deleteTarget.id);
    setDeleteTarget(null);
  }

  return (
    <PageLayout variant="narrow" spacing={4}>
      <Typography component="h1" className={styles.title}>
        Printing lists
      </Typography>

      {/* Create new list */}
      <Box component="form" onSubmit={handleCreate} className={styles.createForm}>
        <TextField
          size="small"
          placeholder="New list name…"
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
          className={styles.createTextField}
        />
        <Button
          variant="contained"
          type="submit"
          disabled={!newName.trim() || createList.isPending}
          startIcon={createList.isPending ? <CircularProgress size={14} color="inherit" /> : null}
          className={styles.pillCreate}
        >
          Create
        </Button>
      </Box>

      {/* List of printing lists */}
      {isPending ? (
        <LoadingView />
      ) : !lists || lists.length === 0 ? (
        <Typography color="text.secondary">No printing lists yet. Create one above.</Typography>
      ) : (
        <Box className={styles.listStack}>
          {lists.map((list) => (
            <Box
              key={list.id}
              className={`${styles.listItem} ${list.isActive ? styles.listItemActive : styles.listItemDefault} ${!list.isActive && editingId !== list.id ? styles.listItemClickable : ''}`}
              onClick={() => {
                if (editingId === list.id) return;
                if (list.isActive) return;
                if (activateList.isPending) return;
                activateList.mutate(list.id);
              }}
            >
              {/* Name / rename */}
              <Box className={styles.listItemName}>
                {editingId === list.id ? (
                  <Box
                    component="form"
                    onSubmit={(e) => {
                      e.preventDefault();
                      handleRename(list.id);
                    }}
                    onClick={(e) => e.stopPropagation()}
                    className={styles.editForm}
                  >
                    <TextField
                      size="small"
                      autoFocus
                      value={editingName}
                      onChange={(e) => setEditingName(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === 'Escape') setEditingId(null);
                      }}
                      className={styles.editTextField}
                    />
                    <Button
                      variant="contained"
                      type="submit"
                      size="small"
                      disabled={renameList.isPending}
                      className={styles.pillSave}
                    >
                      Save
                    </Button>
                    <Button
                      variant="contained"
                      size="small"
                      onClick={() => setEditingId(null)}
                      className={styles.pillCancel}
                    >
                      Cancel
                    </Button>
                  </Box>
                ) : (
                  <Box className={styles.nameMeta}>
                    <Typography
                      className={`${styles.nameText} ${list.isActive ? styles.nameTextActive : styles.nameTextDefault}${!list.isDefault ? ` ${styles.nameTextClickable}` : ''}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        if (!list.isDefault) startEdit(list.id, list.name);
                      }}
                    >
                      {list.name}
                    </Typography>
                    {list.isActive && <span className={styles.activeBadge}>active</span>}
                    {list.isDefault && <span className={styles.defaultBadge}>default</span>}
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
                <Box className={styles.actions} onClick={(e) => e.stopPropagation()}>
                  {!list.isDefault && (
                    <Button
                      variant="warning"
                      size="small"
                      disabled={deleteList.isPending}
                      onClick={() => handleDelete(list.id, list.name)}
                    >
                      Delete
                    </Button>
                  )}
                  <Button
                    variant="contained"
                    size="small"
                    onClick={() => navigate(`/printing-list/${list.id}`)}
                    className={styles.pillView}
                  >
                    View
                  </Button>
                </Box>
              )}
            </Box>
          ))}
        </Box>
      )}

      <ConfirmDialog
        open={deleteTarget !== null}
        title="Delete printing list?"
        message={`Are you sure you want to delete "${deleteTarget?.name ?? ''}"?`}
        confirmLabel="Delete"
        onConfirm={handleConfirmDelete}
        onCancel={() => setDeleteTarget(null)}
        pending={deleteList.isPending}
      />
    </PageLayout>
  );
}

export default PrintingListsManagePage;
