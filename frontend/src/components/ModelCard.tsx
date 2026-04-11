import { memo, useState } from 'react';
import { Box, Chip, Stack } from '@mui/material';
import type { Model } from '../lib/api';
import AppCard from './AppCard';
import PrintingListControls from './PrintingListControls';
import styles from './ModelCard.module.css';
import { formatBytes } from '../lib/utils';

function getFileName(relativePath: string): string {
  const parts = relativePath.split('/');
  return parts[parts.length - 1] ?? relativePath;
}

interface ModelCardProps {
  model: Model;
  href: string;
}

function ModelCard({ model, href }: ModelCardProps) {
  const [hovered, setHovered] = useState(false);

  return (
    <AppCard
      href={href}
      interactive
      className={styles.card}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      {model.previewUrl ? (
        <Box component="img" src={model.previewUrl} alt="" className={styles.preview} />
      ) : (
        <Box className={styles.previewPlaceholder} />
      )}

      <Box className={styles.infoBlock}>
        <Stack direction="row" className={styles.chipRow} spacing={0.5} flexWrap="wrap">
          <Chip
            variant="badge-enabled"
            className={styles.chip}
            label={model.fileType.toUpperCase()}
          />

          {model.material && (
            <Chip variant="badge-enabled" className={styles.chip} label={model.material} />
          )}

          {model.supported !== null && (
            <Chip
              variant={model.supported ? 'badge-enabled' : 'badge-disabled'}
              className={`${styles.chip}${model.supported ? '' : ` ${styles.chipDisabled}`}`}
              label={model.supported ? 'Supported' : 'Unsupported'}
            />
          )}
        </Stack>

        <p className={styles.name}>{model.name}</p>

        <p className={styles.fileName}>{getFileName(model.relativePath)}</p>

        <p className={styles.size}>{formatBytes(model.fileSize)}</p>
      </Box>

      <PrintingListControls modelId={model.id} showButtons={hovered} />
    </AppCard>
  );
}

export default memo(ModelCard);
