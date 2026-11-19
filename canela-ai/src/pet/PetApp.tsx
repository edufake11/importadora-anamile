import { useEffect, useRef, useState } from 'react';
import { api, onMood, onSettingsChanged, onWakeState, voice } from '../lib/ipc';
import type { Activity } from './brain';
import { BUBBLE_AREA, PetController, type PetLayout } from './controller';
import { SpriteSheet } from './sprites';
import './pet.css';

interface Bubble {
  id: number;
  text: string;
}

export function PetApp() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const ctrlRef = useRef<PetController | null>(null);
  const [layout, setLayout] = useState<PetLayout | null>(null);
  const [activity, setActivity] = useState<Activity>('idle');
  const [bubble, setBubble] = useState<Bubble | null>(null);
  const [error, setError] = useState<string | null>(null);
  const bubbleTimer = useRef<number>(undefined);
  // «Hola, Canela» activo: indicador visible (clic = apagar).
  const [wake, setWake] = useState(false);
  useEffect(() => {
    void voice.wakeStatus().then(setWake).catch(() => undefined);
    const un = onWakeState((e) => setWake(e.active));
    return () => void un.then((f) => f());
  }, []);

  useEffect(() => {
    let disposed = false;
    const cleanups: Array<() => void> = [];

    const say = (text: string, ms = 4000) => {
      window.clearTimeout(bubbleTimer.current);
      setBubble({ id: Date.now(), text });
      bubbleTimer.current = window.setTimeout(() => setBubble(null), ms);
    };

    (async () => {
      try {
        const [sheet, settings] = await Promise.all([SpriteSheet.load(), api.getSettings()]);
        if (disposed || !canvasRef.current) return;
        const ctrl = new PetController(canvasRef.current, sheet, settings, {
          onActivity: setActivity,
          onLayout: setLayout,
          say,
        });
        ctrlRef.current = ctrl;
        await ctrl.start();
        cleanups.push(
          await onMood((p) => {
            // Los avisos de micrófono, permisos y errores se muestran siempre.
            const always = p.mood === 'sad' || p.mood === 'listen';
            ctrl.mood(p.mood, settings.replyBubbles || always ? p.bubble : undefined);
            if (p.mood === 'idle' && !p.bubble) setBubble(null);
          }),
          await onSettingsChanged((s) => {
            Object.assign(settings, s);
            void ctrl.updateSettings(s);
          }),
        );
      } catch (e) {
        setError(String(e));
      }
    })();

    return () => {
      disposed = true;
      ctrlRef.current?.dispose();
      cleanups.forEach((c) => c());
      window.clearTimeout(bubbleTimer.current);
    };
  }, []);

  const ctrl = () => ctrlRef.current;
  const canvasLeft = layout ? (layout.width - layout.canvasW) / 2 : 0;

  return (
    <div className="pet-root" style={layout ? { width: layout.width, height: layout.height } : undefined}>
      {error && (
        <div className="pet-error" data-hit>
          No pude cargar a Canela: {error}
        </div>
      )}

      {layout && bubble && (
        <div className="bubble-wrap" style={{ height: BUBBLE_AREA }}>
          <button
            key={bubble.id}
            className="bubble"
            data-hit
            onClick={() => void api.showPanel()}
            title="Abrir el asistente"
          >
            {bubble.text}
          </button>
          <span className="bubble-tail" style={{ left: layout.headX - 7 }} />
        </div>
      )}

      {layout && activity === 'think' && !bubble && (
        <div className="thought" style={{ left: layout.headX - 20, top: layout.headY - 46 }}>
          <span />
          <span />
          <span />
        </div>
      )}

      {layout && activity === 'sleep' && (
        <div className="zzz" style={{ left: layout.headX + (layout.flip ? -30 : 4), top: layout.headY - 30 }}>
          <span>z</span>
          <span>z</span>
          <span>Z</span>
        </div>
      )}

      {layout && (activity === 'oops' || activity === 'curious') && (
        <div className="exclaim" style={{ left: layout.headX + (layout.flip ? -18 : 6), top: layout.headY - 34 }}>
          {activity === 'oops' ? '!' : '?'}
        </div>
      )}

      {layout && wake && activity !== 'listen' && (
        <button
          className="wake-badge"
          data-hit
          style={{ left: layout.headX - 30, top: layout.headY - 14 }}
          title="Escuchando «Hola, Canela» (en este equipo). Clic para apagar."
          aria-label="Apagar «Hola, Canela»"
          onClick={() => void voice.wakeSet(false).then(setWake).catch(() => undefined)}
        >
          <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="#111" strokeWidth="2.6" strokeLinecap="round" aria-hidden>
            <path d="M6 9a6 6 0 0 1 12 0c0 4-4 5-4 9a3 3 0 0 1-6 0" />
          </svg>
        </button>
      )}

      {layout && activity === 'listen' && (
        <div className="listen" style={{ left: layout.headX - 34, top: layout.headY - 20 }} title="Micrófono o atención activa">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="#111" strokeWidth="2.4" strokeLinecap="round" aria-hidden>
            <rect x="9" y="3" width="6" height="11" rx="3" />
            <path d="M5 11a7 7 0 0 0 14 0M12 18v3" />
          </svg>
        </div>
      )}

      <canvas
        ref={canvasRef}
        className="pet-canvas"
        style={{ left: canvasLeft }}
        onPointerDown={(e) => ctrl()?.onPointerDown(e.nativeEvent)}
        onPointerMove={(e) => ctrl()?.onPointerMove(e.nativeEvent)}
        onPointerUp={(e) => ctrl()?.onPointerUp(e.nativeEvent)}
        onDoubleClick={() => void api.showPanel()}
        onContextMenu={(e) => {
          e.preventDefault();
          void ctrl()?.openContextMenu();
        }}
      />
    </div>
  );
}
