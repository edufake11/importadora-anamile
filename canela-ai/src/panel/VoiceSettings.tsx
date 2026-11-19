import { useCallback, useEffect, useState } from "react";
import {
  askUser,
  api,
  fmtMb,
  onVoiceDownload,
  onWakeState,
  voice,
  type DownloadEvent,
  type KeyStatus,
  type LocalAsset,
  type LocalVoiceStatus,
  type Settings,
} from "../lib/ipc";
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

/** Un modelo descargable: tamaño, licencia, descargar / usar / borrar. */
function AssetRow({
  a,
  selected,
  onSelect,
  progress,
  busy,
  onDownload,
  onDelete,
}: {
  a: LocalAsset;
  selected?: boolean;
  onSelect?: () => void;
  progress: DownloadEvent | null;
  busy: boolean;
  onDownload: () => void;
  onDelete: () => void;
}) {
  const pct =
    progress?.done != null && progress.total
      ? Math.round((progress.done / progress.total) * 100)
      : null;
  return (
    <li className={`asset ${selected ? "on" : ""}`}>
      <div className="asset-main">
        {onSelect && a.installed ? (
          <label className="check">
            <input type="radio" checked={!!selected} onChange={onSelect} />
            {a.label}
          </label>
        ) : (
          <span>{a.label}</span>
        )}
        <small className="muted">
          {fmtMb(a.size)} · licencia {a.license}. {a.note}
        </small>
        {pct != null && (
          <div className="mem-bar-track">
            <div className="mem-bar-fill" style={{ width: `${pct}%` }} />
          </div>
        )}
      </div>
      {!a.supported ? (
        <small className="muted">Sólo Windows</small>
      ) : a.installed ? (
        <button
          className="icon-btn"
          title="Borrar"
          aria-label={`Borrar ${a.label}`}
          onClick={onDelete}
        >
          ✕
        </button>
      ) : pct != null ? (
        <button className="btn" onClick={() => void voice.localCancel()}>
          Cancelar {pct} %
        </button>
      ) : (
        <button className="btn" disabled={busy} onClick={onDownload}>
          Descargar ({fmtMb(a.size)})
        </button>
      )}
    </li>
  );
}

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
  const [local, setLocal] = useState<LocalVoiceStatus | null>(null);
  const [dl, setDl] = useState<DownloadEvent | null>(null);
  const [dlMsg, setDlMsg] = useState<string | null>(null);
  const loadLocal = useCallback(
    () =>
      voice
        .localStatus()
        .then(setLocal)
        .catch(() => undefined),
    [],
  );
  useEffect(() => {
    void loadLocal();
    const un = onVoiceDownload((e) => {
      if (e.finished) {
        setDl(null);
        setDlMsg(e.error ? e.error : "Descarga completa y verificada.");
        void loadLocal();
      } else setDl(e);
    });
    return () => void un.then((f) => f());
  }, [loadLocal]);

  async function download(a: LocalAsset) {
    // Consentimiento explícito: se muestra el tamaño antes de descargar.
    if (
      !(await askUser(
        `¿Descargar «${a.label}» (${fmtMb(a.size)}) desde ${a.kind === "engine" ? "GitHub" : "Hugging Face"}? Se comprobará con SHA-256 y quedará en este equipo.`,
      ))
    )
      return;
    setDlMsg(null);
    setDl({ id: a.id, done: 0, total: a.size });
    void voice.localDownload(a.id).catch((e) => {
      setDl(null);
      setDlMsg(String(e));
    });
  }

  async function remove(a: LocalAsset) {
    if (
      !(await askUser(
        `¿Borrar «${a.label}» de este equipo? Podrás descargarlo de nuevo.`,
      ))
    )
      return;
    void voice
      .localDelete(a.id)
      .then(loadLocal)
      .catch((e) => setDlMsg(String(e)));
  }

  const [wakeOn, setWakeOn] = useState(false);
  const [wakeErr, setWakeErr] = useState<string | null>(null);
  useEffect(() => {
    void voice
      .wakeStatus()
      .then(setWakeOn)
      .catch(() => undefined);
    const un = onWakeState((e) => {
      setWakeOn(e.active);
      setWakeErr(e.error ?? null);
    });
    return () => void un.then((f) => f());
  }, []);

  async function toggleWake(on: boolean) {
    if (
      on &&
      !(await askUser(
        "¿Activar «Hola, Canela»? El micrófono quedará abierto y Canela analizará el sonido EN ESTE EQUIPO sólo para reconocer la frase. Nada se guarda ni se envía. Puedes apagarlo cuando quieras con la orejita junto a Canela.",
      ))
    )
      return;
    try {
      setWakeErr(null);
      setWakeOn(await voice.wakeSet(on));
    } catch (e) {
      setWakeErr(String(e));
    }
  }

  const assets = local?.assets ?? [];
  const tinyReady = assets.some((a) => a.id === "stt-tiny" && a.installed);
  const sttAssets = assets.filter(
    (a) => a.kind === "stt" && a.id !== "stt-tiny",
  );
  const voiceAssets = assets.filter((a) => a.kind === "voice");
  const engine = assets.find((a) => a.kind === "engine");
  const row = (a: LocalAsset, selected?: boolean, onSelect?: () => void) => (
    <AssetRow
      key={a.id}
      a={a}
      selected={selected}
      onSelect={onSelect}
      progress={dl?.id === a.id ? dl : null}
      busy={!!dl}
      onDownload={() => void download(a)}
      onDelete={() => void remove(a)}
    />
  );

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
    if (form.ttsEngine === "piper") {
      // Guarda primero (la voz elegida se lee de los ajustes guardados).
      void api
        .saveSettings(form)
        .then(onSaved)
        .then(() => voice.cloudStop())
        .then(() => voice.localSpeak(text))
        .catch((e) => setDlMsg(String(e)));
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
            Canela sólo abre el micrófono cuando pulsas 🎙 o <kbd>Ctrl</kbd>{" "}
            <kbd>Alt</kbd> <kbd>V</kbd>, y lo cierra al terminar la frase. Con
            el reconocimiento <b>en este equipo</b> (recomendado) tu voz no sale
            de la computadora y no cuesta nada. Si eliges OpenAI, el audio se
            envía a OpenAI (≈ 0,0045 USD por minuto). El audio nunca se guarda.
          </p>
          <button className="btn primary" onClick={() => void enable()}>
            Activar la voz
          </button>
        </div>
      ) : (
        <>
          <div className="field">
            <span>Entender tu voz</span>
            <div className="segmented">
              <button
                className={form.sttEngine === "local" ? "on" : ""}
                onClick={() => set("sttEngine", "local")}
              >
                En este equipo (gratis)
              </button>
              <button
                className={form.sttEngine === "openai" ? "on" : ""}
                onClick={() => set("sttEngine", "openai")}
              >
                OpenAI (de pago)
              </button>
            </div>
          </div>
          {form.sttEngine === "local" && (
            <div className="field">
              {local && !local.sttAvailable && (
                <p className="muted">
                  Esta compilación no incluye el reconocimiento local.
                </p>
              )}
              <ul className="asset-list">
                {sttAssets.map((a) =>
                  row(a, form.sttLocalModel === a.id, () =>
                    set("sttLocalModel", a.id),
                  ),
                )}
              </ul>
              <small className="muted">
                Canela nunca descarga modelos sin que pulses «Descargar». El
                modelo se carga al usar el micrófono y se libera solo cuando no
                lo usas (
                {form.resourceMode === "saver"
                  ? "al terminar de escucharte"
                  : "tras 5 min"}
                ).
              </small>
            </div>
          )}
          {form.sttEngine === "openai" && form.provider === "ollama" && (
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
          {form.sttEngine === "openai" &&
            !keys.openai &&
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
                className={form.ttsEngine === "piper" ? "on" : ""}
                onClick={() => set("ttsEngine", "piper")}
              >
                Piper (natural, sin internet)
              </button>
              <button
                className={form.ttsEngine === "openai" ? "on" : ""}
                onClick={() => set("ttsEngine", "openai")}
              >
                OpenAI (más natural)
              </button>
            </div>
          </div>
          {form.ttsEngine === "piper" && (
            <div className="field">
              <ul className="asset-list">
                {engine && row(engine)}
                {voiceAssets.map((a) =>
                  row(a, form.piperVoice === a.id, () =>
                    set("piperVoice", a.id),
                  ),
                )}
              </ul>
              <small className="muted">
                Piper genera la voz en este equipo, frase por frase, y no deja
                memoria ocupada al terminar. Si falta algo, Canela usa la voz de
                Windows.
              </small>
            </div>
          )}
          {form.ttsEngine === "piper" ? null : form.ttsEngine === "local" ? (
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
          {dlMsg && <div className="hint">{dlMsg}</div>}
          <div className="field wake">
            <span>«Hola, Canela» (activación por voz)</span>
            <small className="muted">
              Canela escucha el micrófono continuamente <b>en este equipo</b>{" "}
              sólo para reconocer esa frase; el audio se analiza y se descarta
              al instante, nunca se guarda ni se envía. Mientras esté activa
              verás una orejita junto a Canela (clic para apagarla). Se pausa
              mientras te escucha o te habla.
            </small>
            <ul className="asset-list">
              {assets.filter((a) => a.id === "stt-tiny").map((a) => row(a))}
            </ul>
            <label className="check">
              <input
                type="checkbox"
                checked={wakeOn}
                disabled={!tinyReady && !wakeOn}
                onChange={(e) => void toggleWake(e.target.checked)}
              />
              {wakeOn
                ? "Activada: escuchando «Hola, Canela»"
                : "Activar «Hola, Canela»"}
            </label>
            {!tinyReady && (
              <small className="muted">
                Primero descarga el detector (32 MB).
              </small>
            )}
            {wakeErr && <div className="hint err">{wakeErr}</div>}
          </div>
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
