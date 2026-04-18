import { useEffect, useLayoutEffect } from 'react';
import { BrowserRouter, Routes, Route, useLocation } from 'react-router-dom';
import ModelsPage from './pages/ModelsPage';
import ModelPage from './pages/ModelPage';
import ModelAutoSupportPage from './pages/ModelAutoSupportPage';
import PrintingListPage from './pages/PrintingListPage';
import PrintingListsManagePage from './pages/PrintingListsManagePage';
import ExplorePage from './pages/ExplorePage';
import SettingsPage from './pages/SettingsPage';
import IndexingPage from './pages/IndexingPage';
import NavBar from './components/NavBar';
import InitialSetupPage from './pages/InitialSetupPage';
import { RenderControlsProvider } from './components/RenderControlsContext';
import { useSetupStatus } from './lib/queries';
import { Container, Stack, Typography } from '@mui/material';

function ScrollToTop() {
  const { pathname } = useLocation();

  useEffect(() => {
    const previous = window.history.scrollRestoration;
    window.history.scrollRestoration = 'manual';

    return () => {
      window.history.scrollRestoration = previous;
    };
  }, []);

  useLayoutEffect(() => {
    window.scrollTo({ top: 0, left: 0, behavior: 'auto' });
  }, [pathname]);

  return null;
}

function App() {
  const setupStatusQuery = useSetupStatus();

  if (setupStatusQuery.isLoading) {
    return (
      <Container maxWidth="md">
        <Stack spacing={2} alignItems="center">
          <Typography variant="h5">Loading Application</Typography>
          <Typography variant="body1">Checking setup status...</Typography>
        </Stack>
      </Container>
    );
  }

  if (setupStatusQuery.data?.requiresWizard) {
    return (
      <InitialSetupPage
        onCompleted={() => {
          setupStatusQuery.refetch();
        }}
      />
    );
  }

  return (
    <BrowserRouter>
      <ScrollToTop />
      <RenderControlsProvider>
        <NavBar />
        <Routes>
          <Route path="/" element={<ModelsPage />} />
          <Route path="/model/:id" element={<ModelPage />} />
          <Route path="/model/:id/supports" element={<ModelAutoSupportPage />} />
          <Route path="/printing-list/:listId" element={<PrintingListPage />} />
          <Route path="/printing-lists" element={<PrintingListsManagePage />} />
          <Route path="/explore" element={<ExplorePage />} />
          <Route path="/explore/*" element={<ExplorePage />} />
          <Route path="/indexing" element={<IndexingPage />} />
          <Route path="/settings/*" element={<SettingsPage />} />
        </Routes>
      </RenderControlsProvider>
    </BrowserRouter>
  );
}

export default App;
