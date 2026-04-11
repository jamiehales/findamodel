import { memo, useState } from 'react';
import EditRoundedIcon from '@mui/icons-material/EditRounded';
import LayersRoundedIcon from '@mui/icons-material/LayersRounded';
import IconButton from '@mui/material/IconButton';
import Tooltip from '@mui/material/Tooltip';
import { Box, Chip, Stack } from '@mui/material';
import type { Model } from '../lib/api';
import AppDialog from './AppDialog';
import AppCard from './AppCard';
import ModelMetadataEditor from './ModelMetadataEditor';
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
  const [metadataOpen, setMetadataOpen] = useState(false);
  const showSlicerPlaceholder = !model.previewUrl && !model.canExportToPlate;

  return (
    <>
      <AppCard
        href={href}
        interactive
        className={styles.card}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
      >
        {model.previewUrl ? (
          <Box component="img" src={model.previewUrl} alt="" className={styles.preview} />
        ) : showSlicerPlaceholder ? (
          <Box className={styles.previewPlaceholder}>
            <LayersRoundedIcon className={styles.previewPlaceholderIcon} />
            <p className={styles.previewPlaceholderLabel}>{model.fileType.toUpperCase()}</p>
          </Box>
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

        <Tooltip title="Edit metadata" placement="top">
          <IconButton
            size="small"
            className={styles.editMetadataButton}
            onClick={(e) => {
              e.preventDefault();
              e.stopPropagation();
              setMetadataOpen(true);
            }}
          >
            <EditRoundedIcon fontSize="inherit" />
          </IconButton>
        </Tooltip>

        <PrintingListControls modelId={model.id} showButtons={hovered} />
      </AppCard>

      <AppDialog
        open={metadataOpen}
        onClose={() => setMetadataOpen(false)}
        title="Edit model metadata"
        maxWidth="sm"
        fullWidth
      >
        <ModelMetadataEditor model={model} onClose={() => setMetadataOpen(false)} />
      </AppDialog>
    </>
  );
}

export default memo(ModelCard);
