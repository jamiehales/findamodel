import { memo, useMemo, useState } from 'react';
import EditRoundedIcon from '@mui/icons-material/EditRounded';
import LayersRoundedIcon from '@mui/icons-material/LayersRounded';
import RefreshRoundedIcon from '@mui/icons-material/RefreshRounded';
import CircularProgress from '@mui/material/CircularProgress';
import IconButton from '@mui/material/IconButton';
import Tooltip from '@mui/material/Tooltip';
import { Box, Stack } from '@mui/material';
import { useLocation, useNavigate } from 'react-router-dom';
import type { Model } from '../lib/api';
import { useIndexModel, useIsModelIndexing } from '../lib/queries';
import { appendFilterValue, appendSupportedFilter } from '../lib/modelFilterNavigation';
import AppDialog from './AppDialog';
import AppCard from './AppCard';
import ModelMetadataEditor from './ModelMetadataEditor';
import PrintingListControls from './PrintingListControls';
import FilterPill from './FilterPill';
import styles from './ModelCard.module.css';
import { formatBytes } from '../lib/utils';
import { useRenderControls } from './RenderControlsContext';
import { withPreviewSupports } from '../lib/http';

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
  const { showSupports } = useRenderControls();
  const location = useLocation();
  const navigate = useNavigate();
  const { mutate: indexModel } = useIndexModel(model.relativePath);
  const indexingState = useIsModelIndexing(model.relativePath);
  const previewUrl = useMemo(
    () => (model.previewUrl ? withPreviewSupports(model.previewUrl, showSupports) : null),
    [model.previewUrl, showSupports],
  );
  const showSlicerPlaceholder = !previewUrl && !model.canExportToPlate;
  const generatedDescription = model.generatedDescription?.trim() ?? '';
  const hasDescription = generatedDescription.length > 0;
  const isIndexing = indexingState === 'running';
  const userTags = model.tags ?? [];
  const aiTags = (model.generatedTags ?? []).filter((tag) => !userTags.includes(tag));

  function navigateToModels(nextSearch: string) {
    navigate({ pathname: '/', search: nextSearch ? `?${nextSearch}` : '' });
  }

  function onStandardFilterClick(
    e: React.MouseEvent,
    key: 'fileType' | 'material' | 'category' | 'type',
    value: string,
  ) {
    e.preventDefault();
    e.stopPropagation();
    const sourceSearch = location.pathname === '/' ? location.search : '';
    navigateToModels(appendFilterValue(sourceSearch, key, value));
  }

  function onSupportedFilterClick(e: React.MouseEvent, supported: boolean) {
    e.preventDefault();
    e.stopPropagation();
    const sourceSearch = location.pathname === '/' ? location.search : '';
    navigateToModels(appendSupportedFilter(sourceSearch, supported));
  }

  function onTagFilterClick(e: React.MouseEvent, tag: string) {
    e.preventDefault();
    e.stopPropagation();
    const sourceSearch = location.pathname === '/' ? location.search : '';
    navigateToModels(appendFilterValue(sourceSearch, 'tags', tag));
  }

  return (
    <>
      <Tooltip
        title={generatedDescription}
        placement="top"
        arrow
        disableHoverListener={!hasDescription}
      >
        <span>
          <AppCard
            href={href}
            interactive
            className={styles.card}
            onMouseEnter={() => setHovered(true)}
            onMouseLeave={() => setHovered(false)}
          >
            {previewUrl ? (
              <Box component="img" src={previewUrl} alt="" className={styles.preview} />
            ) : showSlicerPlaceholder ? (
              <Box className={styles.previewPlaceholder}>
                <LayersRoundedIcon className={styles.previewPlaceholderIcon} />
                <p className={styles.previewPlaceholderLabel}>{model.fileType.toUpperCase()}</p>
              </Box>
            ) : (
              <Box className={styles.previewPlaceholder} />
            )}

            <Box className={styles.infoBlock}>
              <Stack direction="row" className={styles.pillRow} spacing={0.5} flexWrap="wrap">
                <FilterPill
                  label={model.fileType.toUpperCase()}
                  onClick={(e) => onStandardFilterClick(e, 'fileType', model.fileType)}
                />

                {model.material && (
                  <FilterPill
                    label={model.material}
                    onClick={(e) => onStandardFilterClick(e, 'material', model.material!)}
                  />
                )}

                {model.category && (
                  <FilterPill
                    label={model.category}
                    onClick={(e) => onStandardFilterClick(e, 'category', model.category!)}
                  />
                )}

                {model.type && (
                  <FilterPill
                    label={model.type}
                    onClick={(e) => onStandardFilterClick(e, 'type', model.type!)}
                  />
                )}

                {model.supported !== null && (
                  <FilterPill
                    label={model.supported ? 'Supported' : 'Unsupported'}
                    tone={model.supported ? 'supported' : 'unsupported'}
                    onClick={(e) => onSupportedFilterClick(e, model.supported!)}
                  />
                )}
              </Stack>

              <p className={styles.name}>{model.name}</p>

              <p className={styles.fileName}>{getFileName(model.relativePath)}</p>

              <p className={styles.size}>{formatBytes(model.fileSize)}</p>

              {aiTags.length > 0 && (
                <Stack direction="row" className={styles.pillRow} spacing={0.5} flexWrap="wrap">
                  {aiTags.map((tag) => (
                    <FilterPill
                      key={`ai-${tag}`}
                      label={`AI: ${tag}`}
                      tone="ai"
                      onClick={(e) => onTagFilterClick(e, tag)}
                    />
                  ))}
                </Stack>
              )}

              {userTags.length > 0 && (
                <Stack direction="row" className={styles.userTagRow} spacing={0.5} flexWrap="wrap">
                  {userTags.map((tag) => (
                    <FilterPill
                      key={`user-${tag}`}
                      label={tag}
                      tone="user"
                      onClick={(e) => onTagFilterClick(e, tag)}
                    />
                  ))}
                </Stack>
              )}
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

            <Tooltip
              title={
                indexingState === 'running'
                  ? 'Refreshing index...'
                  : indexingState === 'queued'
                    ? 'Refresh queued...'
                    : 'Refresh index'
              }
              placement="top"
            >
              <IconButton
                size="small"
                className={styles.refreshIndexButton}
                onClick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  indexModel();
                }}
                disabled={indexingState !== null}
              >
                {isIndexing ? (
                  <CircularProgress size={14} className={styles.refreshSpinner} />
                ) : (
                  <RefreshRoundedIcon fontSize="inherit" />
                )}
              </IconButton>
            </Tooltip>

            <PrintingListControls modelId={model.id} showButtons={hovered} />
          </AppCard>
        </span>
      </Tooltip>

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
