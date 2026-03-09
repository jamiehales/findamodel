import { useState } from 'react'
import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import Collapse from '@mui/material/Collapse'
import IconButton from '@mui/material/IconButton'
import Tooltip from '@mui/material/Tooltip'
import CircularProgress from '@mui/material/CircularProgress'
import type { ExplorerFolder } from '../lib/api'
import MetadataEditor from './MetadataEditor'
import AppCard from './AppCard'
import styles from './FolderCard.module.css'
import { Divider, Stack } from '@mui/material'
import { useIndexFolder, useIsFolderIndexing } from '../lib/queries'

const RULE_COLOR = '#fbbf24B0'

interface Props {
  folder: ExplorerFolder
  href: string
}

function MetaBadge({ type, value, ruleYaml }: { type: string; value: string | null | undefined; ruleYaml?: string | null }) {
  const isRule = ruleYaml != null && value == null

  const badge = (
    <Box
      component="span"
      className={`${styles.metaBadge} ${value || isRule ? styles.metaBadgeSet : styles.metaBadgeUnset}`}
      style={isRule ? { border: `1px dashed ${RULE_COLOR}` } : undefined}
    >
      <div style={{ color: value ? '#a5b4fc' : 'rgba(131, 143, 202, 0.53)' }}>
        {value ?? (isRule ? `${type}` : `Unknown ${type.toLowerCase()}`)}
      </div>
    </Box>
  )

  if (!isRule) return badge

  return (
    <Tooltip
      title={<pre style={{ margin: 0, fontFamily: 'monospace', fontSize: '0.75rem' }}>{ruleYaml}</pre>}
      placement="right"
      arrow
    >
      {badge}
    </Tooltip>
  )
}

export default function FolderCard({ folder, href }: Props) {
  const [editorOpen, setEditorOpen] = useState(false)
  const rv = folder.resolvedValues
  const indexFolder = useIndexFolder(folder.path)
  const indexingState = useIsFolderIndexing(folder.path)

  return (
    <Box className={styles.wrapper}>
      {/* Card face */}
      <AppCard href={href} className={styles.card}>
        {/* Index button — enqueues model indexing for this folder */}
        <Tooltip
          title={indexingState === 'running' ? 'Indexing…' : indexingState === 'queued' ? 'Queued…' : 'Index models'}
          placement="top"
        >
          <span>
            <IconButton
              size="small"
              className={`${styles.indexBtn}${indexingState === 'queued' ? ` ${styles.indexBtnQueued}` : ''}`}
              disabled={indexingState !== null}
              onClick={e => {
                e.preventDefault()
                e.stopPropagation()
                indexFolder.mutate()
              }}
            >
              {indexingState === 'running' ? (
                <CircularProgress size={14} className={styles.spinner} />
              ) : (
                <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M17.65 6.35A7.958 7.958 0 0 0 12 4c-4.42 0-7.99 3.58-7.99 8s3.57 8 7.99 8c3.73 0 6.84-2.55 7.73-6h-2.08A5.99 5.99 0 0 1 12 18c-3.31 0-6-2.69-6-6s2.69-6 6-6c1.66 0 3.14.69 4.22 1.78L13 11h7V4l-2.35 2.35z"/>
                </svg>
              )}
            </IconButton>
          </span>
        </Tooltip>

        {/* Edit button — stops propagation so click doesn't navigate */}
        <Tooltip title="Edit metadata" placement="top">
          <IconButton
            size="small"
            className={`${styles.editBtn}${editorOpen ? ` ${styles.editBtnActive}` : ''}`}
            onClick={e => {
              e.preventDefault()
              e.stopPropagation()
              setEditorOpen(v => !v)
            }}
          >
            {/* Pencil icon (SVG inline) */}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
              <path d="M3 17.25V21h3.75L17.81 9.94l-3.75-3.75L3 17.25zM20.71 7.04a1 1 0 0 0 0-1.41l-2.34-2.34a1 1 0 0 0-1.41 0l-1.83 1.83 3.75 3.75 1.83-1.83z"/>
            </svg>
          </IconButton>
        </Tooltip>

        {/* Folder icon */}
        <Box className={styles.icon}>
          <svg width="40" height="40" viewBox="0 0 24 24" fill="rgba(99,102,241,0.7)">
            <path d="M10 4H4c-1.11 0-2 .89-2 2v12a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V8c0-1.11-.89-2-2-2h-8l-2-2z"/>
          </svg>
        </Box>

        {/* Name */}
        <Typography
          variant="body2"
          className={styles.name}
        >
          {folder.name}
        </Typography>

        {/* Counts */}
        <Typography variant="caption" color="text.disabled" className={styles.counts}>
          {folder.subdirectoryCount > 0 && `${folder.subdirectoryCount} folder${folder.subdirectoryCount !== 1 ? 's' : ''}`}
          {folder.subdirectoryCount > 0 && folder.modelCount > 0 && ' · '}
          {folder.modelCount > 0 && `${folder.modelCount} model${folder.modelCount !== 1 ? 's' : ''}`}
          {folder.subdirectoryCount === 0 && folder.modelCount === 0 && 'Empty'}
        </Typography>

        {/* Resolved metadata badges */}
          <Stack direction="column" spacing={1} textAlign="center" width="100%">
            <MetaBadge type="Creator" value={rv.creator} ruleYaml={folder.ruleConfigs?.creator} />
            <MetaBadge type="Collection" value={rv.collection} ruleYaml={folder.ruleConfigs?.collection} />
            <MetaBadge type="Subcollection" value={rv.subcollection} ruleYaml={folder.ruleConfigs?.subcollection} />
            <MetaBadge type="Category" value={rv.category} ruleYaml={folder.ruleConfigs?.category} />
            <MetaBadge type="Type" value={rv.type} ruleYaml={folder.ruleConfigs?.type} />
            <MetaBadge type="Supports" value={rv.supported == null ? null : rv.supported ? "Supported" : "Unsupported"} ruleYaml={folder.ruleConfigs?.supported} />
            <Divider />
            <MetaBadge type="Model Name" value={rv.modelName} ruleYaml={folder.ruleConfigs?.model_name} />
          </Stack>
      </AppCard>

      {/* Collapsible metadata editor */}
      <Collapse in={editorOpen} unmountOnExit>
        <Box className={styles.metaEditorWrapper}>
          <MetadataEditor path={folder.path} onClose={() => setEditorOpen(false)} />
        </Box>
      </Collapse>
    </Box>
  )
}
