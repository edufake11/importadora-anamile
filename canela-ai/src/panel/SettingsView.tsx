import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { useEffect, useState } from 'react';
import { api, data, type KeyStatus, type ModelInfo, type ProviderId, type Settings } from '../lib/ipc';
import { MemoryManager } from './MemoryManager';
import { RoutineManager } from './RoutineManager';
import { VoiceSettings } from './VoiceSettings';

interface Props {
  settings: Settings;
  keys: KeyStatus;
  onSaved: (s: Settings) => void;
  onKeysChanged: () => void;
}

const EFFORTS: Array<{ v: Settings['anthropicEffort']; label: string }> = [
  { v: 'low', label: 'Bajo (más rápido)' },
  { v: 'medium', label: 'Medio (recomendado)' },
  { v: 'high', label: 'Alto' },
  { v: 'xhigh', label: 'Muy alto' },
  { v: 'max', label: 'Máximo' },
];

function KeyField({ provider, label, present, onChanged }: { provider: ProviderId; label: string; present: boolean; onChanged: () => void }) {
  const [value, setValue] = useState('');
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);

  async function save() {
    setBusy(true);
    setMsg(null);
    try {
      await api.setApiKey(provider, value);
      setValue('');
      setMsg({ ok: true, text: 'Clave guardada.' });
      onChanged();
    } catch (e) {
      setMsg({ ok: false, text: String(e) });
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!confirm(`¿Eliminar la clave de ${label} de este equipo?`)) return;
    try {
      await api.deleteApiKey(provider);
      setMsg({ ok: true, text: 'Clave eliminada.' });
      onChanged();
    } catch (e) {
      setMsg({ ok: false, text: String(e) });
    }
  }

  return (
    <div className="key-field">
      <div className="key-head">
        <span>{label}</span>
        <span className={`status ${present ? 'ok' : ''}`}>{present ? 'Guardada' : 'Sin configurar'}</span>
      </div>
      <div className="row">
        <input
          type="password"
          autoComplete="off"
          spellCheck={false}
          placeholder={present ? 'Nueva clave para reemplazarla' : 'Pega tu clave aquí'}
          value={value}
          onChange={(e) => setValue(e.target.value)}
        />
        <button className="btn" onClick={() => void save()} disabled={busy || value.trim().length < 20}>
          Guardar
        </button>
        {present && (
          <button className="btn" onClick={() => void remove()}>
            Quitar
          </button>
        )}
      </div>
      {msg && <div className={`hint ${msg.ok ? '' : 'err'}`}>{msg.text}</div>}
    </div>
  );
}

/** Comprueba el modelo guardado: capacidades y precio aproximado. */
function ModelCheck({ dirty }: { dirty: boolean }) {
  const [info, setInfo] = useState<ModelInfo | null>(null);
  const [busy, setBusy] = useState(false);
  const yes = (v: boolean | null, label: string) => (v === null ? null : <span className={v ? '' : 'no'}>{label}</span>);
  return (
    <div className="field">
      <button
        className="btn"
        disabled={busy}
        onClick={() => {
          setBusy(true);
          void api
            .modelInfo()
            .then(setInfo)
            .finally(() => setBusy(false));
        }}
      >
        {busy ? 'Comprobando…' : 'Comprobar modelo'}
      </button>
      {dirty && <small>Guarda los cambios antes de comprobar.</small>}
      {info && (
        <>
          <div className="model-info">
            {info.exists === false && <span className="no">No disponible</span>}
            {yes(info.supportsImages, 'Imágenes')}
            {yes(info.supportsThinking, 'Razonamiento')}
            {yes(info.supportsEffort, 'Nivel de razonamiento')}
            {info.contextTokens && <span>Contexto {Math.round(info.contextTokens / 1000)} mil tokens</span>}
            {info.price && (
              <span>
                ≈ {info.price.input} / {info.price.output} US$ por millón (entrada / salida)
              </span>
            )}
            {!info.price && <span>Precio desconocido</span>}
          </div>
          <small>
            {info.source === 'api' ? 'Comprobado en vivo con la API.' : 'Valores conocidos (sin comprobar en vivo).'} {info.note ?? ''} Los precios son aproximados.
          </small>
        </>
      )}
    </div>
  );
}

function Toggle({ checked, onChange, children }: { checked: boolean; onChange: (v: boolean) => void; children: React.ReactNode }) {
  return (
    <label className="check">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      {children}
    </label>
  );
}

export function SettingsView({ settings, keys, onSaved, onKeysChanged }: Props) {
  const [form, setForm] = useState<Settings>(settings);
  const [saved, setSaved] = useState<string | null>(null);
  const [folders, setFolders] = useState<string[]>([]);
  const [docStats, setDocStats] = useState<{ files: number; updatedAt: string | null } | null>(null);
  useEffect(() => {
    void data.docIndexStats().then(setDocStats).catch(() => undefined);
  }, []);
  useEffect(() => setForm(settings), [settings]);
  useEffect(() => {
    void api.allowedFolders().then(setFolders);
  }, [settings]);

  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => setForm((f) => ({ ...f, [k]: v }));
  const dirty = JSON.stringify(form) !== JSON.stringify(settings);
  const isClaude = form.provider === 'anthropic';
  const effectiveFolders = form.allowedFolders ?? folders;

  async function save() {
    try {
      const s = await api.saveSettings(form);
      onSaved(s);
      setSaved('Guardado');
    } catch (e) {
      setSaved(String(e));
    }
    setTimeout(() => setSaved(null), 2000);
  }

  async function addFolder() {
    const picked = await openDialog({ directory: true, multiple: false, title: 'Elige una carpeta para Canela' });
    if (typeof picked === 'string' && !effectiveFolders.includes(picked)) set('allowedFolders', [...effectiveFolders, picked]);
  }

  return (
    <div className="settings">
      <section>
        <h3>Asistente</h3>
        <div className="segmented" role="tablist">
          <button className={isClaude ? 'on' : ''} onClick={() => set('provider', 'anthropic')}>
            Claude
          </button>
          <button className={!isClaude ? 'on' : ''} onClick={() => set('provider', 'openai')}>
            ChatGPT
          </button>
        </div>

        {isClaude ? (
          <KeyField provider="anthropic" label="Clave de Anthropic" present={keys.anthropic} onChanged={onKeysChanged} />
        ) : (
          <KeyField provider="openai" label="Clave de OpenAI" present={keys.openai} onChanged={onKeysChanged} />
        )}
        <p className="muted">La clave se guarda cifrada en el Administrador de credenciales de Windows.</p>

        <details className="advanced">
          <summary>Opciones avanzadas</summary>
          <ModelCheck dirty={form.anthropicModel !== settings.anthropicModel || form.openaiModel !== settings.openaiModel || form.provider !== settings.provider} />
          {isClaude ? (
            <>
              <label className="field">
                <span>Modelo</span>
                <input value={form.anthropicModel} onChange={(e) => set('anthropicModel', e.target.value)} spellCheck={false} />
              </label>
              <label className="field">
                <span>Nivel de razonamiento</span>
                <select value={form.anthropicEffort} onChange={(e) => set('anthropicEffort', e.target.value as Settings['anthropicEffort'])}>
                  {EFFORTS.map((o) => (
                    <option key={o.v} value={o.v}>
                      {o.label}
                    </option>
                  ))}
                </select>
              </label>
            </>
          ) : (
            <label className="field">
              <span>Modelo</span>
              <input value={form.openaiModel} onChange={(e) => set('openaiModel', e.target.value)} spellCheck={false} />
              <small>Identificador de un modelo de chat disponible en tu cuenta.</small>
            </label>
          )}
        </details>
      </section>

      <VoiceSettings form={form} set={set} keys={keys} onSaved={onSaved} />

      <section>
        <h3>Canela</h3>
        <label className="field">
          <span>Tu nombre</span>
          <input value={form.userName} maxLength={40} onChange={(e) => set('userName', e.target.value)} placeholder="Cómo quieres que te llame" />
        </label>
        <label className="field">
          <span>Tamaño · {form.petScale}x</span>
          <input type="range" min={1} max={6} value={form.petScale} onChange={(e) => set('petScale', Number(e.target.value))} />
        </label>
        <Toggle checked={form.wander} onChange={(v) => set('wander', v)}>
          Pasear por el escritorio
        </Toggle>
        <Toggle checked={form.replyBubbles} onChange={(v) => set('replyBubbles', v)}>
          Mostrar respuestas en burbujas
        </Toggle>
        <Toggle checked={form.reduceMotion} onChange={(v) => set('reduceMotion', v)}>
          Animaciones reducidas (consume menos)
        </Toggle>
        <Toggle checked={form.hideInFullscreen} onChange={(v) => set('hideInFullscreen', v)}>
          Ocultarse con juegos o videos en pantalla completa
        </Toggle>
      </section>

      <section>
        <h3>Al iniciar</h3>
        <Toggle checked={form.launchAtStartup} onChange={(v) => set('launchAtStartup', v)}>
          Abrir Canela al encender la computadora
        </Toggle>
        <Toggle checked={form.greetOnStartup} onChange={(v) => set('greetOnStartup', v)}>
          Saludarme y mostrar mis tareas de hoy
        </Toggle>
      </section>

      <section>
        <h3>Memoria</h3>
        <p className="muted">Canela sólo guarda lo que le pides recordar. Nunca guarda contraseñas ni datos de tarjetas.</p>
        <details className="advanced">
          <summary>Ver y editar recuerdos</summary>
          <MemoryManager />
        </details>
        <Toggle checked={form.saveConversations} onChange={(v) => set('saveConversations', v)}>
          Guardar el historial de chats en este equipo
        </Toggle>
        <button
          className="text-button"
          onClick={() => confirm('¿Borrar todas las conversaciones guardadas?') && void data.conversationClear()}
        >
          Borrar historial de chats
        </button>
        <p className="muted">
          Índice de documentos (para buscar dentro de tus archivos, se crea sólo cuando se lo pides a Canela):{' '}
          {docStats ? `${docStats.files} archivos` : '…'}
        </p>
        {docStats && docStats.files > 0 && (
          <button
            className="text-button"
            onClick={() => confirm('¿Borrar el índice de documentos? Tus archivos no se tocan.') && void data.docIndexClear().then(() => setDocStats({ files: 0, updatedAt: null }))}
          >
            Borrar índice de documentos
          </button>
        )}
      </section>

      <section>
        <h3>Automatizaciones</h3>
        <p className="muted">
          Canela puede encadenar varias acciones (un «plan»). Siempre te muestra la lista de pasos antes de empezar, y las acciones delicadas
          (borrar, mover archivos, capturas) te las vuelve a preguntar. Puedes pararlo con el botón ■.
        </p>
        <details className="advanced">
          <summary>Rutinas guardadas</summary>
          <RoutineManager />
        </details>
      </section>

      <section>
        <h3>Permisos</h3>
        <p className="muted">Canela sólo puede ver y cambiar archivos dentro de estas carpetas. Siempre te pedirá permiso antes de crear, mover o modificar algo.</p>
        <ul className="folder-list">
          {effectiveFolders.map((f) => (
            <li key={f}>
              <span title={f}>{f}</span>
              <button className="icon-btn" aria-label="Quitar carpeta" title="Quitar" onClick={() => set('allowedFolders', effectiveFolders.filter((x) => x !== f))}>
                ✕
              </button>
            </li>
          ))}
        </ul>
        <button className="btn" onClick={() => void addFolder()}>
          Añadir carpeta
        </button>
        <Toggle checked={form.autoOpenApps} onChange={(v) => set('autoOpenApps', v)}>
          Abrir aplicaciones y carpetas sin preguntar
        </Toggle>
        <Toggle checked={form.autoOpenWeb} onChange={(v) => set('autoOpenWeb', v)}>
          Abrir páginas web sin preguntar
        </Toggle>
        <Toggle checked={form.confirmFileSharing} onChange={(v) => set('confirmFileSharing', v)}>
          Preguntarme antes de enviar el contenido de mis archivos a la IA
        </Toggle>
        <Toggle checked={form.visionEnabled} onChange={(v) => set('visionEnabled', v)}>
          Permitir capturas de pantalla (siempre con vista previa y tu permiso)
        </Toggle>
      </section>

      <section>
        <h3>Atajos</h3>
        <ul className="shortcuts">
          <li>
            <span>Mostrar u ocultar a Canela</span>
            <span>
              <kbd>Ctrl</kbd> <kbd>Alt</kbd> <kbd>C</kbd>
            </span>
          </li>
          <li>
            <span>Hablar con Canela</span>
            <span>
              <kbd>Ctrl</kbd> <kbd>Alt</kbd> <kbd>V</kbd>
            </span>
          </li>
          <li>
            <span>Abrir o cerrar este panel</span>
            <span>
              <kbd>Ctrl</kbd> <kbd>Alt</kbd> <kbd>Espacio</kbd>
            </span>
          </li>
        </ul>
      </section>

      <section>
        <details className="advanced">
          <summary>Acerca de esta versión (0.2 · Fase 2)</summary>
          <ul className="about-list">
            <li>Disponible: chat con Claude o ChatGPT, memoria, tareas y recordatorios, abrir apps y páginas, buscar y organizar archivos en carpetas permitidas.</li>
            <li>Aún no: voz, análisis de pantalla, Google Calendar, Gmail y GitHub.</li>
            <li>Privacidad: sólo se envía a internet lo que escribes o adjuntas en el chat. Sin telemetría.</li>
          </ul>
        </details>
      </section>

      <div className="save-bar">
        {saved && <span className="hint">{saved}</span>}
        <button className="btn primary" disabled={!dirty} onClick={() => void save()}>
          Guardar cambios
        </button>
      </div>
    </div>
  );
}
