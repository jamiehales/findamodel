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
import { Stack } from '@mui/material'
import { useIndexFolder, useIsFolderIndexing } from '../lib/queries'

interface Props {
  folder: ExplorerFolder
  href: string
}

function MetaBadge({ type, value }: { type: string,value: string | null | undefined }) {
  const color = value ? 'rgba(99,102,241,0.18)' : 'rgba(0,0,0,0.1)';

  return (
    <Box
      component="span"
      sx={{
        display: 'inline-block',
        px: '6px',
        py: '1px',
        borderRadius: '4px',
        fontSize: '0.65rem',
        fontWeight: 600,
        bgcolor: color,
        color: '#a5b4fc',
        mr: '4px',
        mb: '2px',
        overflow: 'hidden',
        textOverflow: 'ellipsis',
        whiteSpace: 'nowrap',
        maxWidth: '100%',
      }}
    >
      <div style={{ color: value ? '#a5b4fc' : 'rgba(131, 143, 202, 0.53)' }}>{value ?? "Unknown " + type.toLowerCase()}</div>
    </Box>
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
              className={styles.indexBtn}
              disabled={indexingState !== null}
              onClick={e => {
                e.preventDefault()
                e.stopPropagation()
                indexFolder.mutate()
              }}
              sx={{
                color: indexingState === 'queued'
                  ? '#fbbf24'
                  : 'rgba(226,232,240,0.5)',
                '&:hover': { color: '#818cf8', bgcolor: 'rgba(99,102,241,0.15)' },
              }}
            >
              {indexingState === 'running' ? (
                <CircularProgress size={14} sx={{ color: '#818cf8' }} />
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
            className={styles.editBtn}
            onClick={e => {
              e.preventDefault()
              e.stopPropagation()
              setEditorOpen(v => !v)
            }}
            sx={{
              color: editorOpen ? '#818cf8' : 'rgba(226,232,240,0.5)',
              '&:hover': { color: '#818cf8', bgcolor: 'rgba(99,102,241,0.15)' },
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
          sx={{
            fontWeight: 600,
            color: '#e2e8f0',
            textAlign: 'center',
            lineHeight: 1.3,
            display: '-webkit-box',
            WebkitLineClamp: 2,
            WebkitBoxOrient: 'vertical',
            overflow: 'hidden',
            px: 1,
            width: '100%',
            height: '2.6em', // 2 lines of text
          }}
        >
          {folder.name}
        </Typography>

        {/* Counts */}
        <Typography variant="caption" sx={{ color: 'text.disabled', mt: '2px' }}>
          {folder.subdirectoryCount > 0 && `${folder.subdirectoryCount} folder${folder.subdirectoryCount !== 1 ? 's' : ''}`}
          {folder.subdirectoryCount > 0 && folder.modelCount > 0 && ' · '}
          {folder.modelCount > 0 && `${folder.modelCount} model${folder.modelCount !== 1 ? 's' : ''}`}
          {folder.subdirectoryCount === 0 && folder.modelCount === 0 && 'Empty'}
        </Typography>

        {/* Resolved metadata badges */}
          <Stack direction="column" spacing={1} textAlign="center" width="100%">
            <MetaBadge type="Creator" value={rv.creator} />
            <MetaBadge type="Collection" value={rv.collection} />
            <MetaBadge type="Subcollection" value={rv.subcollection} />
            <MetaBadge type="Category" value={rv.category} />
            <MetaBadge type="Type" value={rv.type} />
            <MetaBadge type="Supports" value={rv.supported == null ? null : rv.supported ? "Supported" : "Unsupported"} />
          </Stack>
      </AppCard>

      {/* Collapsible metadata editor */}
      <Collapse in={editorOpen} unmountOnExit>
        <Box
          sx={{
            border: '1px solid rgba(99,102,241,0.3)',
            borderTop: 'none',
            borderRadius: '0 0 8px 8px',
            bgcolor: 'rgba(15,23,42,0.8)',
          }}
        >
          <MetadataEditor path={folder.path} onClose={() => setEditorOpen(false)} />
        </Box>
      </Collapse>
    </Box>
  )
}
