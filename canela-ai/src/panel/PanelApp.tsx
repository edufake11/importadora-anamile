import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useCallback, useEffect, useState } from 'react';
import { api, onSettingsChanged, type KeyStatus, type Settings } from '../lib/ipc';
import { Chat } from './Chat';
import { SettingsView } from './SettingsView';
import { TasksView } from './TasksView';
import '../styles/theme.css';
import './panel.css';

type Tab = 'chat' | 'tasks' | 'settings';

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
          <span data-tauri-drag-region>Canela</span>
        </div>
        <nav className="tabs" data-tauri-drag-region>
          <button className={tab === 'chat' ? 'on' : ''} onClick={() => setTab('chat')}>
            Chat
          </button>
          <button className={tab === 'tasks' ? 'on' : ''} onClick={() => setTab('tasks')}>
            Tareas
          </button>
          <button className={tab === 'settings' ? 'on' : ''} onClick={() => setTab('settings')}>
            Ajustes
          </button>
        </nav>
        <div className="win-buttons">
          <button title="Minimizar" aria-label="Minimizar" onClick={() => void getCurrentWindow().minimize()}>
            —
          </button>
          <button title="Cerrar (Canela sigue en el escritorio)" aria-label="Cerrar" onClick={() => void api.hidePanel()}>
            ✕
          </button>
        </div>
      </header>

      <main className="content">
        {/* El chat permanece montado para no perder la conversación al cambiar de pestaña. */}
        <div style={{ display: tab === 'chat' ? 'contents' : 'none' }}>
          <Chat settings={settings} keys={keys} onOpenSettings={() => setTab('settings')} />
        </div>
        {tab === 'tasks' && <TasksView />}
        {tab === 'settings' && <SettingsView settings={settings} keys={keys} onSaved={setSettings} onKeysChanged={refreshKeys} />}
      </main>
    </div>
  );
}
