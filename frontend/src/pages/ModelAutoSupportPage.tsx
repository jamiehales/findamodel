import React from 'react';
import Box from '@mui/material/Box';
import Button from '@mui/material/Button';
import FormControlLabel from '@mui/material/FormControlLabel';
import Switch from '@mui/material/Switch';
import Typography from '@mui/material/Typography';
import Stack from '@mui/material/Stack';
import { useLocation, useNavigate, useParams } from 'react-router-dom';
import {
  useAutoSupportGeometry,
  useAutoSupportJob,
  useGenerateAutoSupportJob,
  useModel,
} from '../lib/queries';
import AutoSupportLayerSlider from '../components/AutoSupportLayerSlider';
import ModelViewer from '../components/ModelViewer';
import ErrorView from '../components/ErrorView';
import LoadingView from '../components/LoadingView';
import PageLayout from '../components/layouts/PageLayout';
import styles from './ModelAutoSupportPage.module.css';

type SupportPageLocationState = {
  autoStart?: boolean;
};

function ModelAutoSupportPage() {
  const { id } = useParams<{ id: string }>();
  const location = useLocation();
  const navigate = useNavigate();
  const decodedId = decodeURIComponent(id ?? '');
  const autoStartRequested =
    (location.state as SupportPageLocationState | null)?.autoStart === true;

  const [autoSupportJobId, setAutoSupportJobId] = React.useState<string | null>(null);
  const [showForceMarkers, setShowForceMarkers] = React.useState(true);
  const [showSupportMesh, setShowSupportMesh] = React.useState(false);
  const [selectedLayerIndex, setSelectedLayerIndex] = React.useState(Number.MAX_SAFE_INTEGER);
  const hasAutoStartedRef = React.useRef(false);

  React.useEffect(() => {
    window.scrollTo({ top: 0, left: 0, behavior: 'auto' });
    setAutoSupportJobId(null);
    setShowForceMarkers(true);
    setShowSupportMesh(false);
    setSelectedLayerIndex(Number.MAX_SAFE_INTEGER);
    hasAutoStartedRef.current = false;
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

  const sliceLayers = autoSupportJob?.sliceLayers ?? [];
  const hasSliceLayers = sliceLayers.length > 0;
  const clampedLayerIndex = hasSliceLayers
    ? Math.min(Math.max(selectedLayerIndex, 0), sliceLayers.length - 1)
    : 0;
  const selectedSliceLayer = hasSliceLayers ? sliceLayers[clampedLayerIndex] : null;

  React.useEffect(() => {
    if (!hasSliceLayers) {
      setSelectedLayerIndex(Number.MAX_SAFE_INTEGER);
      return;
    }

    setSelectedLayerIndex((current) => Math.min(Math.max(current, 0), sliceLayers.length - 1));
  }, [hasSliceLayers, sliceLayers.length]);

  React.useEffect(() => {
    if (
      !autoStartRequested ||
      model?.supported !== true ||
      autoSupportJobId ||
      hasAutoStartedRef.current
    ) {
      return;
    }

    hasAutoStartedRef.current = true;
    generateAutoSupport(undefined, {
      onSuccess: (job) => setAutoSupportJobId(job.jobId),
    });
  }, [autoStartRequested, autoSupportJobId, generateAutoSupport, model?.supported]);

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

  const isSupportedModel = model.supported === true;
  const pageTitle = isSupportedModel ? 'Support detection' : 'Auto support generation';
  const primaryButtonLabel = isSupportedModel ? 'Find supports' : 'Generate autosupport';
  const placeholderText = isSupportedModel
    ? 'Find supports to detect where the existing supports touch the model and preview those contact sizes on the slice view.'
    : 'Generate supports to preview recommended contact points for this unsupported model.';
  const progressText = isSupportedModel
    ? `Finding supports ${autoSupportJob?.progressPercent ?? 0}%`
    : `Generating supports ${autoSupportJob?.progressPercent ?? 0}%`;

  return (
    <PageLayout variant="full">
      <Stack spacing={2} className={styles.pageContent}>
        {backButton}

        <Stack spacing={0.5}>
          <Typography component="h1" variant="page-title">
            {pageTitle}
          </Typography>
          <Typography variant="body2" color="text.secondary">
            {model.name}
          </Typography>
          <Typography variant="body2" color="text.secondary">
            {isSupportedModel
              ? 'Detected support contact points are sized from the existing support mesh and aligned to the body intersection.'
              : 'Suggested support points are shown with pull-force arrows sized by magnitude.'}
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
            {isGeneratingAutoSupport ? progressText : primaryButtonLabel}
          </Button>
          {!isSupportedModel && (
            <FormControlLabel
              control={
                <Switch
                  checked={showForceMarkers}
                  onChange={(event) => setShowForceMarkers(event.target.checked)}
                />
              }
              label="Show force markers"
            />
          )}
          {isSupportedModel && (
            <FormControlLabel
              control={
                <Switch
                  checked={showSupportMesh}
                  onChange={(event) => setShowSupportMesh(event.target.checked)}
                />
              }
              label="Show support mesh"
            />
          )}
        </Stack>

        {selectedSliceLayer && (
          <Stack
            direction={{ xs: 'column', sm: 'row' }}
            spacing={2}
            alignItems={{ xs: 'flex-start', sm: 'center' }}
          >
            <Typography variant="body2" color="text.secondary">
              Layer {selectedSliceLayer.layerIndex + 1} of {sliceLayers.length} (
              {selectedSliceLayer.sliceHeightMm.toFixed(2)} mm)
            </Typography>
            {!isSupportedModel && (
              <Typography variant="body2" color="text.secondary">
                Force vectors: gravity (blue), peel (amber), rotation (red), total (size colour)
              </Typography>
            )}
          </Stack>
        )}

        <Box className={styles.viewerBox}>
          {autoSupportGeometry ? (
            <Box className={styles.viewerLayout}>
              <Box className={styles.viewerCanvas}>
                <ModelViewer
                  modelId={model.id}
                  convexHull={null}
                  concaveHull={null}
                  convexSansRaftHull={null}
                  supported={model.supported === true}
                  splitGeometryOverride={autoSupportGeometry}
                  supportPointsOverride={autoSupportJob?.supportPoints ?? null}
                  islandsOverride={isSupportedModel ? null : (autoSupportJob?.islands ?? null)}
                  sliceLayersOverride={sliceLayers}
                  selectedSliceLayerIndex={clampedLayerIndex}
                  selectedSliceHeightMm={selectedSliceLayer?.sliceHeightMm ?? null}
                  slicePreviewEnabled
                  showForceMarkers={showForceMarkers}
                  showSupportMesh={isSupportedModel ? showSupportMesh : true}
                  forceMarkersFollowSupportsToggle={!isSupportedModel}
                />
              </Box>
              <AutoSupportLayerSlider
                className={styles.layerSliderWrap}
                sliceLayers={sliceLayers}
                selectedLayerIndex={clampedLayerIndex}
                onLayerChange={setSelectedLayerIndex}
              />
            </Box>
          ) : (
            <Box className={styles.placeholderBox}>
              <Typography>
                {autoSupportJob?.status === 'failed'
                  ? (autoSupportJob.errorMessage ?? 'Support generation failed.')
                  : isGeneratingAutoSupport
                    ? 'Generating supported preview...'
                    : placeholderText}
              </Typography>
            </Box>
          )}
        </Box>
      </Stack>
    </PageLayout>
  );
}

export default ModelAutoSupportPage;
