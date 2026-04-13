import { BrowserRouter, Routes, Route } from 'react-router-dom';
import ModelsPage from './pages/ModelsPage';
import ModelPage from './pages/ModelPage';
import PrintingListPage from './pages/PrintingListPage';
import PrintingListsManagePage from './pages/PrintingListsManagePage';
import ExplorePage from './pages/ExplorePage';
import SettingsPage from './pages/SettingsPage';
import IndexingPage from './pages/IndexingPage';
import NavBar from './components/NavBar';
import InitialSetupPage from './pages/InitialSetupPage';
import { useSetupStatus } from './lib/queries';
import { Container, Stack, Typography } from '@mui/material';

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
      <NavBar />
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
    </BrowserRouter>
  );
}

export default App;
