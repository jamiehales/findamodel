import React from 'react';
import Box from '@mui/material/Box';
import Button from '@mui/material/Button';
import CircularProgress from '@mui/material/CircularProgress';
import IconButton from '@mui/material/IconButton';
import Typography from '@mui/material/Typography';
import DownloadRoundedIcon from '@mui/icons-material/DownloadRounded';
import RefreshRoundedIcon from '@mui/icons-material/RefreshRounded';
import { useParams, useNavigate } from 'react-router-dom';
import {
  useModel,
  useModelOtherParts,
  useActivePrintingList,
  useUpsertPrintingListItem,
} from '../lib/queries';
import { useIndexModel, useIsModelIndexing } from '../lib/queries';
import ModelViewer from '../components/ModelViewer';
import HullPreview from '../components/HullPreview';
import PathBreadcrumb from '../components/PathBreadcrumb';
import ModelCard from '../components/ModelCard';
import gridStyles from '../components/ModelGrid.module.css';
import styles from './ModelPage.module.css';

function formatBytes(bytes: number): string {
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

const badgeColors: Record<string, { bg: string; color: string }> = {
  stl: { bg: 'rgba(99,102,241,0.2)', color: '#818cf8' },
  obj: { bg: 'rgba(16,185,129,0.2)', color: '#34d399' },
};

function ModelPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();

  const decodedId = decodeURIComponent(id ?? '');

  React.useEffect(() => {
    window.scrollTo({ top: 0, left: 0, behavior: 'auto' });
  }, [decodedId]);

  const { data: model, isPending, isError } = useModel(decodedId);
  const { data: otherParts } = useModelOtherParts(decodedId);
  const { data: activeList } = useActivePrintingList();
  const { mutate: upsertItem } = useUpsertPrintingListItem();
  const activeListId = activeList?.id ?? '';
  const qty = model ? (activeList?.items.find((i) => i.modelId === model.id)?.quantity ?? 0) : 0;

  const { mutate: indexModel } = useIndexModel(model?.relativePath ?? '');
  const indexingStatus = useIsModelIndexing(model?.relativePath ?? '');
  const isReindexing = indexingStatus === 'running';

  const backButton = (
    <Button variant="back" onClick={() => navigate('/')}>
      ← Back
    </Button>
  );

  if (isPending) {
    return (
      <Box className={styles.page}>
        {backButton}
        <Box className={styles.loadingCenter}>
          <CircularProgress color="primary" />
        </Box>
      </Box>
    );
  }

  if (isError || model === null) {
    return (
      <Box className={styles.page}>
        {backButton}
        <Box className={styles.errorCenter} color="text.secondary">
          <Typography>Model not found.</Typography>
        </Box>
      </Box>
    );
  }

  const badge = badgeColors[model.fileType] ?? { bg: 'rgba(255,255,255,0.1)', color: '#94a3b8' };

  const metaRows: { label: string; value: React.ReactNode }[] = [
    model.name && { label: 'Name', value: model.name },
    model.creator && { label: 'Creator', value: model.creator },
    model.collection && { label: 'Collection', value: model.collection },
    model.subcollection && { label: 'Subcollection', value: model.subcollection },
    model.category && { label: 'Category', value: model.category },
    model.type && { label: 'Type', value: model.type },
  ].filter(Boolean) as { label: string; value: React.ReactNode }[];

  return (
    <Box className={styles.page}>
      {backButton}

      <Box className={styles.content}>
        <Box className={styles.titleGroup}>
          <Typography component="h1" className={styles.modelTitle}>
            {model.name}
          </Typography>

          <PathBreadcrumb path={model.relativePath} />

          <Box className={styles.headerMetaRow}>
            <Box className={styles.tagRow}>
              <span
                className={styles.fileTypeBadge}
                style={{ background: badge.bg, color: badge.color }}
              >
                {model.fileType.toUpperCase()}
              </span>
              <Typography component="span" className={styles.fileSizeInline}>
                {formatBytes(model.fileSize)}
              </Typography>
              {model.supported != null && (
                <span
                  className={styles.supportedBadge}
                  style={{
                    background: model.supported ? 'rgba(16,185,129,0.15)' : 'rgba(239,68,68,0.15)',
                    color: model.supported ? '#34d399' : '#f87171',
                  }}
                >
                  {model.supported ? 'SUPPORTED' : 'UNSUPPORTED'}
                </span>
              )}
            </Box>

            <Box className={styles.headerActions}>
              <IconButton
                component="a"
                href={model.fileUrl}
                download={`${model.name}.${model.fileType}`}
                aria-label={`Download .${model.fileType}`}
                color="primary"
                className={styles.actionIconBtn}
              >
                <DownloadRoundedIcon fontSize="small" />
              </IconButton>

              <IconButton
                onClick={() => indexModel()}
                aria-label="Reindex model"
                color="primary"
                className={styles.actionIconBtn}
                disabled={isReindexing}
              >
                {isReindexing ? (
                  <CircularProgress size={16} className={styles.reindexSpinner} />
                ) : (
                  <RefreshRoundedIcon fontSize="small" />
                )}
              </IconButton>
            </Box>
          </Box>
        </Box>

        <Box className={styles.qtyControl}>
          <IconButton
            onClick={() =>
              upsertItem({ listId: activeListId, modelId: model.id, quantity: qty - 1 })
            }
            aria-label="Decrease quantity"
            className={styles.qtyBtn}
          >
            −
          </IconButton>
          <Typography className={styles.qtyValue}>{qty}</Typography>
          <IconButton
            onClick={() =>
              upsertItem({ listId: activeListId, modelId: model.id, quantity: qty + 1 })
            }
            aria-label="Increase quantity"
            className={styles.qtyBtn}
          >
            +
          </IconButton>
        </Box>

        {metaRows.length > 0 && (
          <Box className={styles.metaGrid}>
            {metaRows.map(({ label, value }) => (
              <React.Fragment key={label}>
                <Typography className={styles.metaLabel}>{label}</Typography>
                <Typography component="div" className={styles.metaValue}>
                  {value}
                </Typography>
              </React.Fragment>
            ))}
          </Box>
        )}

        <Box className={styles.viewerBox}>
          <ModelViewer
            modelId={model.id}
            fileType={model.fileType}
            convexHull={model.convexHull}
            concaveHull={model.concaveHull}
            convexSansRaftHull={model.convexSansRaftHull}
          />
        </Box>

        {(model.convexHull || model.concaveHull || model.convexSansRaftHull) && (
          <HullPreview
            convexHull={model.convexHull}
            concaveHull={model.concaveHull}
            convexSansRaftHull={model.convexSansRaftHull}
            label="Hull Projections"
          />
        )}

      </Box>

      {(otherParts?.length ?? 0) > 0 && (
        <Box className={styles.otherPartsSection}>
          <Box className={gridStyles.container}>
            <Typography variant="h6" className={styles.otherPartsTitle}>
              Other parts
            </Typography>
            <Box className={gridStyles.grid}>
              {otherParts!.map((part) => (
                <ModelCard
                  key={part.id}
                  href={`/model/${encodeURIComponent(part.id)}`}
                  model={{
                    ...model,
                    id: part.id,
                    name: part.name,
                    relativePath: part.relativePath,
                    fileType: part.fileType,
                    fileSize: part.fileSize,
                    fileUrl: '',
                    hasPreview: part.previewUrl != null,
                    previewUrl: part.previewUrl,
                  }}
                />
              ))}
            </Box>
          </Box>
        </Box>
      )}
    </Box>
  );
}

export default ModelPage;
