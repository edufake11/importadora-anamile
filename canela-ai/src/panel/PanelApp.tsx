import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useCallback, useEffect, useState } from 'react';
import { api, onSettingsChanged, type KeyStatus, type Settings } from '../lib/ipc';
import { Chat } from './Chat';
import { SettingsView } from './SettingsView';
import '../styles/theme.css';
import './panel.css';

type Tab = 'chat' | 'settings' | 'about';

export function PanelApp() {
  const [tab, setTab] = useState<Tab>('chat');
  const [settings, setSettings] = useState<Settings | null>(null);
  const [keys, setKeys] = useState<KeyStatus>({ anthropic: false, openai: false });

  const refreshKeys = useCallback(() => void api.keyStatus().then(setKeys), []);

  useEffect(() => {
    void api.getSettings().then(setSettings);
    refreshKeys();
    const un = [onSettingsChanged(setSettings), listen('panel-shown', refreshKeys)];
    return () => un.forEach((p) => void p.then((f) => f()));
  }, [refreshKeys]);

  if (!settings) return <div className="panel loading">Cargando…</div>;

  return (
    <div className="panel">
      <header className="titlebar" data-tauri-drag-region>
        <div className="brand" data-tauri-drag-region>
          <img className="logo" src="/sprites/canela-face.png" alt="" data-tauri-drag-region />
          <span className="name" data-tauri-drag-region>
            CANELA <b>AI</b>
          </span>
        </div>
        <div className="win-buttons">
          <button title="Minimizar" onClick={() => void getCurrentWindow().minimize()}>
            —
          </button>
          <button title="Cerrar (Canela sigue en el escritorio)" onClick={() => void api.hidePanel()}>
            ✕
          </button>
        </div>
      </header>

      <nav className="tabs">
        {(
          [
            ['chat', 'Chat'],
            ['settings', 'Ajustes'],
            ['about', 'Acerca de'],
          ] as Array<[Tab, string]>
        ).map(([id, label]) => (
          <button key={id} className={tab === id ? 'on' : ''} onClick={() => setTab(id)}>
            {label}
          </button>
        ))}
      </nav>

      <main className="content">
        {/* El chat permanece montado para no perder la conversación al cambiar de pestaña. */}
        <div style={{ display: tab === 'chat' ? 'contents' : 'none' }}>
          <Chat settings={settings} keys={keys} onOpenSettings={() => setTab('settings')} />
        </div>
        {tab === 'settings' && <SettingsView settings={settings} keys={keys} onSaved={setSettings} onKeysChanged={refreshKeys} />}
        {tab === 'about' && <About />}
      </main>
    </div>
  );
}

function About() {
  return (
    <div className="about">
      <h3>Canela AI · v0.1.0 (Fase 1 — MVP)</h3>
      <p className="muted">Mascota virtual y asistente inteligente de escritorio.</p>
      <h4>Disponible en esta versión</h4>
      <ul className="feat ok">
        <li>Canela en el escritorio: camina, corre, se sienta, duerme, se despierta, escucha, habla, piensa, celebra y muestra emociones.</li>
        <li>Arrastrar con el ratón, clic, doble clic (abre el chat) y menú con clic derecho.</li>
        <li>Clic a través de las zonas transparentes de la ventana.</li>
        <li>Chat con Claude o ChatGPT con respuestas en tiempo real y contexto durante la conversación.</li>
        <li>Adjuntar archivos de texto o código para analizarlos.</li>
        <li>Claves API en el llavero seguro del sistema; icono en la bandeja y atajos globales.</li>
      </ul>
      <h4>Todavía NO implementado</h4>
      <ul className="feat todo">
        <li>Fase 2: memoria persistente (SQLite), abrir aplicaciones, organizar archivos, gestor de tareas y control de permisos.</li>
        <li>Fase 3: voz (micrófono y respuestas habladas), análisis de pantalla, automatizaciones y multitarea.</li>
        <li>Las conversaciones se pierden al cerrar la aplicación.</li>
        <li>El arte de Canela es provisional (generado por código); se reemplaza sin tocar el código.</li>
      </ul>
      <h4>Privacidad</h4>
      <p className="muted">
        Canela sólo envía datos a internet cuando tú escribes en el chat: el texto de la conversación (y los archivos que adjuntes) se envía
        al proveedor elegido. No hay telemetría. El micrófono y la pantalla no se usan en esta versión.
      </p>
    </div>
  );
}
