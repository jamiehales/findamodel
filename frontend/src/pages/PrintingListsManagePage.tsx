import { useState } from 'react';
import { Link } from 'react-router-dom';
import { Pencil } from 'lucide-react';
import { Button, IconButton, Input, Spinner, Tag, Tooltip } from '@jamiehales/ui-kit/mostlymaking';
import {
  usePrintingLists,
  useCreatePrintingList,
  useRenamePrintingList,
  useDeletePrintingList,
  useActivatePrintingList,
} from '../lib/queries';
import ConfirmDialog from '../components/ConfirmDialog';
import LoadingView from '../components/LoadingView';
import PageLayout, { PageHeader } from '../components/layouts/PageLayout';
import statusStyles from '../components/StatusView.module.css';
import styles from './PrintingListsManagePage.module.css';

function PrintingListsManagePage() {
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
    <PageLayout variant="narrow">
      <PageHeader title="Printing lists" />

      <form onSubmit={handleCreate} className={styles.create}>
        <Input
          aria-label="New list name"
          placeholder="New list name"
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
        />
        <Button variant="primary" type="submit" disabled={!newName.trim() || createList.isPending}>
          {createList.isPending ? <Spinner label="Creating" /> : null}
          New list
        </Button>
      </form>

      {isPending ? (
        <LoadingView />
      ) : !lists || lists.length === 0 ? (
        <p className={statusStyles.empty}>No printing lists yet.</p>
      ) : (
        <ul className={styles.lists}>
          {lists.map((list) => (
            <li key={list.id} className={styles.row} data-active={list.isActive ? '' : undefined}>
              {editingId === list.id ? (
                <form
                  className={styles.edit}
                  onSubmit={(e) => {
                    e.preventDefault();
                    handleRename(list.id);
                  }}
                >
                  <Input
                    autoFocus
                    aria-label="List name"
                    value={editingName}
                    onChange={(e) => setEditingName(e.target.value)}
                    onKeyDown={(e) => {
                      if (e.key === 'Escape') setEditingId(null);
                    }}
                  />
                  <Button variant="ghost" size="sm" onClick={() => setEditingId(null)}>
                    Cancel
                  </Button>
                  <Button
                    variant="secondary"
                    size="sm"
                    type="submit"
                    disabled={renameList.isPending}
                  >
                    Save
                  </Button>
                </form>
              ) : (
                <>
                  <div className={styles.info}>
                    <Link to={`/printing-list/${list.id}`} className={styles.name}>
                      {list.name}
                    </Link>
                    <span className={styles.meta}>
                      {list.itemCount} {list.itemCount === 1 ? 'item' : 'items'}
                      {list.ownerUsername ? ` · ${list.ownerUsername}` : ''}
                    </span>
                  </div>
                  {list.isActive && <Tag variant="success">Active</Tag>}
                  {list.isDefault && <Tag variant="neutral">Default</Tag>}
                  <div className={styles.actions}>
                    {!list.isActive && (
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={activateList.isPending}
                        onClick={() => activateList.mutate(list.id)}
                      >
                        Set active
                      </Button>
                    )}
                    {!list.isDefault && (
                      <Tooltip content="Rename">
                        <IconButton
                          variant="ghost"
                          size="sm"
                          aria-label={`Rename ${list.name}`}
                          onClick={() => startEdit(list.id, list.name)}
                        >
                          <Pencil size={14} strokeWidth={1.5} />
                        </IconButton>
                      </Tooltip>
                    )}
                    {!list.isDefault && (
                      <Button
                        size="sm"
                        variant="ghost"
                        className={styles.delete}
                        disabled={deleteList.isPending}
                        onClick={() => handleDelete(list.id, list.name)}
                      >
                        Delete
                      </Button>
                    )}
                  </div>
                </>
              )}
            </li>
          ))}
        </ul>
      )}

      <ConfirmDialog
        open={deleteTarget !== null}
        title={`Delete ${deleteTarget?.name ?? 'this list'}?`}
        confirmLabel="Delete"
        onConfirm={handleConfirmDelete}
        onCancel={() => setDeleteTarget(null)}
        pending={deleteList.isPending}
      />
    </PageLayout>
  );
}

export default PrintingListsManagePage;
