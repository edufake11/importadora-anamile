import { useEffect, useRef, useState } from 'react';
import { screen, type CaptureTarget, type ImageData, type ScreenSources } from '../lib/ipc';

interface Props {
  onAttach: (img: ImageData) => void;
  onClose: () => void;
}

interface Pending {
  captureId: string;
  preview: string;
  width: number;
  height: number;
}

type Sel = { x0: number; y0: number; x1: number; y1: number } | null;

/** Elegir qué capturar → vista previa → (opcional) recortar región → adjuntar. */
export function CaptureSheet({ onAttach, onClose }: Props) {
  const [sources, setSources] = useState<ScreenSources | null>(null);
  const [showWindows, setShowWindows] = useState(false);
  const [pending, setPending] = useState<Pending | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sel, setSel] = useState<Sel>(null);
  const dragging = useRef(false);
  const imgRef = useRef<HTMLImageElement>(null);

  useEffect(() => {
    void screen.sources().then(setSources).catch((e) => setError(String(e)));
  }, []);

  async function take(target: CaptureTarget) {
    setBusy(true);
    setError(null);
    try {
      setPending(await screen.capture(target));
      setSel(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  function cancel() {
    if (pending) void screen.discard(pending.captureId);
    onClose();
  }

  async function attach() {
    if (!pending) return;
    let crop: { x: number; y: number; w: number; h: number } | null = null;
    const el = imgRef.current;
    if (sel && el) {
      const k = pending.width / el.clientWidth;
      const x = Math.min(sel.x0, sel.x1);
      const y = Math.min(sel.y0, sel.y1);
      const w = Math.abs(sel.x1 - sel.x0);
      const h = Math.abs(sel.y1 - sel.y0);
      if (w > 6 && h > 6) crop = { x: Math.round(x * k), y: Math.round(y * k), w: Math.round(w * k), h: Math.round(h * k) };
    }
    try {
      onAttach(await screen.finalize(pending.captureId, crop));
      onClose();
    } catch (e) {
      setError(String(e));
    }
  }

  const point = (e: React.PointerEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return { x: Math.max(0, Math.min(r.width, e.clientX - r.left)), y: Math.max(0, Math.min(r.height, e.clientY - r.top)) };
  };

  return (
    <div className="sheet" role="dialog" aria-label="Compartir captura">
      <div className="sheet-card">
        {!pending ? (
          <>
            <div className="sheet-title">Compartir captura</div>
            <p className="muted">Canela y este panel se ocultan un instante. Verás la imagen antes de enviarla.</p>
            {busy && <p className="muted">Capturando…</p>}
            {sources?.monitors.map((m, i) => (
              <button key={m.id} className="option" disabled={busy} onClick={() => void take({ kind: 'monitor', id: m.id })}>
                Pantalla completa{sources.monitors.length > 1 ? ` ${i + 1}` : ''} <small>{m.primary ? 'principal · ' : ''}{m.width}×{m.height}</small>
              </button>
            ))}
            <button className="option" disabled={busy} onClick={() => setShowWindows(!showWindows)}>
              Una ventana… <small>{sources ? `${sources.windows.length} abiertas` : ''}</small>
            </button>
            {showWindows && (
              <div className="window-list">
                {sources?.windows.length === 0 && <p className="muted">No hay ventanas disponibles.</p>}
                {sources?.windows.map((w) => (
                  <button key={w.id} className="option small" disabled={busy} onClick={() => void take({ kind: 'window', id: w.id })} title={w.title}>
                    {w.title}
                  </button>
                ))}
              </div>
            )}
            <p className="muted small-print">Las ventanas de contraseñas o bancos no aparecen en esta lista.</p>
            {error && <div className="hint err">{error}</div>}
            <div className="row end">
              <button className="btn" onClick={cancel}>
                Cancelar
              </button>
            </div>
          </>
        ) : (
          <>
            <div className="sheet-title">Revisa la captura</div>
            <p className="muted">Arrastra sobre la imagen para enviar sólo una región (opcional). Comprueba que no haya nada privado.</p>
            <div
              className="capture-preview"
              onPointerDown={(e) => {
                dragging.current = true;
                (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
                const p = point(e);
                setSel({ x0: p.x, y0: p.y, x1: p.x, y1: p.y });
              }}
              onPointerMove={(e) => {
                if (!dragging.current) return;
                const p = point(e);
                setSel((s) => (s ? { ...s, x1: p.x, y1: p.y } : s));
              }}
              onPointerUp={() => (dragging.current = false)}
            >
              <img ref={imgRef} src={pending.preview} alt="Vista previa de la captura" draggable={false} />
              {sel && Math.abs(sel.x1 - sel.x0) > 2 && (
                <div
                  className="region"
                  style={{ left: Math.min(sel.x0, sel.x1), top: Math.min(sel.y0, sel.y1), width: Math.abs(sel.x1 - sel.x0), height: Math.abs(sel.y1 - sel.y0) }}
                />
              )}
            </div>
            {error && <div className="hint err">{error}</div>}
            <div className="row end">
              {sel && (
                <button className="btn" onClick={() => setSel(null)}>
                  Usar toda
                </button>
              )}
              <button className="btn" onClick={cancel}>
                Cancelar
              </button>
              <button className="btn primary" onClick={() => void attach()}>
                Adjuntar
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
