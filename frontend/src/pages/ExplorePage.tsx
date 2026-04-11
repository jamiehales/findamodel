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
import AppDialog from '../components/AppDialog';
import ErrorView from '../components/ErrorView';
import FolderCard from '../components/FolderCard';
import ExplorerFileCard from '../components/ExplorerFileCard';
import ExplorerModelCard from '../components/ExplorerModelCard';
import LoadingView from '../components/LoadingView';
import MetadataEditor from '../components/MetadataEditor';
import PathBreadcrumb from '../components/PathBreadcrumb';
import PageLayout from '../components/layouts/PageLayout';
import CardGrid, { DEFAULT_CARD_MIN_WIDTH_PX } from '../components/CardGrid';
import styles from './ExplorePage.module.css';

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
            {data.models.map((model) => (
              <ExplorerModelCard
                key={model.relativePath}
                model={model}
                href={model.id ? `/model/${encodeURIComponent(model.id)}` : undefined}
              />
            ))}
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
