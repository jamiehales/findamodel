import { useState } from 'react'
import Box from '@mui/material/Box'
import Typography from '@mui/material/Typography'
import type { Model } from '../lib/api'
import AppCard from './AppCard'
import PrintingListControls from './PrintingListControls'
import styles from './ModelCard.module.css'

function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

const badgeColors: Record<string, { bg: string; color: string }> = {
  stl: { bg: 'rgba(99,102,241,0.2)', color: '#818cf8' },
  obj: { bg: 'rgba(16,185,129,0.2)', color: '#34d399' },
}

interface ModelCardProps {
  model: Model
  href: string
  showControls?: boolean
}

function ModelCard({ model, href, showControls = true }: ModelCardProps) {
  const badge = badgeColors[model.fileType] ?? { bg: 'rgba(255,255,255,0.1)', color: '#94a3b8' }
  const [hovered, setHovered] = useState(false)

  return (
    <AppCard href={href} className={styles.card} onMouseEnter={() => setHovered(true)} onMouseLeave={() => setHovered(false)}>
      {model.previewUrl && (
        <Box
          component="img"
          src={model.previewUrl}
          alt=""
          className={styles.preview}
        />
      )}

      <Box className={styles.overlay}>
        <span
          className={styles.badge}
          style={{ background: badge.bg, color: badge.color }}
        >
          {model.fileType.toUpperCase()}
        </span>

        <Typography className={styles.name}>
          {model.name}
        </Typography>

        <Typography className={styles.size}>
          {formatBytes(model.fileSize)}
        </Typography>
      </Box>

      {showControls && <PrintingListControls modelId={model.id} showButtons={hovered} />}
    </AppCard>
  )
}

export default ModelCard
