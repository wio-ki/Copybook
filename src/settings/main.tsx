import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { AppProvider } from '../shared/app';
import { SettingsApp } from './SettingsApp';
import './settings.css';

window.addEventListener('contextmenu', (e) => {
  const el = e.target as HTMLElement | null;
  if (!el?.closest('input, textarea')) e.preventDefault();
});
window.addEventListener('keydown', (e) => {
  if (e.key === 'F5' || ((e.ctrlKey || e.metaKey) && ['r', 'f', 'g', 'j', 'u', 'p'].includes(e.key.toLowerCase()))) {
    e.preventDefault();
  }
});

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <AppProvider>
      <SettingsApp />
    </AppProvider>
  </StrictMode>,
);
