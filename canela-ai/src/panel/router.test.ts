import { describe, expect, it } from "vitest";
import { routeAction, type RouteDecision } from "../lib/ipc";

const base: RouteDecision = {
  provider: "ollama",
  complexity: "simple",
  reasons: [],
  offer: null,
  blocked: null,
  private: false,
  spentMonthUsd: 0,
  budgetUsd: 5,
};
const offer = {
  provider: "anthropic" as const,
  model: "claude-opus-5-5",
  label: "Claude",
  estCostUsd: 0.03,
  why: "la tarea parece compleja",
};

describe("enrutador en la interfaz", () => {
  it("lo sencillo se envía al modelo local sin preguntar", () => {
    expect(routeAction(base, false)).toEqual({ kind: "send" });
  });

  it("lo complejo pregunta antes de usar un modelo de pago", () => {
    const a = routeAction({ ...base, complexity: "complex", offer }, false);
    expect(a).toEqual({ kind: "ask", offer, localPossible: true });
  });

  it("si lo permitiste en esta conversación, no vuelve a preguntar", () => {
    const a = routeAction({ ...base, complexity: "complex", offer }, true);
    expect(a).toEqual({ kind: "send", override: "anthropic" });
  });

  it("con datos privados siempre pregunta (y el backend lo rechaza)", () => {
    const a = routeAction({ ...base, private: true, offer }, true);
    expect(a.kind).toBe("ask");
  });

  it("si Ollama no está, sólo ofrece el externo o cancelar", () => {
    const a = routeAction({ ...base, blocked: "No encuentro Ollama", offer }, false);
    expect(a).toEqual({ kind: "ask", offer, localPossible: false });
  });

  it("bloqueado sin alternativa: explica y no envía", () => {
    const a = routeAction({ ...base, blocked: "límite alcanzado" }, false);
    expect(a).toEqual({ kind: "blocked", message: "límite alcanzado" });
  });
});
