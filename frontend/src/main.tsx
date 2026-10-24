import { StrictMode, useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import '@fontsource/fira-sans-condensed/600.css';
import '@fontsource/fira-sans-condensed/700.css';
import '@fontsource/ibm-plex-sans/400.css';
import '@fontsource/ibm-plex-sans/500.css';
import '@fontsource/ibm-plex-sans/600.css';
import '@fontsource/ibm-plex-mono/400.css';
import '@fontsource/ibm-plex-mono/500.css';
import './styles/global.css';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { Spinner } from '@jamiehales/ui-kit/mostlymaking';
import App from './App';
import {
  getRuntimeConfig,
  initializeRuntimeConfig,
  waitForBackendReady,
} from './lib/runtimeConfig';
import styles from './main.module.css';

const queryClient = new QueryClient();

function BootstrapApp() {
  const [state, setState] = useState<'starting' | 'ready' | 'failed'>('starting');
  const [message, setMessage] = useState('Starting local services');

  useEffect(() => {
    let cancelled = false;

    const start = async () => {
      try {
        await initializeRuntimeConfig();
        const config = getRuntimeConfig();

        if (config.mode === 'desktop') {
          setMessage('Starting local services');
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
    return <App />;
  }

  return (
    <div className={styles.boot}>
      {state === 'starting' && <Spinner className={styles.spinner} />}
      <p className={styles.message}>{message}</p>
      {state === 'failed' && (
        <p className={styles.detail}>
          Check the logs in the application data folder, then restart the app.
        </p>
      )}
    </div>
  );
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <BootstrapApp />
    </QueryClientProvider>
  </StrictMode>,
);
