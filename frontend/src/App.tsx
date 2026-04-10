import { BrowserRouter, Routes, Route } from 'react-router-dom'
import ModelsPage from './pages/ModelsPage'
import ModelPage from './pages/ModelPage'
import PrintingListPage from './pages/PrintingListPage'
import PrintingListsManagePage from './pages/PrintingListsManagePage'
import ExplorePage from './pages/ExplorePage'
import NavBar from './components/NavBar'

function App() {
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
      </Routes>
    </BrowserRouter>
  )
}

export default App
