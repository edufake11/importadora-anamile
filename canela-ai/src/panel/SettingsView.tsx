import { useEffect, useState } from 'react';
import { api, type KeyStatus, type ProviderId, type Settings } from '../lib/ipc';

interface Props {
  settings: Settings;
  keys: KeyStatus;
  onSaved: (s: Settings) => void;
  onKeysChanged: () => void;
}

const EFFORTS: Array<{ v: Settings['anthropicEffort']; label: string }> = [
  { v: 'low', label: 'Bajo — respuestas más rápidas' },
  { v: 'medium', label: 'Medio — equilibrado (recomendado)' },
  { v: 'high', label: 'Alto — más razonamiento' },
  { v: 'xhigh', label: 'Muy alto — programación compleja' },
  { v: 'max', label: 'Máximo — sin límite de esfuerzo' },
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
      setMsg({ ok: true, text: 'Clave guardada en el llavero seguro del sistema.' });
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
        <strong>{label}</strong>
        <span className={`status ${present ? 'ok' : ''}`}>{present ? '● Guardada' : '○ Sin configurar'}</span>
      </div>
      <div className="row">
        <input
          type="password"
          autoComplete="off"
          spellCheck={false}
          placeholder={present ? 'Escribe una nueva clave para reemplazarla' : 'Pega aquí tu clave API'}
          value={value}
          onChange={(e) => setValue(e.target.value)}
        />
        <button onClick={() => void save()} disabled={busy || value.trim().length < 20}>
          Guardar
        </button>
        {present && (
          <button className="danger" onClick={() => void remove()}>
            Eliminar
          </button>
        )}
      </div>
      {msg && <div className={`hint ${msg.ok ? 'ok' : 'err'}`}>{msg.text}</div>}
    </div>
  );
}

export function SettingsView({ settings, keys, onSaved, onKeysChanged }: Props) {
  const [form, setForm] = useState<Settings>(settings);
  const [saved, setSaved] = useState<string | null>(null);
  useEffect(() => setForm(settings), [settings]);

  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => setForm((f) => ({ ...f, [k]: v }));
  const dirty = JSON.stringify(form) !== JSON.stringify(settings);

  async function save() {
    try {
      const s = await api.saveSettings(form);
      onSaved(s);
      setSaved('Ajustes guardados.');
    } catch (e) {
      setSaved(String(e));
    }
    setTimeout(() => setSaved(null), 2500);
  }

  return (
    <div className="settings">
      <section>
        <h3>Inteligencia artificial</h3>
        <label className="field">
          <span>Proveedor del chat</span>
          <div className="segmented">
            <button className={form.provider === 'anthropic' ? 'on' : ''} onClick={() => set('provider', 'anthropic')}>
              Claude (Anthropic)
            </button>
            <button className={form.provider === 'openai' ? 'on' : ''} onClick={() => set('provider', 'openai')}>
              ChatGPT (OpenAI)
            </button>
          </div>
        </label>

        {form.provider === 'anthropic' ? (
          <>
            <label className="field">
              <span>Modelo de Claude</span>
              <input value={form.anthropicModel} onChange={(e) => set('anthropicModel', e.target.value)} spellCheck={false} />
            </label>
            <label className="field">
              <span>Esfuerzo de razonamiento</span>
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
            <span>Modelo de OpenAI</span>
            <input value={form.openaiModel} onChange={(e) => set('openaiModel', e.target.value)} spellCheck={false} />
            <small>Usa el identificador exacto de un modelo de chat disponible en tu cuenta de OpenAI.</small>
          </label>
        )}
      </section>

      <section>
        <h3>Claves API</h3>
        <p className="muted">
          Se guardan cifradas en el Administrador de credenciales de Windows, nunca en archivos ni en el código. La interfaz no puede
          leerlas: sólo el núcleo nativo las usa al llamar a la API.
        </p>
        <KeyField provider="anthropic" label="Anthropic (Claude)" present={keys.anthropic} onChanged={onKeysChanged} />
        <KeyField provider="openai" label="OpenAI (ChatGPT)" present={keys.openai} onChanged={onKeysChanged} />
      </section>

      <section>
        <h3>Canela</h3>
        <label className="field">
          <span>¿Cómo quieres que te llame?</span>
          <input value={form.userName} maxLength={40} onChange={(e) => set('userName', e.target.value)} placeholder="Tu nombre" />
          <small>Se aplica a partir de la próxima conversación.</small>
        </label>
        <label className="field">
          <span>Tamaño de Canela: {form.petScale}x</span>
          <input type="range" min={1} max={6} value={form.petScale} onChange={(e) => set('petScale', Number(e.target.value))} />
        </label>
        <label className="check">
          <input type="checkbox" checked={form.wander} onChange={(e) => set('wander', e.target.checked)} />
          Pasear libremente por el escritorio
        </label>
        <label className="check">
          <input type="checkbox" checked={form.replyBubbles} onChange={(e) => set('replyBubbles', e.target.checked)} />
          Mostrar el inicio de mis respuestas en una burbuja
        </label>
      </section>

      <section>
        <h3>Atajos de teclado</h3>
        <ul className="shortcuts">
          <li>
            <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>C</kbd> Mostrar u ocultar a Canela
          </li>
          <li>
            <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Espacio</kbd> Abrir o cerrar el asistente
          </li>
        </ul>
      </section>

      <div className="save-bar">
        {saved && <span className="hint ok">{saved}</span>}
        <button className="primary" disabled={!dirty} onClick={() => void save()}>
          Guardar ajustes
        </button>
      </div>
    </div>
  );
}
