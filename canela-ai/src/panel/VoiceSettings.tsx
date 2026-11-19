import { useEffect, useState } from "react";
import { api, voice, type KeyStatus, type Settings } from "../lib/ipc";
import { spanishVoices } from "./speech";

const CLOUD_VOICES = [
  "coral",
  "nova",
  "shimmer",
  "sage",
  "alloy",
  "ash",
  "ballad",
  "echo",
  "fable",
  "onyx",
  "verse",
];

interface Props {
  form: Settings;
  set: <K extends keyof Settings>(k: K, v: Settings[K]) => void;
  keys: KeyStatus;
  onSaved: (s: Settings) => void;
}

export function VoiceSettings({ form, set, keys, onSaved }: Props) {
  const [devices, setDevices] = useState<{
    inputs: string[];
    outputs: string[];
  }>({ inputs: [], outputs: [] });
  const [voices, setVoices] = useState<SpeechSynthesisVoice[]>([]);

  useEffect(() => {
    void voice
      .devices()
      .then(setDevices)
      .catch(() => undefined);
    if (typeof speechSynthesis === "undefined") return;
    const load = () => setVoices(spanishVoices(speechSynthesis.getVoices()));
    load();
    speechSynthesis.addEventListener("voiceschanged", load);
    return () => speechSynthesis.removeEventListener("voiceschanged", load);
  }, []);

  async function enable() {
    const s = await api.saveSettings({ ...form, voiceConsent: true });
    onSaved(s);
  }

  function test() {
    const text = "¡Hola! Soy Canela. Así sueno cuando te hablo.";
    if (form.ttsEngine === "openai") {
      void voice.cloudStop().then(() => voice.cloudSpeak(text));
      return;
    }
    if (typeof speechSynthesis === "undefined") return;
    speechSynthesis.cancel();
    const u = new SpeechSynthesisUtterance(text);
    const v = voices.find((x) => x.name === form.ttsVoice) ?? voices[0];
    if (v) u.voice = v;
    u.lang = v?.lang ?? "es-MX";
    u.rate = form.ttsRate;
    u.volume = form.ttsVolume;
    speechSynthesis.speak(u);
  }

  return (
    <section>
      <h3>Voz</h3>
      {!form.voiceConsent ? (
        <div className="consent">
          <p>
            Para entender lo que dices, Canela envía tu voz a OpenAI <b>sólo</b>{" "}
            mientras el micrófono está activo (cuando pulsas el micrófono o{" "}
            <kbd>Ctrl</kbd> <kbd>Alt</kbd> <kbd>V</kbd>). El audio no se guarda
            en tu equipo. Cuesta aprox. 0,0045 USD por minuto y necesita una
            clave de OpenAI.
          </p>
          <button className="btn primary" onClick={() => void enable()}>
            Activar la voz
          </button>
        </div>
      ) : (
        <>
          {form.provider === "ollama" && (
            <div className="field">
              <p className="muted">
                Con Ollama tu conversación no sale de este equipo. Entender tu
                voz y la voz de OpenAI necesitan enviar audio o texto a OpenAI
                por internet, así que están <b>bloqueadas</b> salvo que lo
                permitas. La voz de Windows sí funciona sin internet.
              </p>
              <label className="check">
                <input
                  type="checkbox"
                  checked={form.ollamaCloudVoice}
                  onChange={(e) => set("ollamaCloudVoice", e.target.checked)}
                />
                Permitir la voz de OpenAI aunque use Ollama (envía el audio de
                lo que dictes a OpenAI)
              </label>
            </div>
          )}
          {!keys.openai &&
            (form.provider !== "ollama" || form.ollamaCloudVoice) && (
              <p className="muted">
                ⚠ Falta la clave de OpenAI para entender tu voz (puedes seguir
                usando Claude u Ollama para el chat).
              </p>
            )}
          <label className="field">
            <span>Micrófono</span>
            <select
              value={form.voiceInputDevice ?? ""}
              onChange={(e) => set("voiceInputDevice", e.target.value || null)}
            >
              <option value="">Predeterminado de Windows</option>
              {devices.inputs.map((d) => (
                <option key={d} value={d}>
                  {d}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>Responder en voz alta</span>
            <select
              value={form.speakReplies}
              onChange={(e) =>
                set("speakReplies", e.target.value as Settings["speakReplies"])
              }
            >
              <option value="voice">Sólo cuando le hablo</option>
              <option value="always">Siempre</option>
              <option value="never">Nunca</option>
            </select>
          </label>
          <div className="field">
            <span>Voz de Canela</span>
            <div className="segmented">
              <button
                className={form.ttsEngine === "local" ? "on" : ""}
                onClick={() => set("ttsEngine", "local")}
              >
                Windows (gratis)
              </button>
              <button
                className={form.ttsEngine === "openai" ? "on" : ""}
                onClick={() => set("ttsEngine", "openai")}
              >
                OpenAI (más natural)
              </button>
            </div>
          </div>
          {form.ttsEngine === "local" ? (
            <label className="field">
              <span>Voz</span>
              <select
                value={form.ttsVoice}
                onChange={(e) => set("ttsVoice", e.target.value)}
              >
                {voices.length === 0 && (
                  <option value="">No hay voces en español instaladas</option>
                )}
                {voices.map((v) => (
                  <option key={v.name} value={v.name}>
                    {v.name}
                  </option>
                ))}
              </select>
              <small>
                Usa el altavoz predeterminado de Windows. Funciona sin internet.
              </small>
            </label>
          ) : (
            <>
              <label className="field">
                <span>Voz</span>
                <select
                  value={form.ttsCloudVoice}
                  onChange={(e) => set("ttsCloudVoice", e.target.value)}
                >
                  {CLOUD_VOICES.map((v) => (
                    <option key={v} value={v}>
                      {v}
                    </option>
                  ))}
                </select>
              </label>
              <label className="field">
                <span>Altavoz</span>
                <select
                  value={form.voiceOutputDevice ?? ""}
                  onChange={(e) =>
                    set("voiceOutputDevice", e.target.value || null)
                  }
                >
                  <option value="">Predeterminado de Windows</option>
                  {devices.outputs.map((d) => (
                    <option key={d} value={d}>
                      {d}
                    </option>
                  ))}
                </select>
                <small>
                  Requiere internet y la clave de OpenAI. Cuesta aprox. 0,015
                  USD por minuto de voz.
                </small>
              </label>
            </>
          )}
          <label className="field">
            <span>Velocidad · {form.ttsRate.toFixed(1)}x</span>
            <input
              type="range"
              min={0.5}
              max={2}
              step={0.1}
              value={form.ttsRate}
              onChange={(e) => set("ttsRate", Number(e.target.value))}
            />
          </label>
          <label className="field">
            <span>Volumen · {Math.round(form.ttsVolume * 100)} %</span>
            <input
              type="range"
              min={0}
              max={1}
              step={0.05}
              value={form.ttsVolume}
              onChange={(e) => set("ttsVolume", Number(e.target.value))}
            />
          </label>
          <button className="btn" onClick={test}>
            Probar voz
          </button>
          <label className="check disabled">
            <input type="checkbox" checked={false} disabled />
            «Hola, Canela» (próximamente)
          </label>
          <small className="muted">
            Escuchar siempre esa frase necesita un modelo local que pediré
            permiso para descargar. Mientras tanto, usa el micrófono o{" "}
            <kbd>Ctrl</kbd> <kbd>Alt</kbd> <kbd>V</kbd>.
          </small>
          <details className="advanced">
            <summary>Opciones avanzadas de voz</summary>
            <label className="field">
              <span>Modelo de transcripción (OpenAI)</span>
              <input
                value={form.sttModel}
                onChange={(e) => set("sttModel", e.target.value)}
                spellCheck={false}
              />
            </label>
            <label className="field">
              <span>Modelo de voz (OpenAI)</span>
              <input
                value={form.ttsCloudModel}
                onChange={(e) => set("ttsCloudModel", e.target.value)}
                spellCheck={false}
              />
            </label>
            <button
              className="text-button"
              onClick={() =>
                void api
                  .saveSettings({ ...form, voiceConsent: false })
                  .then(onSaved)
              }
            >
              Desactivar la voz
            </button>
          </details>
        </>
      )}
    </section>
  );
}
