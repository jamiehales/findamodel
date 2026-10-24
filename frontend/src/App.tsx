import { useEffect, useLayoutEffect, useState } from 'react';
import { BrowserRouter, Routes, Route, useLocation } from 'react-router-dom';
import ModelsPage from './pages/ModelsPage';
import ModelPage from './pages/ModelPage';
import PrintingListPage from './pages/PrintingListPage';
import PrintingListsManagePage from './pages/PrintingListsManagePage';
import ExplorePage from './pages/ExplorePage';
import SettingsPage from './pages/SettingsPage';
import IndexingPage from './pages/IndexingPage';
import AppHeader from './components/AppHeader';
import InitialSetupPage from './pages/InitialSetupPage';
import { RenderControlsProvider } from './components/RenderControlsContext';
import { useSetupStatus } from './lib/queries';
import LoadingView from './components/LoadingView';

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
  const requiresWizard = !!setupStatusQuery.data?.requiresWizard;
  // Saving the setup refreshes the setup status, but the wizard stays up for its
  // last screen until the person presses Finish.
  const [inWizard, setInWizard] = useState(false);

  useEffect(() => {
    if (requiresWizard) setInWizard(true);
  }, [requiresWizard]);

  if (setupStatusQuery.isLoading) {
    return <LoadingView minHeight="100vh" />;
  }

  if (inWizard || requiresWizard) {
    return (
      <InitialSetupPage
        onCompleted={() => {
          setInWizard(false);
          setupStatusQuery.refetch();
        }}
      />
    );
  }

  return (
    <BrowserRouter>
      <ScrollToTop />
      <RenderControlsProvider>
        <AppHeader />
        <Routes>
          <Route path="/" element={<ModelsPage />} />
          <Route path="/model/:id" element={<ModelPage />} />
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
