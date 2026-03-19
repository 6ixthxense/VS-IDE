import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import { ErrorBoundary } from './components/ErrorBoundary'
import { ensureElectronApi } from './utils/electronApiFallback'
import './styles/app.css'

ensureElectronApi()

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <ErrorBoundary fallbackMessage="Application crashed — click Retry to restart">
      <App />
    </ErrorBoundary>
  </React.StrictMode>
)
