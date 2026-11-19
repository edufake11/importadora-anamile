import { useEffect, useState } from "react";
import {
  api,
  formatUsd,
  type KeyStatus,
  type Settings,
  type SpendSummary,
} from "../lib/ipc";

interface Props {
  form: Settings;
  set: <K extends keyof Settings>(k: K, v: Settings[K]) => void;
  keys: KeyStatus;
}

/** Ajustes › Asistente: cuándo proponer modelos externos y límite de gasto. */
export function RouterSettings({ form, set, keys }: Props) {
  const [spend, setSpend] = useState<SpendSummary | null>(null);
  useEffect(() => {
    void api
      .spendSummary()
      .then(setSpend)
      .catch(() => undefined);
  }, []);
  const anyCloudKey = keys.anthropic || keys.openai;
  const isLocal = form.provider === "ollama";

  return (
    <div className="field router">
      {isLocal && (
        <label className="check">
          <input
            type="checkbox"
            checked={form.routerAuto}
            onChange={(e) => set("routerAuto", e.target.checked)}
          />
          Proponer Claude o ChatGPT para tareas difíciles (siempre te pregunto
          antes y te digo cuánto costaría)
        </label>
      )}
      {isLocal && form.routerAuto && !anyCloudKey && (
        <small className="muted">
          Para eso hace falta una clave de Claude o de ChatGPT. Sin clave,
          Canela responde siempre con el modelo local.
        </small>
      )}
      {isLocal && form.routerAuto && keys.anthropic && keys.openai && (
        <label className="field">
          <span>Preferir</span>
          <select
            value={form.cloudPreference}
            onChange={(e) =>
              set(
                "cloudPreference",
                e.target.value as Settings["cloudPreference"],
              )
            }
          >
            <option value="anthropic">Claude</option>
            <option value="openai">ChatGPT</option>
          </select>
        </label>
      )}
      <label className="field">
        <span>Límite de gasto mensual en modelos de pago (US$)</span>
        <input
          type="number"
          min={0}
          step={1}
          value={form.monthlyBudgetUsd}
          onChange={(e) =>
            set("monthlyBudgetUsd", Math.max(0, Number(e.target.value) || 0))
          }
        />
        <small className="muted">
          {spend
            ? `Este mes llevas ${formatUsd(spend.spentUsd)} (aproximado). `
            : ""}
          Al llegar al límite, Canela deja de usar Claude o ChatGPT hasta el mes
          siguiente. 0 = no gastar nada.
        </small>
      </label>
    </div>
  );
}
