import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { BrowserRouter } from 'react-router-dom'
import { App } from './App'
import { AuthProvider } from './state/auth'
import { StandingProvider } from './state/standing'
import './styles/app.css'
import { adoptLegacyStorage } from './state/legacyStorage'

// Before anything reads storage: keys from before the rename move first.
adoptLegacyStorage()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <BrowserRouter>
      <AuthProvider>
        <StandingProvider>
          <App />
        </StandingProvider>
      </AuthProvider>
    </BrowserRouter>
  </StrictMode>,
)
