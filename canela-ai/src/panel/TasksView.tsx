import { useCallback, useEffect, useState } from "react";
import {
  askUser,
  data,
  onDataChanged,
  type ActivityItem,
  type Task,
  type TaskInput,
} from "../lib/ipc";
import {
  formatDue,
  fromDateTimeInput,
  groupTasks,
  localToday,
  PRIORITY_LABEL,
  toDateTimeInput,
} from "./tasksModel";

interface Draft {
  title: string;
  due: string;
  remindAt: string;
  priority: number;
}

const emptyDraft: Draft = { title: "", due: "", remindAt: "", priority: 2 };

function TaskFields({
  draft,
  onChange,
}: {
  draft: Draft;
  onChange: (d: Draft) => void;
}) {
  return (
    <div className="task-fields">
      <label>
        <span>Fecha</span>
        <input
          type="date"
          value={draft.due.slice(0, 10)}
          onChange={(e) => onChange({ ...draft, due: e.target.value })}
        />
      </label>
      <label>
        <span>Recordatorio</span>
        <input
          type="datetime-local"
          value={toDateTimeInput(draft.remindAt)}
          onChange={(e) =>
            onChange({ ...draft, remindAt: fromDateTimeInput(e.target.value) })
          }
        />
      </label>
      <label>
        <span>Prioridad</span>
        <select
          value={draft.priority}
          onChange={(e) =>
            onChange({ ...draft, priority: Number(e.target.value) })
          }
        >
          <option value={1}>Alta</option>
          <option value={2}>Normal</option>
          <option value={3}>Baja</option>
        </select>
      </label>
    </div>
  );
}

function TaskRow({
  task,
  today,
  onError,
}: {
  task: Task;
  today: string;
  onError: (e: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<Draft>(emptyDraft);

  const startEdit = () => {
    setDraft({
      title: task.title,
      due: task.due ?? "",
      remindAt: task.remindAt ?? "",
      priority: task.priority,
    });
    setEditing(true);
  };

  async function save() {
    try {
      const input: TaskInput = {
        title: draft.title,
        priority: draft.priority,
        due: draft.due,
        remindAt: draft.remindAt,
      };
      await data.taskUpdate(task.id, input);
      setEditing(false);
    } catch (e) {
      onError(String(e));
    }
  }

  async function remove() {
    if (!(await askUser(`¿Eliminar la tarea «${task.title}»?`))) return;
    await data.taskDelete(task.id).catch((e) => onError(String(e)));
  }

  if (editing) {
    return (
      <li className="task editing">
        <input
          className="task-title-input"
          value={draft.title}
          autoFocus
          onChange={(e) => setDraft({ ...draft, title: e.target.value })}
          onKeyDown={(e) => e.key === "Enter" && void save()}
        />
        <TaskFields draft={draft} onChange={setDraft} />
        <div className="row end">
          <button className="btn" onClick={() => setEditing(false)}>
            Cancelar
          </button>
          <button
            className="btn primary"
            onClick={() => void save()}
            disabled={!draft.title.trim()}
          >
            Guardar
          </button>
        </div>
      </li>
    );
  }

  const meta = [
    task.due ? formatDue(task.due, today) : null,
    task.priority !== 2
      ? `Prioridad ${PRIORITY_LABEL[task.priority].toLowerCase()}`
      : null,
    task.remindAt ? `Aviso ${formatDue(task.remindAt, today)}` : null,
  ].filter(Boolean);

  return (
    <li
      className={`task ${task.done ? "done" : ""} ${task.priority === 1 ? "high" : ""}`}
    >
      <button
        className="check-circle"
        aria-label={
          task.done ? "Marcar como pendiente" : "Marcar como completada"
        }
        onClick={() =>
          void data
            .taskSetDone(task.id, !task.done)
            .catch((e) => onError(String(e)))
        }
      >
        {task.done ? "✓" : ""}
      </button>
      <div className="task-main" onDoubleClick={startEdit}>
        <div className="task-title">{task.title}</div>
        {meta.length > 0 && <div className="task-meta">{meta.join(" · ")}</div>}
      </div>
      {!task.done && (
        <button
          className="icon-btn"
          title="Editar"
          aria-label="Editar"
          onClick={startEdit}
        >
          ✎
        </button>
      )}
      <button
        className="icon-btn"
        title="Eliminar"
        aria-label="Eliminar"
        onClick={() => void remove()}
      >
        ✕
      </button>
    </li>
  );
}

export function TasksView() {
  const [pending, setPending] = useState<Task[]>([]);
  const [done, setDone] = useState<Task[]>([]);
  const [activity, setActivity] = useState<ActivityItem[]>([]);
  const [draft, setDraft] = useState<Draft>(emptyDraft);
  const [more, setMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const today = localToday();

  const load = useCallback(async () => {
    try {
      const [p, d, a] = await Promise.all([
        data.taskList("pending"),
        data.taskList("done"),
        data.activityList(),
      ]);
      setPending(p);
      setDone(d);
      setActivity(a);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    void load();
    const un = onDataChanged(
      (kind) => (kind === "tasks" || kind === "memory") && void load(),
    );
    return () => void un.then((f) => f());
  }, [load]);

  async function add() {
    if (!draft.title.trim()) return;
    setError(null);
    try {
      await data.taskAdd({
        title: draft.title,
        due: draft.due,
        remindAt: draft.remindAt,
        priority: draft.priority,
      });
      setDraft(emptyDraft);
      setMore(false);
    } catch (e) {
      setError(String(e));
    }
  }

  const g = groupTasks(pending, today);
  const sections: Array<[string, Task[]]> = [
    ["Atrasadas", g.overdue],
    ["Hoy", g.today],
    ["Próximas", g.upcoming],
    ["Sin fecha", g.noDate],
  ];

  return (
    <div className="tasks">
      <div className="task-add">
        <div className="row">
          <input
            placeholder="Nueva tarea…"
            value={draft.title}
            onChange={(e) => setDraft({ ...draft, title: e.target.value })}
            onKeyDown={(e) => e.key === "Enter" && void add()}
          />
          <button
            className="btn primary"
            onClick={() => void add()}
            disabled={!draft.title.trim()}
          >
            Añadir
          </button>
        </div>
        <button className="text-button" onClick={() => setMore(!more)}>
          {more ? "Menos opciones" : "Más opciones"}
        </button>
        {more && <TaskFields draft={draft} onChange={setDraft} />}
      </div>

      {error && <div className="notice err slim">{error}</div>}

      <div className="task-list">
        {pending.length === 0 && (
          <p className="empty-hint">
            No tienes tareas pendientes. También puedes pedírselas a Canela en
            el chat.
          </p>
        )}
        {sections.map(([label, items]) =>
          items.length === 0 ? null : (
            <section key={label}>
              <h4>
                {label} <span>{items.length}</span>
              </h4>
              <ul>
                {items.map((t) => (
                  <TaskRow
                    key={t.id}
                    task={t}
                    today={today}
                    onError={setError}
                  />
                ))}
              </ul>
            </section>
          ),
        )}

        {done.length > 0 && (
          <details className="fold">
            <summary>Completadas ({done.length})</summary>
            <ul>
              {done.slice(0, 30).map((t) => (
                <TaskRow key={t.id} task={t} today={today} onError={setError} />
              ))}
            </ul>
          </details>
        )}

        {activity.length > 0 && (
          <details className="fold">
            <summary>Actividad reciente</summary>
            <ul className="activity">
              {activity.slice(0, 40).map((a) => (
                <li key={a.id}>
                  <span>{a.at.slice(5, 16).replace("-", "/")}</span> {a.summary}
                </li>
              ))}
            </ul>
          </details>
        )}
      </div>
    </div>
  );
}
