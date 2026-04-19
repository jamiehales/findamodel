import React from 'react';
import Box from '@mui/material/Box';
import Button from '@mui/material/Button';
import FormControlLabel from '@mui/material/FormControlLabel';
import Switch from '@mui/material/Switch';
import Typography from '@mui/material/Typography';
import Stack from '@mui/material/Stack';
import { useNavigate, useParams } from 'react-router-dom';
import {
  useAutoSupportGeometry,
  useAutoSupportJob,
  useGenerateAutoSupportJob,
  useModel,
} from '../lib/queries';
import ModelViewer from '../components/ModelViewer';
import ErrorView from '../components/ErrorView';
import LoadingView from '../components/LoadingView';
import PageLayout from '../components/layouts/PageLayout';
import styles from './ModelAutoSupportPage.module.css';

function ModelAutoSupportPage() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const decodedId = decodeURIComponent(id ?? '');

  const [autoSupportJobId, setAutoSupportJobId] = React.useState<string | null>(null);
  const [showForceMarkers, setShowForceMarkers] = React.useState(true);

  React.useEffect(() => {
    window.scrollTo({ top: 0, left: 0, behavior: 'auto' });
    setAutoSupportJobId(null);
    setShowForceMarkers(true);
  }, [decodedId]);

  const { data: model, isPending, isError } = useModel(decodedId);
  const { mutate: generateAutoSupport, isPending: isGeneratingAutoSupportRequest } =
    useGenerateAutoSupportJob(decodedId);
  const { data: autoSupportJob } = useAutoSupportJob(
    decodedId,
    autoSupportJobId,
    !!autoSupportJobId,
  );
  const { data: autoSupportGeometry } = useAutoSupportGeometry(
    decodedId,
    autoSupportJobId,
    autoSupportJob?.status === 'completed',
  );

  const isGeneratingAutoSupport =
    isGeneratingAutoSupportRequest ||
    autoSupportJob?.status === 'queued' ||
    autoSupportJob?.status === 'running';

  const backButton = (
    <Button variant="back" onClick={() => navigate(`/model/${encodeURIComponent(decodedId)}`)}>
      ← Back to model
    </Button>
  );

  if (isPending) {
    return (
      <PageLayout variant="full">
        {backButton}
        <LoadingView minHeight="100vh" />
      </PageLayout>
    );
  }

  if (isError || model === null) {
    return (
      <PageLayout variant="full">
        {backButton}
        <ErrorView message="Model not found." minHeight="100vh" />
      </PageLayout>
    );
  }

  return (
    <PageLayout variant="full">
      <Stack spacing={2} className={styles.pageContent}>
        {backButton}

        <Stack spacing={0.5}>
          <Typography component="h1" variant="page-title">
            Auto support generation
          </Typography>
          <Typography variant="body2" color="text.secondary">
            {model.name}
          </Typography>
          <Typography variant="body2" color="text.secondary">
            Suggested support points are shown with pull-force arrows sized by magnitude.
          </Typography>
        </Stack>

        <Stack direction={{ xs: 'column', sm: 'row' }} spacing={1}>
          <Button
            variant="contained"
            disabled={isGeneratingAutoSupport}
            onClick={() =>
              generateAutoSupport(undefined, {
                onSuccess: (job) => setAutoSupportJobId(job.jobId),
              })
            }
          >
            {isGeneratingAutoSupport
              ? `Generating supports ${autoSupportJob?.progressPercent ?? 0}%`
              : 'Generate autosupport'}
          </Button>
          <FormControlLabel
            control={
              <Switch
                checked={showForceMarkers}
                onChange={(event) => setShowForceMarkers(event.target.checked)}
              />
            }
            label="Show force markers"
          />
        </Stack>

        <Box className={styles.viewerBox}>
          {autoSupportGeometry ? (
            <ModelViewer
              modelId={model.id}
              convexHull={null}
              concaveHull={null}
              convexSansRaftHull={null}
              supported
              splitGeometryOverride={autoSupportGeometry}
              supportPointsOverride={autoSupportJob?.supportPoints ?? null}
              islandsOverride={autoSupportJob?.islands ?? null}
              showForceMarkers={showForceMarkers}
            />
          ) : (
            <Box className={styles.placeholderBox}>
              <Typography>
                {autoSupportJob?.status === 'failed'
                  ? (autoSupportJob.errorMessage ?? 'Support generation failed.')
                  : isGeneratingAutoSupport
                    ? 'Generating supported preview...'
                    : 'Generate supports to preview recommended contact points for this unsupported model.'}
              </Typography>
            </Box>
          )}
        </Box>
      </Stack>
    </PageLayout>
  );
}

export default ModelAutoSupportPage;
