import { useState } from 'react'
import Box from '@mui/material/Box'
import Stack from '@mui/material/Stack'
import Tooltip from '@mui/material/Tooltip'
import Typography from '@mui/material/Typography'
import type { ExplorerModel } from '../lib/api'
import AppCard from './AppCard'
import PrintingListControls from './PrintingListControls'
import styles from './ExplorerModelCard.module.css'

function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

const badgeColors: Record<string, { bg: string; color: string }> = {
  stl: { bg: 'rgba(99,102,241,0.2)', color: '#818cf8' },
  obj: { bg: 'rgba(16,185,129,0.2)', color: '#34d399' },
}

// Colour for fields whose value was computed by a rule (amber = dynamic/computed)
const RULE_COLOR = '#fbbf24'
// Colour for fields with a plain inherited/set value (indigo)
const VALUE_COLOR = '#a5b4fc'

function MetaBadge({ value, isRule, ruleYaml }: { value: string; isRule: boolean; ruleYaml?: string | null }) {
  const badge = (
    <Box
      component="span"
      className={styles.metaBadge}
      style={{ color: isRule ? RULE_COLOR : VALUE_COLOR }}
    >
      {value}
    </Box>
  )

  if (!isRule || !ruleYaml) return badge

  return (
    <Tooltip
      title={<pre style={{ margin: 0, fontFamily: 'monospace', fontSize: '0.75rem' }}>{ruleYaml}</pre>}
      placement="top"
      arrow
    >
      {badge}
    </Tooltip>
  )
}

function MetaBadges({ meta, ruleConfigs }: { meta: ExplorerModel['resolvedMetadata'] & object; ruleConfigs: Record<string, string> | null }) {
  const entries: { value: string; isRule: boolean; ruleYaml?: string }[] = []

  if (meta.creator) entries.push({ value: meta.creator, isRule: 'creator' in (ruleConfigs ?? {}), ruleYaml: ruleConfigs?.creator })
  if (meta.collection) entries.push({ value: meta.collection, isRule: 'collection' in (ruleConfigs ?? {}), ruleYaml: ruleConfigs?.collection })
  if (meta.category) entries.push({ value: meta.category, isRule: 'category' in (ruleConfigs ?? {}), ruleYaml: ruleConfigs?.category })
  if (meta.type) entries.push({ value: meta.type, isRule: 'type' in (ruleConfigs ?? {}), ruleYaml: ruleConfigs?.type })
  if (meta.supported != null)
    entries.push({ value: meta.supported ? 'Supported' : 'Unsupported', isRule: false })

  if (entries.length === 0) return null

  return (
    <Stack direction="row" flexWrap="wrap" gap={0.5} className={styles.metaBadges}>
      {entries.map((e, i) => (
        <MetaBadge key={i} value={e.value} isRule={e.isRule} ruleYaml={e.ruleYaml} />
      ))}
    </Stack>
  )
}

interface Props {
  model: ExplorerModel
  href?: string
}

export default function ExplorerModelCard({ model, href }: Props) {
  const badge = badgeColors[model.fileType] ?? { bg: 'rgba(255,255,255,0.1)', color: '#94a3b8' }
  const isIndexed = model.id != null
  const [hovered, setHovered] = useState(false)

  return (
    <AppCard
      href={href}
      className={`${styles.card}${!isIndexed ? ` ${styles.cardUnindexed}` : ''}`}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
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
          {model.resolvedMetadata?.modelName ?? model.fileName.replace(/\.[^.]+$/, '')}
        </Typography>

        {model.fileSize != null && (
          <Typography className={styles.size}>
            {formatBytes(model.fileSize)}
          </Typography>
        )}

        {!isIndexed && (
          <Typography className={styles.unindexedLabel}>
            Not yet indexed
          </Typography>
        )}

        {model.resolvedMetadata && (
          <MetaBadges meta={model.resolvedMetadata} ruleConfigs={model.ruleConfigs} />
        )}
      </Box>

      {model.id && <PrintingListControls modelId={model.id} showButtons={hovered} />}
    </AppCard>
  )
}
