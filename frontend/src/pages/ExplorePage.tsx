import Box from '@mui/material/Box';
import Button from '@mui/material/Button';
import IconButton from '@mui/material/IconButton';
import CircularProgress from '@mui/material/CircularProgress';
import Tooltip from '@mui/material/Tooltip';
import Stack from '@mui/material/Stack';
import Typography from '@mui/material/Typography';
import RefreshRoundedIcon from '@mui/icons-material/RefreshRounded';
import { useState } from 'react';
import { useParams } from 'react-router-dom';
import { useExplorer, useIndexFolder, useIsFolderIndexing } from '../lib/queries';
import type { ExplorerModel, Model } from '../lib/api';
import { explorerFileUrl } from '../lib/api';
import AppDialog from '../components/AppDialog';
import ErrorView from '../components/ErrorView';
import FolderCard from '../components/FolderCard';
import ExplorerFileCard from '../components/ExplorerFileCard';
import ExplorerModelCard from '../components/ExplorerModelCard';
import ModelCard from '../components/ModelCard';
import LoadingView from '../components/LoadingView';
import MetadataEditor from '../components/MetadataEditor';
import PathBreadcrumb from '../components/PathBreadcrumb';
import PageLayout from '../components/layouts/PageLayout';
import CardGrid, { DEFAULT_CARD_MIN_WIDTH_PX } from '../components/CardGrid';
import styles from './ExplorePage.module.css';

function toProcessedModel(model: ExplorerModel): Model | null {
  if (!model.id) return null;

  const nameFromFile = model.fileName.replace(/\.[^.]+$/, '');

  return {
    id: model.id,
    name: model.resolvedMetadata?.modelName ?? nameFromFile,
    partName: model.resolvedMetadata?.partName ?? null,
    relativePath: model.relativePath,
    fileType: model.fileType,
    canExportToPlate:
      model.fileType.toLowerCase() === 'stl' || model.fileType.toLowerCase() === 'obj',
    fileSize: model.fileSize ?? 0,
    fileUrl: explorerFileUrl(model.relativePath),
    hasPreview: model.hasPreview,
    previewUrl: model.previewUrl,
    creator: model.resolvedMetadata?.creator ?? null,
    collection: model.resolvedMetadata?.collection ?? null,
    subcollection: model.resolvedMetadata?.subcollection ?? null,
    tags: model.resolvedMetadata?.tags ?? [],
    generatedTags: [],
    generatedTagConfidence: {},
    generatedTagsStatus: 'none',
    generatedTagsAt: null,
    generatedTagsError: null,
    generatedTagsModel: null,
    generatedDescription: null,
    category: model.resolvedMetadata?.category ?? null,
    type: model.resolvedMetadata?.type ?? null,
    material: model.resolvedMetadata?.material ?? null,
    supported: model.resolvedMetadata?.supported ?? null,
    convexHull: null,
    concaveHull: null,
    convexSansRaftHull: null,
    raftHeightMm: model.resolvedMetadata?.raftHeightMm ?? 0,
    dimensionXMm: null,
    dimensionYMm: null,
    dimensionZMm: null,
    sphereCentreX: null,
    sphereCentreY: null,
    sphereCentreZ: null,
    sphereRadius: null,
  };
}

function ExplorePageInner({ path }: { path: string }) {
  const { data, isPending, isError } = useExplorer(path);

  if (isPending) return <LoadingView />;

  if (isError) return <ErrorView message="Failed to load directory. Check that the path exists." />;

  const isEmpty = data.folders.length === 0 && data.models.length === 0 && data.files.length === 0;

  return (
    <>
      {isEmpty && (
        <Typography color="text.disabled" className={styles.statusMessage}>
          This folder is empty.
        </Typography>
      )}

      {data.folders.length > 0 && (
        <Box
          className={
            data.models.length > 0 || data.files.length > 0 ? styles.sectionWithMargin : undefined
          }
        >
          <Typography variant="section-label">Folders</Typography>
          <CardGrid minCardWidth={DEFAULT_CARD_MIN_WIDTH_PX}>
            {data.folders.map((folder) => (
              <FolderCard key={folder.path} folder={folder} href={`/explore/${folder.path}`} />
            ))}
          </CardGrid>
        </Box>
      )}

      {data.models.length > 0 && (
        <Box className={data.files.length > 0 ? styles.sectionWithMargin : undefined}>
          <Typography variant="section-label">Models</Typography>
          <CardGrid minCardWidth={DEFAULT_CARD_MIN_WIDTH_PX}>
            {data.models.map((model) => {
              const processedModel = toProcessedModel(model);
              if (processedModel) {
                return (
                  <ModelCard
                    key={processedModel.id}
                    model={processedModel}
                    href={`/model/${encodeURIComponent(processedModel.id)}`}
                  />
                );
              }

              return (
                <ExplorerModelCard
                  key={model.relativePath}
                  model={model}
                  href={model.id ? `/model/${encodeURIComponent(model.id)}` : undefined}
                />
              );
            })}
          </CardGrid>
        </Box>
      )}

      {data.files.length > 0 && (
        <Box>
          <Typography variant="section-label">Files</Typography>
          <CardGrid minCardWidth={DEFAULT_CARD_MIN_WIDTH_PX}>
            {data.files.map((file) => (
              <ExplorerFileCard key={file.relativePath} file={file} />
            ))}
          </CardGrid>
        </Box>
      )}
    </>
  );
}

export default function ExplorePage() {
  const params = useParams();
  const path = params['*'] ?? '';
  const [metadataOpen, setMetadataOpen] = useState(false);
  const indexFolder = useIndexFolder(path);
  const indexingState = useIsFolderIndexing(path);
  const isIndexing = indexingState === 'running';

  return (
    <PageLayout>
      <Box className={styles.headerRow}>
        <PathBreadcrumb path={path} />
        <Stack direction="row" alignItems="center" className={styles.headerActions}>
          <Button size="small" variant="outlined" onClick={() => setMetadataOpen(true)}>
            Edit metadata
          </Button>
          <Tooltip
            title={
              indexingState === 'running'
                ? 'Indexing...'
                : indexingState === 'queued'
                  ? 'Queued...'
                  : 'Index folder'
            }
            placement="top"
          >
            <span>
              <IconButton
                size="small"
                color="primary"
                aria-label="Index folder"
                onClick={() => indexFolder.mutate()}
                disabled={indexingState !== null}
              >
                {isIndexing ? (
                  <CircularProgress size={16} />
                ) : (
                  <RefreshRoundedIcon fontSize="small" />
                )}
              </IconButton>
            </span>
          </Tooltip>
        </Stack>
      </Box>

      <ExplorePageInner path={path} />

      <AppDialog
        open={metadataOpen}
        onClose={() => setMetadataOpen(false)}
        title="Edit metadata"
        maxWidth="md"
        fullWidth
      >
        <MetadataEditor path={path} onClose={() => setMetadataOpen(false)} />
      </AppDialog>
    </PageLayout>
  );
}
