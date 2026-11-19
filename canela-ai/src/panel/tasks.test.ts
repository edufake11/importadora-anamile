import { describe, expect, it } from "vitest";
import type { Task } from "../lib/ipc";
import {
  formatDue,
  fromDateTimeInput,
  groupTasks,
  toDateTimeInput,
} from "./tasksModel";

const t = (
  id: number,
  due: string | null,
  priority = 2,
  done = false,
): Task => ({
  id,
  title: `t${id}`,
  notes: "",
  priority,
  due,
  remindAt: null,
  reminded: false,
  done,
  createdAt: "",
  completedAt: null,
});

describe("tareas", () => {
  it("agrupa en atrasadas, hoy, próximas y sin fecha", () => {
    const g = groupTasks(
      [
        t(1, "2026-11-17"),
        t(2, "2026-11-18 09:00"),
        t(3, "2026-12-01"),
        t(4, null),
        t(5, "2026-11-18", 2, true),
      ],
      "2026-11-18",
    );
    expect(g.overdue.map((x) => x.id)).toEqual([1]);
    expect(g.today.map((x) => x.id)).toEqual([2]);
    expect(g.upcoming.map((x) => x.id)).toEqual([3]);
    expect(g.noDate.map((x) => x.id)).toEqual([4]);
  });

  it("ordena por prioridad", () => {
    const g = groupTasks([t(1, null, 3), t(2, null, 1)], "2026-11-18");
    expect(g.noDate.map((x) => x.id)).toEqual([2, 1]);
  });

  it("formatea fechas de forma amigable", () => {
    expect(formatDue("2026-11-18 18:00", "2026-11-18")).toBe("Hoy 18:00");
    expect(formatDue("2026-11-19", "2026-11-18")).toBe("Mañana");
    expect(formatDue("2026-11-25", "2026-11-18")).toBe("25 nov");
    expect(formatDue("2027-01-02", "2026-11-18")).toBe("2 ene 2027");
  });

  it("convierte el formato de los campos de fecha y hora", () => {
    expect(fromDateTimeInput("2026-11-18T09:30")).toBe("2026-11-18 09:30");
    expect(toDateTimeInput("2026-11-18 09:30")).toBe("2026-11-18T09:30");
    expect(fromDateTimeInput("")).toBe("");
  });
});
