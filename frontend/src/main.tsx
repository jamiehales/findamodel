import { StrictMode, useEffect, useMemo, useState } from 'react';
import { createRoot } from 'react-dom/client';
import './styles/tokens.css';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ThemeProvider } from '@mui/material/styles';
import CssBaseline from '@mui/material/CssBaseline';
import CircularProgress from '@mui/material/CircularProgress';
import Stack from '@mui/material/Stack';
import Typography from '@mui/material/Typography';
import App from './App';
import { buildTheme, type ThemeName } from './theme';
import { useAppConfig } from './lib/queries';
import {
  getRuntimeConfig,
  initializeRuntimeConfig,
  waitForBackendReady,
} from './lib/runtimeConfig';

const queryClient = new QueryClient();

function ThemedApp() {
  const { data: appConfig } = useAppConfig();
  const themeName = (appConfig?.theme ?? 'nord') as ThemeName;
  const theme = useMemo(() => buildTheme(themeName), [themeName]);

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', themeName);
  }, [themeName]);

  return (
    <ThemeProvider theme={theme}>
      <CssBaseline />
      <App />
    </ThemeProvider>
  );
}

function BootstrapApp() {
  const [state, setState] = useState<'starting' | 'ready' | 'failed'>('starting');
  const [message, setMessage] = useState('Starting local services...');

  useEffect(() => {
    let cancelled = false;

    const start = async () => {
      try {
        await initializeRuntimeConfig();
        const config = getRuntimeConfig();

        if (config.mode === 'desktop') {
          setMessage('Starting local services...');
          await waitForBackendReady();
        }

        if (!cancelled) {
          setState('ready');
        }
      } catch (error) {
        if (cancelled) return;
        const details = error instanceof Error ? error.message : 'Unknown startup error';
        setMessage(`Desktop startup timed out. ${details}`);
        setState('failed');
      }
    };

    void start();
    return () => {
      cancelled = true;
    };
  }, []);

  if (state === 'ready') {
    return <ThemedApp />;
  }

  return (
    <Stack
      alignItems="center"
      justifyContent="center"
      spacing={2}
      style={{ minHeight: '100vh', padding: '1.5rem', textAlign: 'center' }}
    >
      {state === 'starting' && <CircularProgress color="primary" />}
      <Typography variant="h6">{message}</Typography>
      {state === 'failed' && (
        <Typography variant="body2" color="text.secondary">
          Check logs in the application data directory and restart the app.
        </Typography>
      )}
    </Stack>
  );
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <BootstrapApp />
    </QueryClientProvider>
  </StrictMode>,
);
