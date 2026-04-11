import { memo } from 'react';
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
  return (
    <AppCard href={href} interactive className={styles.card}>
      {model.previewUrl && (
        <Box component="img" src={model.previewUrl} alt="" className={styles.preview} />
      )}

      <Box className={styles.overlay}>
        <Stack direction="row" spacing={0.5} flexWrap="wrap">
          <Chip variant="badge-enabled" label={model.fileType.toUpperCase()} />

          {model.material && <Chip variant="badge-enabled" label={model.material} />}

          {model.supported !== null && (
            <Chip
              variant={model.supported ? 'badge-enabled' : 'badge-disabled'}
              label={model.supported ? 'Supported' : 'Unsupported'}
            />
          )}
        </Stack>

        <p className={styles.name}>{model.name}</p>

        <p className={styles.fileName}>{getFileName(model.relativePath)}</p>

        <p className={styles.size}>{formatBytes(model.fileSize)}</p>
      </Box>

      <PrintingListControls modelId={model.id} />
    </AppCard>
  );
}

export default memo(ModelCard);
