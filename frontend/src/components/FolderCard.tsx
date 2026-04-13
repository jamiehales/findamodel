import { memo, useState } from 'react';
import Box from '@mui/material/Box';
import Typography from '@mui/material/Typography';
import IconButton from '@mui/material/IconButton';
import Tooltip from '@mui/material/Tooltip';
import CircularProgress from '@mui/material/CircularProgress';
import type { ExplorerFolder } from '../lib/api';
import { useLocation, useNavigate } from 'react-router-dom';
import AppDialog from './AppDialog';
import MetadataEditor from './MetadataEditor';
import AppCard from './AppCard';
import CodeTooltip from './CodeTooltip';
import FilterPill from './FilterPill';
import styles from './FolderCard.module.css';
import { Stack } from '@mui/material';
import { useIndexFolder, useIsFolderIndexing } from '../lib/queries';
import { SHARED_FIELDS } from './metadata/fieldDefs';
import { appendFilterValue } from '../lib/modelFilterNavigation';

interface Props {
  folder: ExplorerFolder;
  href: string;
}

type MetaSource = 'set' | 'inherited' | 'unset';

function MetaBadge({
  type,
  value,
  source,
  ruleYaml,
}: {
  type: string;
  value: string | null | undefined;
  source: MetaSource;
  ruleYaml?: string | null;
}) {
  const sourceLabel = source === 'set' ? 'Set' : source === 'inherited' ? 'Inherited' : 'Unset';
  const sourceClass =
    source === 'set'
      ? styles.sourceSet
      : source === 'inherited'
        ? styles.sourceInherited
        : styles.sourceUnset;
  const valueLabel = value ?? (ruleYaml ? 'Rule' : 'Not set');

  const badge = (
    <Box
      component="span"
      className={`${styles.metaBadge} ${source === 'unset' ? styles.metaBadgeUnset : styles.metaBadgeSet}`}
      style={ruleYaml ? { border: `1px dashed var(--color-warning)` } : undefined}
    >
      <span className={styles.metaType}>{type}</span>
      <span className={styles.metaValue}>{valueLabel}</span>
      <span className={`${styles.metaSource} ${sourceClass}`}>{sourceLabel}</span>
    </Box>
  );

  if (!ruleYaml) return badge;

  return (
    <CodeTooltip code={ruleYaml} placement="right">
      {badge}
    </CodeTooltip>
  );
}

function FolderCard({ folder, href }: Props) {
  const [editorOpen, setEditorOpen] = useState(false);
  const navigate = useNavigate();
  const location = useLocation();
  const rv = folder.resolvedValues;
  const lv = folder.localValues;
  const localRuleFields = new Set((folder.localRuleFields ?? []).map((f) => f.toLowerCase()));
  const indexFolder = useIndexFolder(folder.path);
  const indexingState = useIsFolderIndexing(folder.path);
  const resolvedTags = rv.tags ?? [];

  function onTagClick(e: React.MouseEvent, tag: string) {
    e.preventDefault();
    e.stopPropagation();
    const sourceSearch = location.pathname === '/' ? location.search : '';
    const nextSearch = appendFilterValue(sourceSearch, 'tags', tag);
    navigate({ pathname: '/', search: nextSearch ? `?${nextSearch}` : '' });
  }

  function getSource(localValue: unknown, resolvedValue: unknown, ruleKey: string): MetaSource {
    const hasLocalRule = localRuleFields.has(ruleKey.toLowerCase());
    if (localValue != null || hasLocalRule) return 'set';

    const hasInheritedValue = resolvedValue != null || !!folder.ruleConfigs?.[ruleKey];
    return hasInheritedValue ? 'inherited' : 'unset';
  }

  function getDisplayValue(
    key: string,
    value: string | number | boolean | null | undefined,
  ): string | null {
    if (key === 'supported') {
      if (value == null) return null;
      return value ? 'Supported' : 'Unsupported';
    }
    if (key === 'raftHeightMm') {
      if (typeof value !== 'number' || !Number.isFinite(value)) return null;
      return `${value} mm`;
    }
    if (typeof value === 'string' || typeof value === 'number') return String(value);
    return null;
  }

  return (
    <Box className={styles.wrapper}>
      {/* Card face */}
      <AppCard href={href} interactive className={styles.card}>
        {/* Index button - enqueues model indexing for this folder */}
        <Tooltip
          title={
            indexingState === 'running'
              ? 'Indexing…'
              : indexingState === 'queued'
                ? 'Queued…'
                : 'Index models'
          }
          placement="top"
        >
          <span className={styles.indexBtnWrap}>
            <IconButton
              size="medium"
              className={`${styles.indexBtn}${indexingState === 'queued' ? ` ${styles.indexBtnQueued}` : ''}`}
              disabled={indexingState !== null}
              onClick={(e) => {
                e.preventDefault();
                e.stopPropagation();
                indexFolder.mutate();
              }}
            >
              {indexingState === 'running' ? (
                <CircularProgress size={20} className={styles.spinner} />
              ) : (
                <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M17.65 6.35A7.958 7.958 0 0 0 12 4c-4.42 0-7.99 3.58-7.99 8s3.57 8 7.99 8c3.73 0 6.84-2.55 7.73-6h-2.08A5.99 5.99 0 0 1 12 18c-3.31 0-6-2.69-6-6s2.69-6 6-6c1.66 0 3.14.69 4.22 1.78L13 11h7V4l-2.35 2.35z" />
                </svg>
              )}
            </IconButton>
          </span>
        </Tooltip>

        {/* Edit button - stops propagation so click doesn't navigate */}
        <Tooltip title="Edit metadata" placement="top">
          <IconButton
            size="medium"
            className={styles.editBtnWrap}
            sx={{
              position: 'absolute',
              top: 4,
              right: 4,
              padding: '8px',
              color: editorOpen ? 'var(--color-primary-light)' : 'var(--color-text-secondary)',
              minWidth: 44,
              minHeight: 44,
              '&:hover': {
                color: 'var(--color-primary-light)',
                backgroundColor: 'var(--color-surface-hover)',
              },
            }}
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation();
              setEditorOpen(true);
            }}
          >
            {/* Pencil icon (SVG inline) */}
            <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
              <path d="M3 17.25V21h3.75L17.81 9.94l-3.75-3.75L3 17.25zM20.71 7.04a1 1 0 0 0 0-1.41l-2.34-2.34a1 1 0 0 0-1.41 0l-1.83 1.83 3.75 3.75 1.83-1.83z" />
            </svg>
          </IconButton>
        </Tooltip>

        {/* Folder icon */}
        <Box className={styles.icon}>
          <svg width="40" height="40" viewBox="0 0 24 24" fill="currentColor">
            <path d="M10 4H4c-1.11 0-2 .89-2 2v12a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V8c0-1.11-.89-2-2-2h-8l-2-2z" />
          </svg>
        </Box>

        {/* Name */}
        <Typography variant="body2" className={styles.name}>
          {folder.name}
        </Typography>

        {/* Counts */}
        <Typography variant="caption" color="text.secondary" className={styles.counts}>
          {folder.subdirectoryCount > 0 &&
            `${folder.subdirectoryCount} folder${folder.subdirectoryCount !== 1 ? 's' : ''}`}
          {folder.subdirectoryCount > 0 && folder.modelCount > 0 && ' · '}
          {folder.modelCount > 0 &&
            `${folder.modelCount} model${folder.modelCount !== 1 ? 's' : ''}`}
          {folder.subdirectoryCount === 0 && folder.modelCount === 0 && 'Empty'}
        </Typography>

        {resolvedTags.length > 0 && (
          <Stack direction="row" spacing={0.5} flexWrap="wrap" className={styles.tagRow}>
            {resolvedTags.slice(0, 8).map((tag) => (
              <FilterPill key={tag} label={tag} tone="user" onClick={(e) => onTagClick(e, tag)} />
            ))}
            {resolvedTags.length > 8 && (
              <span className={styles.tagOverflow}>+{resolvedTags.length - 8}</span>
            )}
          </Stack>
        )}

        {/* Resolved metadata badges */}
        <Stack direction="column" spacing={1} textAlign="center" width="100%">
          {SHARED_FIELDS.map((field) => {
            const resolved = (rv as unknown as Record<string, unknown>)[field.key] as
              | string
              | number
              | boolean
              | null
              | undefined;
            const local = (lv as Record<string, unknown> | null)?.[field.key];
            const ruleKey = field.yamlName ?? field.key;

            return (
              <MetaBadge
                key={field.key}
                type={field.label}
                value={getDisplayValue(field.key, resolved)}
                source={getSource(local, resolved, ruleKey)}
                ruleYaml={folder.ruleConfigs?.[ruleKey]}
              />
            );
          })}
        </Stack>
      </AppCard>

      <AppDialog
        open={editorOpen}
        onClose={() => setEditorOpen(false)}
        title="Edit metadata"
        maxWidth="md"
        fullWidth
      >
        <MetadataEditor path={folder.path} onClose={() => setEditorOpen(false)} />
      </AppDialog>
    </Box>
  );
}

export default memo(FolderCard);
