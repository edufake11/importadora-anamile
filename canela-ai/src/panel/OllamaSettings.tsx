import { useCallback, useEffect, useState } from "react";
import { api, type OllamaStatus, type Settings } from "../lib/ipc";

const CONTEXTS = [
  { v: 8192, label: "8 192 tokens · usa menos memoria de la GPU" },
  { v: 16384, label: "16 384 tokens · recomendado" },
  { v: 32768, label: "32 768 tokens · conversaciones largas (más memoria)" },
];

/** Memoria aproximada de qwen3:8b con este contexto (modelo + caché). */
export function estimateVramGb(numCtx: number): number {
  // ≈ 5,2 GB de pesos + ≈ 144 KB por token de caché (36 capas · 8 cabezas · 128 · K+V · fp16).
  return Math.round((5.2 + (numCtx * 147_456) / 1e9) * 10) / 10;
}

interface Props {
  form: Settings;
  set: <K extends keyof Settings>(k: K, v: Settings[K]) => void;
}

/** Ajustes del cerebro local: estado de Ollama, modelo y rendimiento. */
export function OllamaSettings({ form, set }: Props) {
  const [status, setStatus] = useState<OllamaStatus | null>(null);
  const [busy, setBusy] = useState(false);

  const check = useCallback(async (model?: string) => {
    setBusy(true);
    try {
      setStatus(await api.ollamaStatus(model));
    } catch (e) {
      setStatus(null);
      console.warn(e);
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    void check(form.ollamaModel);
    // Sólo al abrir esta sección; luego con el botón.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const installed = status?.models ?? [];
  const known = installed.some((m) => m.name === form.ollamaModel);
  const ok = status?.running && status.modelInstalled;

  return (
    <div className="ollama">
      <p className="muted">
        Canela usa un modelo instalado en tu computadora con Ollama. No necesita
        clave y <strong>la conversación no sale de este equipo</strong>.
      </p>

      <div className={`ollama-status ${ok ? "ok" : "bad"}`} role="status">
        <span className="dot" aria-hidden />
        <span>
          {busy ? "Comprobando Ollama…" : (status?.message ?? "Sin comprobar")}
        </span>
      </div>
      <button
        className="btn"
        disabled={busy}
        onClick={() => void check(form.ollamaModel)}
      >
        {busy ? "Comprobando…" : "Comprobar Ollama"}
      </button>

      <label className="field">
        <span>Modelo</span>
        {installed.length > 0 ? (
          <select
            value={known ? form.ollamaModel : ""}
            onChange={(e) => {
              set("ollamaModel", e.target.value);
              void check(e.target.value);
            }}
          >
            {!known && (
              <option value="">{form.ollamaModel} (no instalado)</option>
            )}
            {installed.map((m) => (
              <option key={m.name} value={m.name}>
                {m.name}
                {m.parameters ? ` · ${m.parameters}` : ""} ·{" "}
                {m.sizeGb.toLocaleString("es")} GB
              </option>
            ))}
          </select>
        ) : (
          <input
            value={form.ollamaModel}
            onChange={(e) => set("ollamaModel", e.target.value)}
            spellCheck={false}
          />
        )}
        <small>
          Recomendado: <code>qwen3:8b</code> (entiende herramientas). Canela
          nunca descarga modelos: para instalar uno, abre una terminal y escribe{" "}
          <code>ollama pull nombre</code>.
        </small>
      </label>
      {status?.modelInstalled && status.capabilities.length > 0 && (
        <div className="model-info">
          <span className={status.capabilities.includes("tools") ? "" : "no"}>
            Herramientas
          </span>
          <span
            className={status.capabilities.includes("thinking") ? "" : "no"}
          >
            Razonamiento
          </span>
          <span className={status.capabilities.includes("vision") ? "" : "no"}>
            Imágenes
          </span>
        </div>
      )}

      <label className="field">
        <span>Memoria de la conversación (contexto)</span>
        <select
          value={form.ollamaNumCtx}
          onChange={(e) => set("ollamaNumCtx", Number(e.target.value))}
        >
          {CONTEXTS.map((c) => (
            <option key={c.v} value={c.v}>
              {c.label}
            </option>
          ))}
          {!CONTEXTS.some((c) => c.v === form.ollamaNumCtx) && (
            <option value={form.ollamaNumCtx}>
              {form.ollamaNumCtx} tokens
            </option>
          )}
        </select>
        <small>
          Con qwen3:8b ocupa ≈{" "}
          {estimateVramGb(form.ollamaNumCtx).toLocaleString("es")} GB de tu GPU
          (tienes 16 GB). Las instrucciones y herramientas de Canela ya ocupan ≈
          5 200 tokens, por eso el valor de Ollama por defecto (4 096) no
          alcanza.
        </small>
      </label>

      <label className="check">
        <input
          type="checkbox"
          checked={form.ollamaThink}
          onChange={(e) => set("ollamaThink", e.target.checked)}
        />
        Pensar más antes de responder (más lento; mejor en tareas difíciles). El
        razonamiento nunca se muestra.
      </label>
    </div>
  );
}
