import { useState } from 'react';
import { Box, Chip, Typography, Stack } from '@mui/material';
import type { Model } from '../lib/api';
import AppCard from './AppCard';
import PrintingListControls from './PrintingListControls';
import styles from './ModelCard.module.css';

function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

interface ModelCardProps {
  model: Model;
  href: string;
  showControls?: boolean;
}

function ModelCard({ model, href, showControls = true }: ModelCardProps) {
  const [hovered, setHovered] = useState(false);

  return (
    <AppCard
      href={href}
      className={styles.card}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      {model.previewUrl && (
        <Box component="img" src={model.previewUrl} alt="" className={styles.preview} />
      )}

      <Box className={styles.overlay}>
        <Stack direction="row" spacing={0.5}>
          <Chip variant="badge-enabled" label={model.fileType.toUpperCase()} />

          {model.supported !== null && (
            <Chip
              variant={model.supported ? 'badge-enabled' : 'badge-disabled'}
              label={model.supported ? 'Supported' : 'Unsupported'}
            />
          )}
        </Stack>

        <Typography className={styles.name}>{model.name}</Typography>

        <Typography className={styles.size}>{formatBytes(model.fileSize)}</Typography>
      </Box>

      {showControls && <PrintingListControls modelId={model.id} showButtons={hovered} />}
    </AppCard>
  );
}

export default ModelCard;
