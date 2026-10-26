import React, { lazy, Suspense } from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App.js';
import { takeStandaloneEmbedContext } from './lib/embed.js';
import { WebSocketProvider } from './ws/WebSocketContext.js';
import { useUiStore } from './store/uiStore.js';
import { applyTheme } from './lib/themes.js';
import { withBasePath } from './lib/basePath.js';
import './index.css';

// The embed route's root is code-split with the chat it renders; the
// dashboard's entry bundle carries neither.
const EmbedApp = lazy(() => import('./pages/EmbedPage.js').then((module) => ({ default: module.EmbedApp })));

// Apply the persisted/default theme before first paint.
applyTheme(useUiStore.getState().theme);

// The GitLab panel's chrome-less route (#1593): `${basePath}/embed`. Every
// other path is the dashboard shell; a standalone link (`/?gitlab=…`) opens
// the same view with app chrome.
const embedPaths = [withBasePath('/embed'), `${withBasePath('/embed')}/`];
const isEmbedRoute = typeof window !== 'undefined' && embedPaths.includes(window.location.pathname);
const standaloneEmbedContext = isEmbedRoute ? null : takeStandaloneEmbedContext();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <WebSocketProvider>
      {isEmbedRoute ? (
        <Suspense fallback={null}>
          <EmbedApp />
        </Suspense>
      ) : (
        <App standaloneEmbedContext={standaloneEmbedContext} />
      )}
    </WebSocketProvider>
  </React.StrictMode>
);
