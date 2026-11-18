import { getCurrentWindow } from '@tauri-apps/api/window';
import { lazy, StrictMode, Suspense } from 'react';
import { createRoot } from 'react-dom/client';

// Ambas ventanas cargan el mismo index.html; la etiqueta de la ventana decide
// qué interfaz se monta. La carga diferida mantiene separados sus estilos.
const PetApp = lazy(() => import('./pet/PetApp').then((m) => ({ default: m.PetApp })));
const PanelApp = lazy(() => import('./panel/PanelApp').then((m) => ({ default: m.PanelApp })));

const label = getCurrentWindow().label;
document.documentElement.dataset.view = label;

// Sin menú contextual del navegador ni recarga accidental con F5 en producción.
document.addEventListener('contextmenu', (e) => e.preventDefault());
if (!import.meta.env.DEV) {
  document.addEventListener('keydown', (e) => {
    if (e.key === 'F5' || (e.ctrlKey && e.key.toLowerCase() === 'r')) e.preventDefault();
  });
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Suspense fallback={null}>{label === 'pet' ? <PetApp /> : <PanelApp />}</Suspense>
  </StrictMode>,
);
