import { useState, useRef, useEffect } from 'react'
import IconButton from '@mui/material/IconButton'
import Popover from '@mui/material/Popover'
import Stack from '@mui/material/Stack'
import Typography from '@mui/material/Typography'
import Chip from '@mui/material/Chip'
import Badge from '@mui/material/Badge'
import Tooltip from '@mui/material/Tooltip'
import { useIndexerStatus } from '../lib/queries'
import type { IndexRequest } from '../lib/api'
import { IndexFlags } from '../lib/api'
import styles from './IndexerStatus.module.css'

function formatElapsed(since: string): string {
  const s = Math.max(0, Math.floor((Date.now() - new Date(since).getTime()) / 1000))
  if (s < 60) return `${s}s`
  const m = Math.floor(s / 60)
  if (m < 60) return `${m}m ${s % 60}s`
  return `${Math.floor(m / 60)}h ${m % 60}m`
}

function useElapsed(since: string): string {
  const [, tick] = useState(0)
  useEffect(() => {
    const id = setInterval(() => tick(n => n + 1), 1000)
    return () => clearInterval(id)
  }, [since])
  return formatElapsed(since)
}

function flagsLabel(flags: number): string {
  const parts: string[] = []
  if (flags & IndexFlags.Directories) parts.push('dirs')
  if (flags & IndexFlags.Models) parts.push('models')
  return parts.join(', ') || 'none'
}

function RequestRow({ request, status }: { request: IndexRequest; status: 'running' | 'queued' }) {
  const elapsed = useElapsed(request.requestedAt)
  return (
    <Stack direction="row" spacing={1} alignItems="center" className={styles.requestRow}>
      <Chip
        label={status === 'running' ? 'Running' : 'Queued'}
        size="small"
        className={status === 'running' ? styles.chipRunning : styles.chipQueued}
      />
      <Typography variant="caption">
        {request.directoryFilter ?? 'All directories'}
      </Typography>
      <Typography variant="caption" color="text.disabled">
        {flagsLabel(request.flags)}
      </Typography>
      <Typography variant="caption" color="text.disabled" className={styles.elapsed}>
        {elapsed}
      </Typography>
    </Stack>
  )
}

export default function IndexerStatus() {
  const { data: status } = useIndexerStatus()
  const [open, setOpen] = useState(false)
  const anchorRef = useRef<HTMLButtonElement>(null)

  const isActive = !!(status?.isRunning || (status?.queue && status.queue.length > 0))
  const totalCount = (status?.isRunning ? 1 : 0) + (status?.queue.length ?? 0)

  return (
    <>
      <Tooltip title="Indexer queue" placement="left">
        <IconButton
          ref={anchorRef}
          size="small"
          onClick={() => setOpen(v => !v)}
          className={`${styles.iconButton} ${isActive ? styles.iconButtonActive : ''}`}
        >
          <Badge
            badgeContent={totalCount > 1 ? totalCount : 0}
            classes={{ badge: styles.badge }}
          >
            <svg
              width="18"
              height="18"
              viewBox="0 0 24 24"
              fill="currentColor"
              className={isActive ? styles.iconSpin : undefined}
            >
              <path d="M17.65 6.35A7.958 7.958 0 0 0 12 4c-4.42 0-7.99 3.58-7.99 8s3.57 8 7.99 8c3.73 0 6.84-2.55 7.73-6h-2.08A5.99 5.99 0 0 1 12 18c-3.31 0-6-2.69-6-6s2.69-6 6-6c1.66 0 3.14.69 4.22 1.78L13 11h7V4l-2.35 2.35z" />
            </svg>
          </Badge>
        </IconButton>
      </Tooltip>

      <Popover
        open={open}
        anchorEl={anchorRef.current}
        onClose={() => setOpen(false)}
        anchorOrigin={{ vertical: 'bottom', horizontal: 'right' }}
        transformOrigin={{ vertical: 'top', horizontal: 'right' }}
        slotProps={{ paper: { className: styles.popoverPaper } }}
      >
        <Stack spacing={1}>
          <Typography variant="caption" color="text.disabled" className={styles.sectionLabel}>
            Indexer
          </Typography>

          {!isActive && (
            <Typography variant="caption" color="text.disabled">
              No active indexing
            </Typography>
          )}

          {status?.currentRequest && (
            <RequestRow request={status.currentRequest} status="running" />
          )}

          {status?.queue.map(req => (
            <RequestRow key={req.id} request={req} status="queued" />
          ))}
        </Stack>
      </Popover>
    </>
  )
}
