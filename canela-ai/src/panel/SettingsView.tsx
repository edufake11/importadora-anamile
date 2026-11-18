import { useEffect, useState } from 'react';
import { api, type KeyStatus, type ProviderId, type Settings } from '../lib/ipc';

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

export function SettingsView({ settings, keys, onSaved, onKeysChanged }: Props) {
  const [form, setForm] = useState<Settings>(settings);
  const [saved, setSaved] = useState<string | null>(null);
  useEffect(() => setForm(settings), [settings]);

  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => setForm((f) => ({ ...f, [k]: v }));
  const dirty = JSON.stringify(form) !== JSON.stringify(settings);
  const isClaude = form.provider === 'anthropic';

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
        <label className="check">
          <input type="checkbox" checked={form.wander} onChange={(e) => set('wander', e.target.checked)} />
          Pasear por el escritorio
        </label>
        <label className="check">
          <input type="checkbox" checked={form.replyBubbles} onChange={(e) => set('replyBubbles', e.target.checked)} />
          Mostrar respuestas en burbujas
        </label>
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
            <span>Abrir o cerrar este panel</span>
            <span>
              <kbd>Ctrl</kbd> <kbd>Alt</kbd> <kbd>Espacio</kbd>
            </span>
          </li>
        </ul>
      </section>

      <section>
        <details className="advanced">
          <summary>Acerca de esta versión (0.1 · Fase 1)</summary>
          <ul className="about-list">
            <li>Disponible: Canela en el escritorio, chat con Claude o ChatGPT, adjuntar archivos de texto.</li>
            <li>Aún no: memoria entre sesiones, control de Windows, voz y análisis de pantalla.</li>
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
