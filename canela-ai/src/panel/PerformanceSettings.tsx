import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useState } from "react";
import { api, fmtGb, type MemStatus, type Settings } from "../lib/ipc";

interface Props {
  form: Settings;
  set: <K extends keyof Settings>(k: K, v: Settings[K]) => void;
}

function Bar({
  used,
  total,
  label,
}: {
  used: number;
  total: number;
  label: string;
}) {
  const pct = total > 0 ? Math.min(100, Math.round((used / total) * 100)) : 0;
  return (
    <div className="mem-bar" title={`${pct} %`}>
      <div className="mem-bar-label">
        <span>{label}</span>
        <span>
          {fmtGb(used)} de {fmtGb(total)} · {pct} %
        </span>
      </div>
      <div className="mem-bar-track">
        <div
          className={`mem-bar-fill ${pct >= 90 ? "high" : ""}`}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  );
}

/** Texto «se descarga en 1:32» a partir de expires_at de Ollama. */
export function expiresIn(
  expiresAt: string | null,
  now = Date.now(),
): string | null {
  if (!expiresAt) return null;
  const ms = Date.parse(expiresAt) - now;
  if (!Number.isFinite(ms) || ms <= 0) return null;
  // Ollama usa una fecha muy lejana cuando el modelo no caduca.
  if (ms > 24 * 3600_000) return "no se descarga solo";
  const s = Math.round(ms / 1000);
  return `se descarga en ${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** Ajustes › Rendimiento y memoria: modos, estado, registro y liberar memoria. */
export function PerformanceSettings({ form, set }: Props) {
  const [st, setSt] = useState<MemStatus | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(
    () =>
      api
        .memoryStatus()
        .then(setSt)
        .catch(() => undefined),
    [],
  );
  useEffect(() => {
    void load();
    const t = window.setInterval(() => void load(), 10_000);
    return () => window.clearInterval(t);
  }, [load]);

  const s = st?.latest;
  const ai = s?.ai;

  async function release() {
    setBusy(true);
    try {
      const r = await api.memoryRelease();
      setMsg(r.message);
      void load();
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function exportReport() {
    const path = await saveDialog({
      defaultPath: `canela-memoria-${new Date().toISOString().slice(0, 10)}.json`,
      filters: [{ name: "Informe JSON", extensions: ["json"] }],
    });
    if (!path) return;
    try {
      setMsg(`Informe guardado en ${await api.memoryExport(path)}`);
    } catch (e) {
      setMsg(String(e));
    }
  }

  return (
    <section>
      <h3>Rendimiento y memoria</h3>
      <div className="segmented" role="tablist">
        <button
          className={form.resourceMode === "balanced" ? "on" : ""}
          onClick={() => set("resourceMode", "balanced")}
        >
          Equilibrado
        </button>
        <button
          className={form.resourceMode === "saver" ? "on" : ""}
          onClick={() => set("resourceMode", "saver")}
        >
          Ahorro de recursos
        </button>
      </div>
      <p className="muted">
        {form.resourceMode === "saver"
          ? "Ahorro: el modelo de IA se descarga a los 30 s sin uso, contexto máximo 8 192 tokens, sin «pensar más», y el modelo de voz se descarga al terminar de escucharte. Responde un poco más lento la primera vez."
          : "Equilibrado: el modelo de IA se descarga tras 2 min sin uso y se vuelve a cargar solo con tu próximo mensaje (al abrir el micrófono empieza a cargarse para ganar tiempo)."}{" "}
        Nunca se descarga mientras Canela responde o usa una herramienta.
      </p>

      {st?.lowRam && (
        <div className="banner">
          ⚠ Queda poca RAM libre. Libera la memoria de IA o cierra programas que
          no uses.
        </div>
      )}

      {s ? (
        <div className="mem-card">
          <Bar
            label="RAM en uso"
            used={s.sys.total - s.sys.available}
            total={s.sys.total}
          />
          <Bar
            label="Memoria comprometida"
            used={s.sys.commitUsed}
            total={s.sys.commitLimit}
          />
          {s.gpu && (
            <Bar
              label={`VRAM · ${s.gpu.name}`}
              used={s.gpu.used}
              total={s.gpu.total}
            />
          )}
          <div className="mem-line">
            <span>Modelo de IA</span>
            <span>
              {ai == null
                ? "Ollama no responde"
                : ai.length === 0
                  ? "ninguno cargado"
                  : ai
                      .map((m) =>
                        [
                          `${m.name} · ${fmtGb(m.size)}`,
                          m.sizeVram ? `${fmtGb(m.sizeVram)} en VRAM` : null,
                          expiresIn(m.expiresAt),
                        ]
                          .filter(Boolean)
                          .join(" · "),
                      )
                      .join("; ")}
            </span>
          </div>
          <div className="mem-line">
            <span>Canela (con sus ventanas)</span>
            <span>
              {fmtGb(s.canelaPrivate)} privada · {fmtGb(s.canelaWorkingSet)} en
              RAM
            </span>
          </div>
          <small className="muted">Medido a las {s.ts.slice(11)}</small>
        </div>
      ) : (
        <p className="muted">Midiendo…</p>
      )}

      <div className="row">
        <button
          className="btn primary"
          disabled={busy}
          onClick={() => void release()}
        >
          Liberar memoria de IA
        </button>
        <button
          className="btn"
          onClick={() => void api.memoryMeasureNow().then(setSt)}
        >
          Medir ahora
        </button>
        <button className="btn" onClick={() => void exportReport()}>
          Exportar informe
        </button>
      </div>
      {msg && <div className="hint">{msg}</div>}

      <details className="advanced">
        <summary>Programas que más memoria usan</summary>
        <table className="mem-table">
          <thead>
            <tr>
              <th>Programa</th>
              <th>PID</th>
              <th>Privada</th>
              <th>En RAM</th>
            </tr>
          </thead>
          <tbody>
            {s?.top.slice(0, 10).map((p) => (
              <tr
                key={p.pid}
                title={
                  p.path ??
                  "Ruta no disponible (Windows no permite consultarla)"
                }
              >
                <td>{p.name}</td>
                <td>{p.pid}</td>
                <td>{fmtGb(p.private)}</td>
                <td>{fmtGb(p.workingSet)}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <small className="muted">
          Pasa el ratón sobre una fila para ver la ruta del programa. Canela
          nunca cierra programas.
        </small>
      </details>

      <details className="advanced">
        <summary>Registro de eventos ({st?.events.length ?? 0})</summary>
        {st && st.events.length === 0 && (
          <p className="muted">
            Sin eventos: nada ha crecido de forma anormal.
          </p>
        )}
        <ul className="mem-events">
          {st?.events.slice(0, 30).map((e, i) => (
            <li key={i}>
              <span className="muted">{e.ts.slice(5)}</span> {e.message}
            </li>
          ))}
        </ul>
        <label className="check">
          <input
            type="checkbox"
            checked={form.memoryLog}
            onChange={(e) => set("memoryLog", e.target.checked)}
          />
          Guardar el registro en archivos (últimos 7 días)
        </label>
        {st && <small className="muted">Carpeta: {st.logDir}</small>}
      </details>

      <details className="advanced">
        <summary>¿Qué significa cada dato?</summary>
        <p className="muted">
          <b>RAM en uso</b>: memoria física ocupada ahora.{" "}
          <b>Memoria comprometida</b>: lo que Windows ha prometido a los
          programas (RAM + archivo de paginación); puede pasar de 16 GB y, en
          Windows, también cuenta reservas de la tarjeta gráfica, por eso sube
          mucho con un modelo de IA cargado aunque esté «en la GPU».{" "}
          <b>Privada</b>: memoria propia de un programa (cuenta para la
          comprometida). <b>En RAM</b>: la parte de ese programa que está ahora
          en la memoria física. <b>VRAM</b>: memoria de la tarjeta gráfica.
        </p>
      </details>
    </section>
  );
}
