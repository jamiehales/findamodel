import { BrowserRouter, Routes, Route } from 'react-router-dom'
import WelcomePage from './pages/WelcomePage'
import ModelPage from './pages/ModelPage'

function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<WelcomePage />} />
        <Route path="/model/:id" element={<ModelPage />} />
      </Routes>
    </BrowserRouter>
  )
}

export default App
