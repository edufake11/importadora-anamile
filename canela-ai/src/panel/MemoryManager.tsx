import { useCallback, useEffect, useState } from 'react';
import { data, MEMORY_KINDS, onDataChanged, type Memory } from '../lib/ipc';

const kindLabel = (k: string) => MEMORY_KINDS.find((m) => m.id === k)?.label ?? 'Otro';

function MemoryRow({ m, onError }: { m: Memory; onError: (e: string) => void }) {
  const [editing, setEditing] = useState(false);
  const [kind, setKind] = useState(m.kind);
  const [text, setText] = useState(m.content);

  async function save() {
    try {
      await data.memoryUpdate(m.id, kind, text);
      setEditing(false);
    } catch (e) {
      onError(String(e));
    }
  }

  if (editing) {
    return (
      <li className="memory editing">
        <select value={kind} onChange={(e) => setKind(e.target.value)}>
          {MEMORY_KINDS.map((k) => (
            <option key={k.id} value={k.id}>
              {k.label}
            </option>
          ))}
        </select>
        <textarea value={text} rows={2} onChange={(e) => setText(e.target.value)} />
        <div className="row end">
          <button className="btn" onClick={() => setEditing(false)}>
            Cancelar
          </button>
          <button className="btn primary" disabled={!text.trim()} onClick={() => void save()}>
            Guardar
          </button>
        </div>
      </li>
    );
  }

  return (
    <li className="memory">
      <div>
        <div className="memory-kind">
          {kindLabel(m.kind)}
          {m.source === 'canela' ? ' · guardado por Canela' : ''}
          {m.expiresAt ? ` · temporal hasta ${m.expiresAt.slice(0, 10)}` : ''}
        </div>
        <div className="memory-text">{m.content}</div>
      </div>
      <button
        className="icon-btn"
        title={m.expiresAt ? 'Hacer permanente' : 'Hacer temporal (7 días)'}
        aria-label={m.expiresAt ? 'Hacer permanente' : 'Hacer temporal'}
        onClick={() => void data.memorySetTemporary(m.id, m.expiresAt ? null : 7).catch((e) => onError(String(e)))}
      >
        {m.expiresAt ? '∞' : '⏱'}
      </button>
      <button className="icon-btn" title="Editar" aria-label="Editar" onClick={() => setEditing(true)}>
        ✎
      </button>
      <button
        className="icon-btn"
        title="Olvidar"
        aria-label="Olvidar"
        onClick={() => confirm('¿Olvidar este recuerdo?') && void data.memoryDelete(m.id).catch((e) => onError(String(e)))}
      >
        ✕
      </button>
    </li>
  );
}

/** Ver, buscar, añadir, editar y borrar lo que Canela recuerda. */
export function MemoryManager() {
  const [items, setItems] = useState<Memory[]>([]);
  const [filter, setFilter] = useState('');
  const [query, setQuery] = useState('');
  const [newKind, setNewKind] = useState('dato');
  const [newText, setNewText] = useState('');
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setItems(await data.memoryList(filter, query));
    } catch (e) {
      setError(String(e));
    }
  }, [filter, query]);

  useEffect(() => {
    void load();
    const un = onDataChanged((k) => k === 'memory' && void load());
    return () => void un.then((f) => f());
  }, [load]);

  async function add() {
    try {
      await data.memoryAdd(newKind, newText);
      setNewText('');
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="memory-manager">
      <div className="row">
        <select value={filter} onChange={(e) => setFilter(e.target.value)} aria-label="Filtrar por tipo">
          <option value="">Todo</option>
          {MEMORY_KINDS.map((k) => (
            <option key={k.id} value={k.id}>
              {k.label}
            </option>
          ))}
        </select>
        <input placeholder="Buscar…" value={query} onChange={(e) => setQuery(e.target.value)} />
      </div>

      {items.length === 0 ? (
        <p className="muted">Todavía no hay recuerdos. Dile a Canela: «recuerda que…», o añádelos aquí.</p>
      ) : (
        <ul className="memory-list">
          {items.map((m) => (
            <MemoryRow key={m.id} m={m} onError={setError} />
          ))}
        </ul>
      )}

      <div className="row">
        <select value={newKind} onChange={(e) => setNewKind(e.target.value)} aria-label="Tipo">
          {MEMORY_KINDS.map((k) => (
            <option key={k.id} value={k.id}>
              {k.label}
            </option>
          ))}
        </select>
        <input placeholder="Algo que Canela deba recordar" value={newText} onChange={(e) => setNewText(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && newText.trim() && void add()} />
        <button className="btn" disabled={!newText.trim()} onClick={() => void add()}>
          Añadir
        </button>
      </div>
      {error && <div className="hint err">{error}</div>}
      {items.length > 0 && (
        <button
          className="text-button"
          onClick={() => confirm('¿Borrar TODA la memoria de Canela? No se puede deshacer.') && void data.memoryClear()}
        >
          Borrar toda la memoria
        </button>
      )}
    </div>
  );
}
