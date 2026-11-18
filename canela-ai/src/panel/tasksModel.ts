// Lógica pura de la lista de tareas (agrupar, formatear) para poder probarla.
import type { Task } from '../lib/ipc';

export interface TaskGroups {
  overdue: Task[];
  today: Task[];
  upcoming: Task[];
  noDate: Task[];
}

export function localToday(d = new Date()): string {
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export function groupTasks(tasks: Task[], today: string): TaskGroups {
  const g: TaskGroups = { overdue: [], today: [], upcoming: [], noDate: [] };
  for (const t of tasks) {
    if (t.done) continue;
    const day = t.due?.slice(0, 10);
    if (!day) g.noDate.push(t);
    else if (day < today) g.overdue.push(t);
    else if (day === today) g.today.push(t);
    else g.upcoming.push(t);
  }
  const byPriority = (a: Task, b: Task) => a.priority - b.priority || (a.due ?? '').localeCompare(b.due ?? '');
  g.overdue.sort(byPriority);
  g.today.sort(byPriority);
  g.noDate.sort(byPriority);
  return g;
}

const MONTHS = ['ene', 'feb', 'mar', 'abr', 'may', 'jun', 'jul', 'ago', 'sep', 'oct', 'nov', 'dic'];

/** "Hoy 18:00", "Mañana", "25 nov", "25 nov 2027". */
export function formatDue(due: string, today: string): string {
  const day = due.slice(0, 10);
  const time = due.length > 10 ? ' ' + due.slice(11, 16) : '';
  const t = new Date(today + 'T00:00:00');
  const d = new Date(day + 'T00:00:00');
  const diff = Math.round((d.getTime() - t.getTime()) / 86_400_000);
  if (diff === 0) return 'Hoy' + time;
  if (diff === 1) return 'Mañana' + time;
  if (diff === -1) return 'Ayer' + time;
  const sameYear = d.getFullYear() === t.getFullYear();
  return `${d.getDate()} ${MONTHS[d.getMonth()]}${sameYear ? '' : ' ' + d.getFullYear()}${time}`;
}

export const PRIORITY_LABEL: Record<number, string> = { 1: 'Alta', 2: 'Normal', 3: 'Baja' };

/** Convierte el valor de <input type="datetime-local"> al formato del backend. */
export function fromDateTimeInput(v: string): string {
  return v ? v.replace('T', ' ').slice(0, 16) : '';
}

export function toDateTimeInput(v: string | null): string {
  return v ? v.replace(' ', 'T').slice(0, 16) : '';
}
