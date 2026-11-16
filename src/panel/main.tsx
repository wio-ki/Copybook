import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { AppProvider } from '../shared/app';
import { Panel } from './Panel';
import './panel.css';

// The webview's own menu and shortcuts (reload, find, print) have no place in the panel.
window.addEventListener('contextmenu', (e) => e.preventDefault());
window.addEventListener('keydown', (e) => {
  if (e.key === 'F5' || ((e.ctrlKey || e.metaKey) && ['r', 'f', 'g', 'j', 'u'].includes(e.key.toLowerCase()))) {
    e.preventDefault();
  }
});

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <AppProvider>
      <Panel />
    </AppProvider>
  </StrictMode>,
);
