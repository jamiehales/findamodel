import { useState } from 'react'
import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import Collapse from '@mui/material/Collapse'
import IconButton from '@mui/material/IconButton'
import Tooltip from '@mui/material/Tooltip'
import type { ExplorerFolder } from '../lib/api'
import MetadataEditor from './MetadataEditor'
import styles from './FolderCard.module.css'

interface Props {
  folder: ExplorerFolder
  onNavigate: () => void
}

function MetaBadge({ value }: { value: string | null | undefined }) {
  if (!value) return null
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
        bgcolor: 'rgba(99,102,241,0.18)',
        color: '#a5b4fc',
        mr: '4px',
        mb: '2px',
        overflow: 'hidden',
        textOverflow: 'ellipsis',
        whiteSpace: 'nowrap',
        maxWidth: '100%',
      }}
    >
      {value}
    </Box>
  )
}

export default function FolderCard({ folder, onNavigate }: Props) {
  const [editorOpen, setEditorOpen] = useState(false)
  const rv = folder.resolvedValues

  return (
    <Box className={styles.wrapper}>
      {/* Card face */}
      <Box className={styles.card} onClick={onNavigate}>
        {/* Edit button — stops propagation so click doesn't navigate */}
        <Tooltip title="Edit metadata" placement="top">
          <IconButton
            size="small"
            className={styles.editBtn}
            onClick={e => {
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
        <Box sx={{ px: 1, mt: '4px', textAlign: 'center', width: '100%' }}>
          <MetaBadge value={rv.author} />
          <MetaBadge value={rv.collection} />
        </Box>
      </Box>

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
