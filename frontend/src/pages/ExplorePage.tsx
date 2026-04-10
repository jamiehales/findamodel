import Box from '@mui/material/Box';
import Button from '@mui/material/Button';
import CircularProgress from '@mui/material/CircularProgress';
import Typography from '@mui/material/Typography';
import { useState } from 'react';
import { useParams } from 'react-router-dom';
import { useExplorer } from '../lib/queries';
import AppDialog from '../components/AppDialog';
import FolderCard from '../components/FolderCard';
import ExplorerModelCard from '../components/ExplorerModelCard';
import MetadataEditor from '../components/MetadataEditor';
import PathBreadcrumb from '../components/PathBreadcrumb';
import styles from './ExplorePage.module.css';

function ExplorePageInner({ path }: { path: string }) {
  const { data, isPending, isError } = useExplorer(path);

  if (isPending) {
    return (
      <Box className={styles.loadingCenter}>
        <CircularProgress color="primary" />
      </Box>
    );
  }

  if (isError) {
    return (
      <Typography color="error.main" style={{ marginTop: 16 }}>
        Failed to load directory. Check that the path exists.
      </Typography>
    );
  }

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
          <Box className={styles.grid}>
            {data.folders.map((folder) => (
              <FolderCard key={folder.path} folder={folder} href={`/explore/${folder.path}`} />
            ))}
          </Box>
        </Box>
      )}

      {data.models.length > 0 && (
        <Box>
          <Typography variant="section-label">Models</Typography>
          <Box className={styles.grid}>
            {data.models.map((model) => (
              <ExplorerModelCard
                key={model.relativePath}
                model={model}
                href={model.id ? `/model/${encodeURIComponent(model.id)}` : undefined}
              />
            ))}
          </Box>
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
    <Box className={styles.page}>
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
    </Box>
  );
}
