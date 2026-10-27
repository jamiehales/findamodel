import { memo, useState } from 'react';
import { Link, useLocation, useNavigate } from 'react-router-dom';
import { Folder, Pencil, RefreshCw } from 'lucide-react';
import { IconButton, Spinner, Tag, Tooltip } from '@jamiehales/ui-kit/mostlymaking';
import type { ExplorerFolder } from '../lib/api';
import { useIndexFolder, useIsFolderIndexing } from '../lib/queries';
import { appendFilterValue } from '../lib/modelFilterNavigation';
import { SHARED_FIELDS } from './metadata/fieldDefs';
import FilterPill from './FilterPill';
import MetadataEditor from './MetadataEditor';
import styles from './Explorer.module.css';

interface Props {
  folder: ExplorerFolder;
  href: string;
}

type MetaSource = 'set' | 'inherited';

const SOURCE_LABEL: Record<MetaSource, string> = {
  set: 'Set',
  inherited: 'Inherited',
};

const SOURCE_TAG = { set: 'default', inherited: 'neutral' } as const;

const MAX_TAGS = 8;

function countLabel(folder: ExplorerFolder): string {
  const parts: string[] = [];
  if (folder.subdirectoryCount > 0)
    parts.push(
      `${folder.subdirectoryCount} ${folder.subdirectoryCount === 1 ? 'folder' : 'folders'}`,
    );
  if (folder.modelCount > 0)
    parts.push(`${folder.modelCount} ${folder.modelCount === 1 ? 'model' : 'models'}`);
  return parts.length > 0 ? parts.join(' · ') : 'Empty';
}

function displayValue(key: string, value: unknown): string | null {
  if (key === 'supported') {
    if (value == null) return null;
    return value ? 'Supported' : 'Unsupported';
  }
  if (key === 'raftHeightMm') {
    return typeof value === 'number' && Number.isFinite(value) ? `${value} mm` : null;
  }
  return typeof value === 'string' || typeof value === 'number' ? String(value) : null;
}

/**
 * A folder tile: its name and counts, its tags, and every metadata field with
 * where the value comes from. The name links into the folder and stretches over
 * the card; the tags, rule hints and buttons sit on top of it.
 */
function FolderCard({ folder, href }: Props) {
  const [editorOpen, setEditorOpen] = useState(false);
  const navigate = useNavigate();
  const location = useLocation();
  const indexFolder = useIndexFolder(folder.path);
  const indexingState = useIsFolderIndexing(folder.path);
  const resolved = folder.resolvedValues as unknown as Record<string, unknown>;
  const local = folder.localValues as Record<string, unknown> | null;
  const localRuleFields = new Set((folder.localRuleFields ?? []).map((f) => f.toLowerCase()));
  const tags = folder.resolvedValues.tags ?? [];

  function sourceOf(key: string, ruleKey: string): MetaSource {
    return local?.[key] != null || localRuleFields.has(ruleKey.toLowerCase()) ? 'set' : 'inherited';
  }

  // A field shows when it has a value (plain or from a rule), or when this folder sets a
  // rule for it even if the rule computes nothing here.
  const visibleFields = SHARED_FIELDS.filter(
    (field) =>
      displayValue(field.key, resolved[field.key]) != null ||
      localRuleFields.has((field.yamlName ?? field.key).toLowerCase()),
  );

  function onTagClick(e: React.MouseEvent, tag: string) {
    e.preventDefault();
    e.stopPropagation();
    const sourceSearch = location.pathname === '/' ? location.search : '';
    const nextSearch = appendFilterValue(sourceSearch, 'tags', tag);
    navigate({ pathname: '/', search: nextSearch ? `?${nextSearch}` : '' });
  }

  return (
    <div className={`mm-card-link ${styles.folder}`}>
      <div className={styles.folderHead}>
        <Folder size={20} strokeWidth={1.5} aria-hidden="true" className={styles.folderIcon} />
        <span className={styles.folderText}>
          <Link to={href} className={styles.folderName}>
            {folder.name}
          </Link>
          <span className={styles.meta}>{countLabel(folder)}</span>
        </span>
      </div>

      {tags.length > 0 && (
        <div className={styles.folderTags}>
          {tags.slice(0, MAX_TAGS).map((tag) => (
            <FilterPill key={tag} label={tag} tone="user" onClick={(e) => onTagClick(e, tag)} />
          ))}
          {tags.length > MAX_TAGS && <span className={styles.meta}>+{tags.length - MAX_TAGS}</span>}
        </div>
      )}

      {visibleFields.length > 0 && (
        <dl className={styles.folderMeta}>
          {visibleFields.map((field) => {
            const ruleKey = field.yamlName ?? field.key;
            const rule = folder.ruleConfigs?.[ruleKey] ?? null;
            const value = displayValue(field.key, resolved[field.key]);
            const source = sourceOf(field.key, ruleKey);
            return (
              <div key={field.key} className={styles.metaRow} data-source={source}>
                <dt>{field.label}</dt>
                <dd className={styles.metaValue}>
                  {value ??
                    (rule && (
                      <Tooltip
                        side="right"
                        content={<pre className={styles.rulePreview}>{rule}</pre>}
                      >
                        <button
                          type="button"
                          className={styles.ruleButton}
                          aria-label={`${field.label} comes from a rule: ${rule}`}
                          onClick={(e) => {
                            e.preventDefault();
                            e.stopPropagation();
                          }}
                        >
                          Rule
                        </button>
                      </Tooltip>
                    ))}
                </dd>
                <dd>
                  <Tag variant={SOURCE_TAG[source]} className={styles.sourceTag}>
                    {SOURCE_LABEL[source]}
                  </Tag>
                </dd>
              </div>
            );
          })}
        </dl>
      )}

      <div className={styles.folderActions}>
        <Tooltip content="Edit metadata">
          <IconButton
            size="sm"
            aria-label={`Edit metadata for ${folder.name}`}
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation();
              setEditorOpen(true);
            }}
          >
            <Pencil size={14} strokeWidth={1.5} />
          </IconButton>
        </Tooltip>
        <Tooltip
          content={
            indexingState === 'running'
              ? 'Indexing'
              : indexingState === 'queued'
                ? 'Queued'
                : 'Index folder'
          }
        >
          <IconButton
            size="sm"
            aria-label={`Index ${folder.name}`}
            disabled={indexingState !== null}
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation();
              indexFolder.mutate();
            }}
          >
            {indexingState === 'running' ? (
              <Spinner label="Indexing" />
            ) : (
              <RefreshCw size={14} strokeWidth={1.5} />
            )}
          </IconButton>
        </Tooltip>
      </div>

      <MetadataEditor path={folder.path} open={editorOpen} onClose={() => setEditorOpen(false)} />
    </div>
  );
}

export default memo(FolderCard);
