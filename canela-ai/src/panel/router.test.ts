import { describe, expect, it } from "vitest";
import { routeAction, type CloudOffer, type RouteDecision } from "../lib/ipc";

const base: RouteDecision = {
  provider: "ollama",
  complexity: "simple",
  reasons: [],
  offer: null,
  blocked: null,
  private: false,
  privacy: { findings: [] },
  ackRequired: [],
  candidates: [],
  spentMonthUsd: 0,
  reservedUsd: 0,
  budgetUsd: 5,
};
const offer: CloudOffer = {
  provider: "anthropic",
  model: "claude-opus-5-5",
  label: "Claude",
  estCostUsd: 0.03,
  estSeconds: 20,
  why: "la tarea parece compleja",
  ack: [],
  ackSummary: "",
};

describe("enrutador en la interfaz", () => {
  it("lo sencillo se envía al modelo local sin preguntar", () => {
    expect(routeAction(base, false, "ollama")).toEqual({ kind: "send" });
  });

  it("lo complejo pregunta antes de usar un modelo de pago", () => {
    const a = routeAction(
      { ...base, complexity: "complex", offer },
      false,
      "ollama",
    );
    expect(a).toEqual({ kind: "ask", offer, localPossible: true });
  });

  it("si lo permitiste en esta conversación, no vuelve a preguntar", () => {
    const a = routeAction(
      { ...base, complexity: "complex", offer },
      true,
      "ollama",
    );
    expect(a).toEqual({ kind: "send", override: "anthropic" });
  });

  it("con datos personales o imágenes SIEMPRE pregunta, aunque lo hayas permitido", () => {
    const withAck = {
      ...offer,
      ack: ["correo"],
      ackSummary: "una dirección de correo (mensaje)",
    };
    const a = routeAction({ ...base, offer: withAck }, true, "ollama");
    expect(a.kind).toBe("ask");
  });

  it("si Ollama no está, sólo ofrece el externo o cancelar", () => {
    const a = routeAction(
      { ...base, blocked: "No encuentro Ollama", offer },
      false,
      "ollama",
    );
    expect(a).toEqual({ kind: "ask", offer, localPossible: false });
  });

  it("bloqueado sin alternativa: explica y no envía", () => {
    const a = routeAction(
      { ...base, blocked: "límite alcanzado" },
      false,
      "ollama",
    );
    expect(a).toEqual({ kind: "blocked", message: "límite alcanzado" });
  });

  it("principal externo + datos personales: confirma este mensaje", () => {
    const d: RouteDecision = {
      ...base,
      provider: "anthropic",
      ackRequired: ["correo"],
      privacy: {
        findings: [
          {
            level: "personal",
            kind: "correo",
            what: "una dirección de correo",
            place: "mensaje",
          },
        ],
      },
    };
    const a = routeAction(d, false, "anthropic");
    expect(a).toEqual({
      kind: "confirm-send",
      provider: "anthropic",
      ack: ["correo"],
      summary: "una dirección de correo (mensaje)",
    });
  });

  it("principal externo + secreto: responde el modelo local", () => {
    const a = routeAction(
      { ...base, provider: "ollama", private: true },
      false,
      "anthropic",
    );
    expect(a).toEqual({ kind: "send", override: "ollama" });
  });
});
