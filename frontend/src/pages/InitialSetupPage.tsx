import { useEffect, useState } from 'react';
import {
  Alert,
  Button,
  Card,
  CardContent,
  Checkbox,
  Container,
  FormControl,
  FormControlLabel,
  Grid,
  InputLabel,
  MenuItem,
  Select,
  Stack,
  TextField,
  Typography,
} from '@mui/material';
import { useCompleteInitialSetup, useInitialSetupDefaults } from '../lib/queries';

interface InitialSetupPageProps {
  onCompleted: () => void;
}

function InitialSetupPage({ onCompleted }: InitialSetupPageProps) {
  const defaultsQuery = useInitialSetupDefaults(true);
  const completeSetupMutation = useCompleteInitialSetup();
  const [step, setStep] = useState<1 | 2>(1);

  const [modelsDirectoryPath, setModelsDirectoryPath] = useState('');
  const [defaultRaftHeightMm, setDefaultRaftHeightMm] = useState('2');
  const [theme, setTheme] = useState('nord');
  const [generatePreviewsEnabled, setGeneratePreviewsEnabled] = useState(true);
  const [tagGenerationEnabled, setTagGenerationEnabled] = useState(false);
  const [aiDescriptionEnabled, setAiDescriptionEnabled] = useState(false);
  const [tagGenerationProvider, setTagGenerationProvider] = useState<'internal' | 'ollama'>(
    'internal',
  );
  const [tagGenerationEndpoint, setTagGenerationEndpoint] = useState('http://localhost:11434');
  const [tagGenerationModel, setTagGenerationModel] = useState('Qwen2.5-7B-Instruct');
  const [tagGenerationTimeoutMs, setTagGenerationTimeoutMs] = useState('60000');
  const [tagGenerationMaxTags, setTagGenerationMaxTags] = useState('12');
  const [tagGenerationMinConfidence, setTagGenerationMinConfidence] = useState('0.45');
  const [errorText, setErrorText] = useState<string | null>(null);

  const raftHeight = Number(defaultRaftHeightMm);
  const timeoutMs = Number(tagGenerationTimeoutMs);
  const maxTags = Number(tagGenerationMaxTags);
  const minConfidence = Number(tagGenerationMinConfidence);

  const isThemeValid = theme === 'default' || theme === 'nord';
  const isStepOneValid =
    modelsDirectoryPath.trim().length > 0 &&
    Number.isFinite(raftHeight) &&
    raftHeight >= 0 &&
    isThemeValid;
  const isTagProviderValid =
    tagGenerationProvider === 'internal' || tagGenerationProvider === 'ollama';
  const isAiSettingsValid =
    tagGenerationEndpoint.trim().length > 0 &&
    tagGenerationModel.trim().length > 0 &&
    Number.isInteger(timeoutMs) &&
    timeoutMs >= 1000 &&
    timeoutMs <= 300000 &&
    Number.isInteger(maxTags) &&
    maxTags >= 1 &&
    maxTags <= 100 &&
    Number.isFinite(minConfidence) &&
    minConfidence >= 0 &&
    minConfidence <= 1 &&
    isTagProviderValid;

  useEffect(() => {
    if (!defaultsQuery.data) {
      return;
    }

    const defaults = defaultsQuery.data;
    setModelsDirectoryPath(defaults.modelsDirectoryPath ?? '');
    setDefaultRaftHeightMm(String(defaults.defaultRaftHeightMm));
    setTheme(defaults.theme === 'default' ? 'default' : 'nord');
    setGeneratePreviewsEnabled(defaults.generatePreviewsEnabled);
    // AI features are intentionally opt-in during initial setup.
    setTagGenerationEnabled(false);
    setAiDescriptionEnabled(false);
    setTagGenerationProvider(defaults.tagGenerationProvider === 'ollama' ? 'ollama' : 'internal');
    setTagGenerationEndpoint(defaults.tagGenerationEndpoint);
    setTagGenerationModel(defaults.tagGenerationModel);
    setTagGenerationTimeoutMs(String(defaults.tagGenerationTimeoutMs));
    setTagGenerationMaxTags(String(defaults.tagGenerationMaxTags));
    setTagGenerationMinConfidence(String(defaults.tagGenerationMinConfidence));
  }, [defaultsQuery.data]);

  const handleSubmit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setErrorText(null);

    if (!isStepOneValid) {
      if (!modelsDirectoryPath.trim()) {
        setErrorText('Model root path is required.');
        return;
      }

      if (!Number.isFinite(raftHeight) || raftHeight < 0) {
        setErrorText('Default raft height must be a valid number greater than or equal to 0.');
        return;
      }

      setErrorText('Please complete the required fields on page 1.');
      return;
    }

    if (!isAiSettingsValid) {
      if (!tagGenerationEndpoint.trim()) {
        setErrorText('Tag generation endpoint is required.');
        return;
      }

      if (!tagGenerationModel.trim()) {
        setErrorText('Tag generation model is required.');
        return;
      }

      if (!Number.isInteger(timeoutMs) || timeoutMs < 1000 || timeoutMs > 300000) {
        setErrorText('Tag generation timeout must be an integer between 1000 and 300000.');
        return;
      }

      if (!Number.isInteger(maxTags) || maxTags < 1 || maxTags > 100) {
        setErrorText('Tag generation max tags must be an integer between 1 and 100.');
        return;
      }

      if (!Number.isFinite(minConfidence) || minConfidence < 0 || minConfidence > 1) {
        setErrorText('Tag generation minimum confidence must be between 0 and 1.');
        return;
      }

      setErrorText('Please complete the required AI settings on page 2.');
      return;
    }

    try {
      await completeSetupMutation.mutateAsync({
        modelsDirectoryPath: modelsDirectoryPath.trim(),
        defaultRaftHeightMm: raftHeight,
        theme,
        generatePreviewsEnabled,
        tagGenerationEnabled,
        aiDescriptionEnabled,
        tagGenerationProvider,
        tagGenerationEndpoint: tagGenerationEndpoint.trim(),
        tagGenerationModel: tagGenerationModel.trim(),
        tagGenerationTimeoutMs: timeoutMs,
        tagGenerationMaxTags: maxTags,
        tagGenerationMinConfidence: minConfidence,
      });

      onCompleted();
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to complete setup.';
      setErrorText(message);
    }
  };

  const handleNext = () => {
    if (!isStepOneValid) {
      setErrorText('Model root path is required.');
      return;
    }
    setErrorText(null);
    setStep(2);
  };

  if (defaultsQuery.isLoading) {
    return (
      <Container maxWidth="md">
        <Stack spacing={2} alignItems="center">
          <Typography variant="h4">Initial Setup</Typography>
          <Typography variant="body1">Loading setup defaults...</Typography>
        </Stack>
      </Container>
    );
  }

  if (defaultsQuery.isError) {
    return (
      <Container maxWidth="md">
        <Stack spacing={2}>
          <Typography variant="h4">Initial Setup</Typography>
          <Alert severity="error">Failed to load setup defaults.</Alert>
        </Stack>
      </Container>
    );
  }

  return (
    <Container maxWidth="md">
      <Stack spacing={3}>
        <Typography variant="h4">Initial Setup</Typography>
        <Typography variant="body1">Step {step} of 2</Typography>

        <Card>
          <CardContent>
            <Stack component="form" spacing={3} onSubmit={handleSubmit}>
              {errorText ? <Alert severity="error">{errorText}</Alert> : null}

              {step === 1 ? (
                <Stack spacing={3}>
                  <Typography variant="h6">Model Setup</Typography>

                  <TextField
                    label="Model Root Path"
                    value={modelsDirectoryPath}
                    onChange={(e) => setModelsDirectoryPath(e.target.value)}
                    required
                    fullWidth
                  />

                  <Grid container spacing={2}>
                    <Grid size={{ xs: 12, md: 6 }}>
                      <TextField
                        label="Default Raft Height (mm)"
                        value={defaultRaftHeightMm}
                        onChange={(e) => setDefaultRaftHeightMm(e.target.value)}
                        fullWidth
                      />
                    </Grid>
                    <Grid size={{ xs: 12, md: 6 }}>
                      <FormControl fullWidth>
                        <InputLabel id="theme-select-label">Theme</InputLabel>
                        <Select
                          labelId="theme-select-label"
                          value={theme}
                          label="Theme"
                          onChange={(e) => setTheme(String(e.target.value))}
                        >
                          <MenuItem value="default">Default</MenuItem>
                          <MenuItem value="nord">Nord</MenuItem>
                        </Select>
                      </FormControl>
                    </Grid>
                  </Grid>

                  <Stack direction="row" justifyContent="flex-end">
                    <Button
                      variant={isStepOneValid ? 'affirmative' : 'outlined'}
                      onClick={handleNext}
                      disabled={!isStepOneValid}
                    >
                      Next
                    </Button>
                  </Stack>
                </Stack>
              ) : (
                <Stack spacing={3}>
                  <Typography variant="h6">AI Setup</Typography>
                  <Typography variant="body2">
                    AI features are disabled by default. Enable them only if you want tag generation
                    and image descriptions during indexing.
                  </Typography>

                  <Grid container spacing={2}>
                    <Grid size={{ xs: 12, md: 6 }}>
                      <FormControlLabel
                        control={
                          <Checkbox
                            checked={tagGenerationEnabled}
                            onChange={(e) => setTagGenerationEnabled(e.target.checked)}
                          />
                        }
                        label="Enable AI Tag Generation"
                      />
                    </Grid>
                    <Grid size={{ xs: 12, md: 6 }}>
                      <FormControlLabel
                        control={
                          <Checkbox
                            checked={aiDescriptionEnabled}
                            onChange={(e) => setAiDescriptionEnabled(e.target.checked)}
                          />
                        }
                        label="Enable AI Descriptions"
                      />
                    </Grid>
                  </Grid>

                  <Grid container spacing={2}>
                    <Grid size={{ xs: 12, md: 6 }}>
                      <FormControl fullWidth>
                        <InputLabel id="provider-select-label">Tag Provider</InputLabel>
                        <Select
                          labelId="provider-select-label"
                          value={tagGenerationProvider}
                          label="Tag Provider"
                          onChange={(e) =>
                            setTagGenerationProvider(
                              e.target.value === 'ollama' ? 'ollama' : 'internal',
                            )
                          }
                        >
                          <MenuItem value="internal">Internal</MenuItem>
                          <MenuItem value="ollama">Ollama</MenuItem>
                        </Select>
                      </FormControl>
                    </Grid>
                    <Grid size={{ xs: 12, md: 6 }}>
                      <TextField
                        label="Tag Model"
                        value={tagGenerationModel}
                        onChange={(e) => setTagGenerationModel(e.target.value)}
                        fullWidth
                      />
                    </Grid>
                  </Grid>

                  <TextField
                    label="Tag Endpoint"
                    value={tagGenerationEndpoint}
                    onChange={(e) => setTagGenerationEndpoint(e.target.value)}
                    fullWidth
                  />

                  <Grid container spacing={2}>
                    <Grid size={{ xs: 12, md: 4 }}>
                      <TextField
                        label="Timeout (ms)"
                        value={tagGenerationTimeoutMs}
                        onChange={(e) => setTagGenerationTimeoutMs(e.target.value)}
                        fullWidth
                      />
                    </Grid>
                    <Grid size={{ xs: 12, md: 4 }}>
                      <TextField
                        label="Max Tags"
                        value={tagGenerationMaxTags}
                        onChange={(e) => setTagGenerationMaxTags(e.target.value)}
                        fullWidth
                      />
                    </Grid>
                    <Grid size={{ xs: 12, md: 4 }}>
                      <TextField
                        label="Min Confidence"
                        value={tagGenerationMinConfidence}
                        onChange={(e) => setTagGenerationMinConfidence(e.target.value)}
                        fullWidth
                      />
                    </Grid>
                  </Grid>

                  <Stack direction="row" justifyContent="space-between">
                    <Button variant="text" onClick={() => setStep(1)}>
                      Previous
                    </Button>
                    <Button
                      type="submit"
                      variant={isAiSettingsValid ? 'affirmative' : 'outlined'}
                      disabled={completeSetupMutation.isPending || !isAiSettingsValid}
                    >
                      {completeSetupMutation.isPending ? 'Saving...' : 'Complete Setup'}
                    </Button>
                  </Stack>
                </Stack>
              )}
            </Stack>
          </CardContent>
        </Card>
      </Stack>
    </Container>
  );
}

export default InitialSetupPage;
