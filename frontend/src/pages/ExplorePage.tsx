import Box from '@mui/material/Box';
import Button from '@mui/material/Button';
import Typography from '@mui/material/Typography';
import { useState } from 'react';
import { useParams } from 'react-router-dom';
import { useExplorer } from '../lib/queries';
import AppDialog from '../components/AppDialog';
import ErrorView from '../components/ErrorView';
import FolderCard from '../components/FolderCard';
import ExplorerModelCard from '../components/ExplorerModelCard';
import LoadingView from '../components/LoadingView';
import MetadataEditor from '../components/MetadataEditor';
import PathBreadcrumb from '../components/PathBreadcrumb';
import PageLayout from '../components/layouts/PageLayout';
import CardGrid from '../components/CardGrid';
import styles from './ExplorePage.module.css';

function ExplorePageInner({ path }: { path: string }) {
  const { data, isPending, isError } = useExplorer(path);

  if (isPending) return <LoadingView />;

  if (isError) return <ErrorView message="Failed to load directory. Check that the path exists." />;

  const isEmpty = data.folders.length === 0 && data.models.length === 0;

  return (
    <>
      {isEmpty && (
        <Typography color="text.disabled" style={{ marginTop: 16 }}>
          This folder is empty.
        </Typography>
      )}

      {data.folders.length > 0 && (
        <Box className={data.models.length > 0 ? styles.sectionWithMargin : undefined}>
          <Typography variant="section-label">Folders</Typography>
          <CardGrid>
            {data.folders.map((folder) => (
              <FolderCard key={folder.path} folder={folder} href={`/explore/${folder.path}`} />
            ))}
          </CardGrid>
        </Box>
      )}

      {data.models.length > 0 && (
        <Box>
          <Typography variant="section-label">Models</Typography>
          <CardGrid>
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
    </>
  );
}

export default function ExplorePage() {
  const params = useParams();
  const path = params['*'] ?? '';
  const [metadataOpen, setMetadataOpen] = useState(false);

  return (
    <PageLayout>
      <Box className={styles.headerRow}>
        <PathBreadcrumb path={path} />
        <Button size="small" variant="outlined" onClick={() => setMetadataOpen(true)}>
          Edit metadata
        </Button>
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
