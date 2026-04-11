import { StrictMode, useEffect, useMemo } from 'react';
import { createRoot } from 'react-dom/client';
import './styles/tokens.css';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { ThemeProvider } from '@mui/material/styles';
import CssBaseline from '@mui/material/CssBaseline';
import App from './App';
import { buildTheme, type ThemeName } from './theme';
import { useAppConfig } from './lib/queries';

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

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <ThemedApp />
    </QueryClientProvider>
  </StrictMode>,
);
