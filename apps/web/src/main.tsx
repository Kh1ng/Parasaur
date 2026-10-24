import React from 'react';
import ReactDOM from 'react-dom/client';
import { appUrl } from './lib/deployment.js';
import { App } from './App.js';
import { WebSocketProvider } from './ws/WebSocketContext.js';
import { useUiStore } from './store/uiStore.js';
import { applyTheme } from './lib/themes.js';
import './index.css';

const EmbedPage = React.lazy(() => import('./pages/EmbedPage.js').then(module => ({ default: module.EmbedPage })));

// Apply the persisted/default theme before first paint.
applyTheme(useUiStore.getState().theme);

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <WebSocketProvider>
      {window.location.pathname !== appUrl('/embed') ? <App /> : new URLSearchParams(location.search).get('standalone') === '1'
        ? <App contextView={<EmbedPage standalone />} />
        : <React.Suspense fallback={<p>Loading…</p>}><EmbedPage /></React.Suspense>}
    </WebSocketProvider>
  </React.StrictMode>
);
