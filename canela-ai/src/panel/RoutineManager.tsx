import { useCallback, useEffect, useState } from 'react';
import { data, onDataChanged, routineSteps, type Routine } from '../lib/ipc';

/** Lista de rutinas guardadas: ver sus pasos y borrarlas. */
export function RoutineManager() {
  const [items, setItems] = useState<Routine[]>([]);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setItems(await data.routineList());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void load();
    const un = onDataChanged((k) => k === 'routines' && void load());
    return () => void un.then((f) => f());
  }, [load]);

  return (
    <div className="routine-manager">
      {items.length === 0 ? (
        <p className="muted">
          Aún no hay rutinas. Pídele a Canela algo como: «guarda una rutina llamada <em>empezar el día</em> que abra VS Code y me muestre las
          tareas de hoy».
        </p>
      ) : (
        <ul className="memory-list">
          {items.map((r) => {
            const steps = routineSteps(r);
            return (
              <li key={r.name} className="memory">
                <div>
                  <div className="memory-kind">
                    {steps.length} {steps.length === 1 ? 'paso' : 'pasos'} · di «ejecuta la rutina {r.name}»
                  </div>
                  <div className="memory-text">{r.name}</div>
                  <ol className="routine-steps">
                    {steps.map((s, i) => (
                      <li key={i}>{s.descripcion || s.herramienta}</li>
                    ))}
                  </ol>
                </div>
                <button
                  className="icon-btn"
                  title="Borrar rutina"
                  aria-label={`Borrar rutina ${r.name}`}
                  onClick={() => confirm(`¿Borrar la rutina «${r.name}»?`) && void data.routineDelete(r.name).catch((e) => setError(String(e)))}
                >
                  ✕
                </button>
              </li>
            );
          })}
        </ul>
      )}
      {error && <div className="hint err">{error}</div>}
    </div>
  );
}
