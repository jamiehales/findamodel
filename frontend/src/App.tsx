import { BrowserRouter, Routes, Route } from 'react-router-dom'
import WelcomePage from './pages/WelcomePage'
import ModelPage from './pages/ModelPage'
import PrintingListPage from './pages/PrintingListPage'
import ExplorePage from './pages/ExplorePage'
import { PrintingListProvider } from './lib/printingList'

function App() {
  return (
    <PrintingListProvider>
      <BrowserRouter>
        <Routes>
          <Route path="/" element={<WelcomePage />} />
          <Route path="/model/:id" element={<ModelPage />} />
          <Route path="/printing-list" element={<PrintingListPage />} />
          <Route path="/explore" element={<ExplorePage />} />
          <Route path="/explore/*" element={<ExplorePage />} />
        </Routes>
      </BrowserRouter>
    </PrintingListProvider>
  )
}

export default App
