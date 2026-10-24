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

  const [modelsDirectoryPath, setModelsDirectoryPath] = useState('');
  const [defaultRaftHeightMm, setDefaultRaftHeightMm] = useState('2');
  const [theme, setTheme] = useState('nord');
  const [generatePreviewsEnabled, setGeneratePreviewsEnabled] = useState(true);
  const [errorText, setErrorText] = useState<string | null>(null);

  const raftHeight = Number(defaultRaftHeightMm);

  const isThemeValid = theme === 'default' || theme === 'nord';
  const isValid =
    modelsDirectoryPath.trim().length > 0 &&
    Number.isFinite(raftHeight) &&
    raftHeight >= 0 &&
    isThemeValid;

  useEffect(() => {
    if (!defaultsQuery.data) {
      return;
    }

    const defaults = defaultsQuery.data;
    setModelsDirectoryPath(defaults.modelsDirectoryPath ?? '');
    setDefaultRaftHeightMm(String(defaults.defaultRaftHeightMm));
    setTheme(defaults.theme === 'default' ? 'default' : 'nord');
    setGeneratePreviewsEnabled(defaults.generatePreviewsEnabled);
  }, [defaultsQuery.data]);

  const handleSubmit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    setErrorText(null);

    if (!isValid) {
      if (!modelsDirectoryPath.trim()) {
        setErrorText('Model root path is required.');
        return;
      }

      if (!Number.isFinite(raftHeight) || raftHeight < 0) {
        setErrorText('Default raft height must be a valid number greater than or equal to 0.');
        return;
      }

      setErrorText('Please complete the required fields.');
      return;
    }

    try {
      await completeSetupMutation.mutateAsync({
        modelsDirectoryPath: modelsDirectoryPath.trim(),
        defaultRaftHeightMm: raftHeight,
        theme,
        generatePreviewsEnabled,
      });

      onCompleted();
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to complete setup.';
      setErrorText(message);
    }
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

        <Card>
          <CardContent>
            <Stack component="form" spacing={3} onSubmit={handleSubmit}>
              {errorText ? <Alert severity="error">{errorText}</Alert> : null}

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

                <Grid container spacing={2}>
                  <Grid size={{ xs: 12, md: 6 }}>
                    <FormControlLabel
                      control={
                        <Checkbox
                          checked={generatePreviewsEnabled}
                          onChange={(e) => setGeneratePreviewsEnabled(e.target.checked)}
                        />
                      }
                      label="Generate previews during indexing"
                    />
                  </Grid>
                </Grid>

                <Stack direction="row" justifyContent="flex-end">
                  <Button
                    type="submit"
                    variant={isValid ? 'affirmative' : 'outlined'}
                    disabled={completeSetupMutation.isPending || !isValid}
                  >
                    {completeSetupMutation.isPending ? 'Saving...' : 'Complete Setup'}
                  </Button>
                </Stack>
              </Stack>
            </Stack>
          </CardContent>
        </Card>
      </Stack>
    </Container>
  );
}

export default InitialSetupPage;
