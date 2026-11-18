import { useEffect, useRef, useState } from 'react';
import { api, onMood, onSettingsChanged } from '../lib/ipc';
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
            ctrl.mood(p.mood, settings.replyBubbles || p.mood === 'sad' ? p.bubble : undefined);
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

      {layout && activity === 'listen' && (
        <div className="listen" style={{ left: layout.headX - 30, top: layout.headY - 18 }}>
          ♪
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
