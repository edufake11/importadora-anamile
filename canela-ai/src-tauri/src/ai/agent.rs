//! Bucle del agente: pide una respuesta al modelo, ejecuta las herramientas
//! que solicite (con permiso del usuario cuando corresponde) y repite hasta
//! que el modelo termine, con un límite de pasos.

use super::{model_of, native_history, tool_results, ChatMessage, StreamEvent, ToolCall, ToolResult};
use crate::agents::ActionRecord;
use crate::settings::Settings;
use crate::tools::{self, Permission, ToolCtx};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

pub const MAX_STEPS: usize = 12;
/// Tiempo máximo por paso de una automatización y para el plan completo.
pub const PLAN_STEP_TIMEOUT: Duration = Duration::from_secs(120);
pub const PLAN_TOTAL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Si no respondes a una confirmación en este tiempo, se cancela.
pub const CONFIRM_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Confirmaciones pendientes (la interfaz responde con `confirm_action`).
#[derive(Default)]
pub struct Confirmations {
    pending: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

impl Confirmations {
    pub fn ask(&self, id: &str) -> oneshot::Receiver<bool> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id.to_string(), tx);
        rx
    }

    pub fn resolve(&self, id: &str, approved: bool) -> bool {
        match self.pending.lock().unwrap().remove(id) {
            Some(tx) => tx.send(approved).is_ok(),
            None => false,
        }
    }
}

#[derive(Clone)]
pub struct AgentEnv {
    pub http: reqwest::Client,
    pub settings: Settings,
    pub api_key: String,
    pub system: String,
    pub history: Vec<ChatMessage>,
    pub tools: Option<Arc<ToolCtx>>,
    pub confirms: Arc<Confirmations>,
    /// Freno de gasto: máximo (US$) que puede costar esta respuesta, con
    /// tokens reales. `None` = sin límite (modelo local).
    pub max_cost_usd: Option<f64>,
    /// v0.8: lo que hay que conseguir (para comprobar el resultado). None =
    /// el último mensaje del usuario.
    pub goal: Option<String>,
    /// v0.7: agente especializado (None = general, todas las herramientas).
    pub agent: Option<&'static crate::agents::Profile>,
}

pub type Emit = Arc<dyn Fn(StreamEvent) + Send + Sync>;

/// Ejecuta una llamada. `tainted` = ya se leyó contenido externo en esta respuesta.
/// Devuelve el resultado y si trae contenido externo.
async fn execute(env: &AgentEnv, ctx: &ToolCtx, call: &ToolCall, emit: &Emit, tainted: bool) -> (ToolResult, bool, ActionRecord) {
    let record = |status: &str, label: &str, verified: Option<bool>, detail: Option<String>| ActionRecord {
        tool: call.name.clone(),
        label: label.to_string(),
        status: status.to_string(),
        verified,
        detail,
    };
    // Si falla antes de empezar, el registro dice al menos sobre qué era.
    let target = call
        .input
        .as_ref()
        .ok()
        .and_then(|v| ["ruta", "titulo", "consulta", "url", "nombre", "id"].iter().find_map(|k| v.get(*k)))
        .map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string()));
    let fail_label = match &target {
        Some(t) => format!("{} «{}»", call.name, t.chars().take(80).collect::<String>()),
        None => call.name.clone(),
    };
    let fail = |text: String, status: &str| {
        (
            ToolResult {
                id: call.id.clone(),
                name: call.name.clone(),
                text: text.clone(),
                is_error: true,
                images: vec![],
            },
            false,
            record(status, &fail_label, None, Some(text)),
        )
    };
    let input = match &call.input {
        Ok(v) => v.clone(),
        Err(e) => return fail(e.clone(), "error"),
    };
    // v0.7: cada agente sólo puede usar sus herramientas (control en el backend).
    if let Some(agent) = env.agent.filter(|a| !a.allows(&call.name)) {
        let msg = format!(
            "La {} no tiene permiso para usar «{}». No se ejecutó. Dile al usuario qué haría falta.",
            agent.name, call.name
        );
        emit(StreamEvent::ToolEnd {
            call_id: call.id.clone(),
            ok: false,
            summary: format!("No permitido para la {}", agent.name),
        });
        return fail(msg, "no permitida");
    }
    let mut prepared = match tools::prepare_async(ctx, &call.name, &input).await {
        Ok(p) => p,
        Err(e) => {
            emit(StreamEvent::ToolEnd {
                call_id: call.id.clone(),
                ok: false,
                summary: e.clone(),
            });
            return fail(e, "error");
        }
    };
    emit(StreamEvent::ToolStart {
        call_id: call.id.clone(),
        name: call.name.clone(),
        label: prepared.label.clone(),
    });
    let label = prepared.label.clone();
    // Defensa contra instrucciones escondidas en archivos, documentos o capturas:
    // si ya se leyó contenido externo, toda acción que cambie algo se confirma.
    if tainted && matches!(prepared.permission, Permission::Auto) && !tools::is_read_only(&call.name) {
        prepared.permission = Permission::Confirm {
            title: label.clone(),
            details: "Canela lo pidió después de leer un archivo, documento o captura. Confírmalo sólo si tú se lo pediste.".into(),
            diff: None,
            image: None,
        };
    }
    if let Permission::Confirm {
        title,
        details,
        diff,
        image,
    } = &prepared.permission
    {
        let rx = env.confirms.ask(&call.id);
        let details = if tainted && !details.contains("después de leer") {
            format!("⚠ Propuesto después de leer contenido externo; revísalo bien.\n{details}")
        } else {
            details.clone()
        };
        emit(StreamEvent::ConfirmRequest {
            confirm_id: call.id.clone(),
            title: title.clone(),
            details,
            diff: diff.clone(),
            image: image.clone(),
        });
        let approved = matches!(tokio::time::timeout(CONFIRM_TIMEOUT, rx).await, Ok(Ok(true)));
        if !approved {
            ctx.db.log("permiso", &format!("Rechazado: {label}"));
            emit(StreamEvent::ToolEnd {
                call_id: call.id.clone(),
                ok: false,
                summary: "No autorizado".into(),
            });
            return fail(
                "El usuario NO autorizó esta acción; no se realizó. No la repitas salvo que te lo pida.".into(),
                "denegada",
            );
        }
    }
    if let Some((title, steps)) = prepared.take_plan() {
        // Un plan o rutina tampoco puede usar herramientas ajenas al agente.
        if let Some(agent) = env.agent {
            if let Some(bad) = steps.iter().find(|st| !agent.allows(&st.herramienta)) {
                let msg = format!(
                    "El plan usa «{}», que la {} no tiene permitido. No se ejecutó.",
                    bad.herramienta, agent.name
                );
                emit(StreamEvent::ToolEnd {
                    call_id: call.id.clone(),
                    ok: false,
                    summary: msg.clone(),
                });
                return fail(msg, "no permitida");
            }
        }
        let (text, untrusted) = run_plan(env, ctx, &call.id, &title, &steps, emit).await;
        emit(StreamEvent::ToolEnd {
            call_id: call.id.clone(),
            ok: true,
            summary: label.clone(),
        });
        let first_line = text.lines().next().unwrap_or("").to_string();
        return (
            ToolResult {
                id: call.id.clone(),
                name: call.name.clone(),
                text,
                is_error: false,
                images: vec![],
            },
            untrusted,
            record("ok", &label, None, Some(first_line)),
        );
    }
    match tools::run(ctx, prepared).await {
        Ok(out) => {
            emit(StreamEvent::ToolEnd {
                call_id: call.id.clone(),
                ok: true,
                summary: label.clone(),
            });
            // v0.7: comprobación objetiva del efecto (si es comprobable).
            let check = crate::agents::verify(ctx, &call.name, &input, &out.text);
            if let Some(c) = &check {
                emit(StreamEvent::ToolVerified {
                    call_id: call.id.clone(),
                    ok: c.ok,
                    detail: c.detail.clone(),
                });
            }
            let rec = record("ok", &label, check.as_ref().map(|c| c.ok), check.map(|c| c.detail));
            (
                ToolResult {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    text: out.text,
                    is_error: false,
                    images: out.images,
                },
                out.untrusted,
                rec,
            )
        }
        Err(e) => {
            emit(StreamEvent::ToolEnd {
                call_id: call.id.clone(),
                ok: false,
                summary: e.clone(),
            });
            fail(e, "error")
        }
    }
}

/// Ejecuta los pasos de un plan ya aprobado, en orden. Se detiene en el
/// primer error, si se rechaza un paso delicado o si se agota el tiempo.
async fn run_plan(env: &AgentEnv, ctx: &ToolCtx, call_id: &str, title: &str, steps: &[tools::PlanStep], emit: &Emit) -> (String, bool) {
    let started = std::time::Instant::now();
    let total = steps.len();
    let mut report = Vec::new();
    let mut done = 0;
    let mut untrusted = false;
    ctx.db.log("automatización", &format!("Plan aprobado: {title} ({total} pasos)"));
    for (i, step) in steps.iter().enumerate() {
        let n = i + 1;
        let id = format!("{call_id}-paso{n}");
        if started.elapsed() > PLAN_TOTAL_TIMEOUT {
            report.push(format!("Paso {n}: no se ejecutó (tiempo total agotado)."));
            break;
        }
        let params = if step.parametros.is_null() {
            serde_json::json!({})
        } else {
            step.parametros.clone()
        };
        let mut prepared = match tools::prepare_async(ctx, &step.herramienta, &params).await {
            Ok(p) => p,
            Err(e) => {
                emit(StreamEvent::ToolEnd {
                    call_id: id,
                    ok: false,
                    summary: format!("Paso {n}: {e}"),
                });
                report.push(format!("Paso {n} ({}): ERROR — {e}", step.herramienta));
                break;
            }
        };
        if prepared.take_plan().is_some() {
            report.push(format!("Paso {n}: los planes no pueden anidarse."));
            break;
        }
        emit(StreamEvent::ToolStart {
            call_id: id.clone(),
            name: step.herramienta.clone(),
            label: format!("Paso {n}/{total}: {}", prepared.label),
        });
        // El plan aprobado cubre los pasos normales; los delicados se confirman otra vez.
        if let Permission::Confirm {
            title,
            details,
            diff,
            image,
        } = &prepared.permission
        {
            if tools::NEEDS_OWN_CONFIRM.contains(&step.herramienta.as_str()) {
                let rx = env.confirms.ask(&id);
                emit(StreamEvent::ConfirmRequest {
                    confirm_id: id.clone(),
                    title: format!("Paso {n}: {title}"),
                    details: details.clone(),
                    diff: diff.clone(),
                    image: image.clone(),
                });
                if !matches!(tokio::time::timeout(CONFIRM_TIMEOUT, rx).await, Ok(Ok(true))) {
                    emit(StreamEvent::ToolEnd {
                        call_id: id,
                        ok: false,
                        summary: "No autorizado".into(),
                    });
                    report.push(format!("Paso {n}: no autorizado por el usuario; plan detenido."));
                    break;
                }
            }
        }
        match tokio::time::timeout(PLAN_STEP_TIMEOUT, tools::run(ctx, prepared)).await {
            Ok(Ok(out)) => {
                emit(StreamEvent::ToolEnd {
                    call_id: id,
                    ok: true,
                    summary: format!("Paso {n}/{total} listo"),
                });
                untrusted |= out.untrusted;
                let brief: String = out.text.chars().take(600).collect();
                report.push(format!("Paso {n} ({}): OK — {brief}", step.herramienta));
                done += 1;
            }
            Ok(Err(e)) => {
                emit(StreamEvent::ToolEnd {
                    call_id: id,
                    ok: false,
                    summary: e.clone(),
                });
                report.push(format!("Paso {n} ({}): ERROR — {e}", step.herramienta));
                break;
            }
            Err(_) => {
                emit(StreamEvent::ToolEnd {
                    call_id: id,
                    ok: false,
                    summary: "Tardó demasiado".into(),
                });
                report.push(format!(
                    "Paso {n} ({}): se canceló por tardar más de {} s.",
                    step.herramienta,
                    PLAN_STEP_TIMEOUT.as_secs()
                ));
                break;
            }
        }
    }
    ctx.db
        .log("automatización", &format!("Plan «{title}»: {done}/{total} pasos completados"));
    (
        format!("Plan «{title}»: {done} de {total} pasos completados.\n{}", report.join("\n")),
        untrusted,
    )
}

/// Resumen final (v0.7 + v0.8): qué se hizo, qué se comprobó, si se logró lo
/// pedido y avisos honestos.
fn final_report(
    goal: &str,
    actions: &[ActionRecord],
    answer: &str,
    fetched: &str,
) -> (Vec<String>, Option<String>, crate::verification::Verdict) {
    let mut warnings = Vec::new();
    let did_something = actions.iter().any(|a| a.status == "ok");
    if !did_something && crate::agents::claims_action(answer) {
        warnings.push(
            "Canela dijo haber hecho algo, pero en esta respuesta no ejecutó ninguna acción. Pídeselo de nuevo si lo necesitas."
                .to_string(),
        );
    }
    for a in actions.iter().filter(|a| a.verified == Some(false)) {
        warnings.push(format!(
            "La comprobación de «{}» falló: {}.",
            a.label,
            a.detail.clone().unwrap_or_default()
        ));
    }
    let uncited = crate::agents::uncited_sources(answer, fetched);
    if !uncited.is_empty() {
        warnings.push(format!(
            "Cita fuentes que no consultó en esta respuesta: {}. Compruébalas antes de fiarte.",
            uncited.join(", ")
        ));
    }
    let verdict = crate::verification::assess(goal, actions, answer, fetched);
    let unfinished = matches!(verdict.status.as_str(), "parcial" | "no_logrado");
    if unfinished && crate::verification::claims_done(answer) && did_something {
        warnings.push(format!(
            "La respuesta da a entender que terminó, pero la comprobación no lo confirma. {}",
            verdict.summary
        ));
    }
    let mut lines: Vec<String> = Vec::new();
    if unfinished {
        lines.push(verdict.summary.clone());
    }
    if !warnings.is_empty() {
        lines.push(format!("⚠ {}", warnings.join(" ")));
    }
    let notice = (!lines.is_empty()).then(|| lines.join("\n"));
    (warnings, notice, verdict)
}

fn merge_notice(a: Option<String>, b: Option<String>) -> Option<String> {
    match (a, b) {
        (Some(x), Some(y)) => Some(format!("{x}\n{y}")),
        (x, y) => x.or(y),
    }
}

/// Ejecuta una respuesta completa. Siempre termina con Done o Error.
pub async fn run(env: AgentEnv, emit: Emit) {
    // Toda la respuesta (incluidas las herramientas) cuenta como actividad:
    // el modelo local no se descarga hasta que termine.
    let _busy = crate::resources::busy();
    let provider = env.settings.provider;
    let model = model_of(&env.settings);
    let agent_id = env.agent.map(|a| a.id);
    let agent_name = env.agent.map(|a| a.name).unwrap_or("Canela").to_string();
    // Contexto compartido controlado: de las respuestas de OTRO agente sólo
    // pasa el texto final, no los datos internos de sus herramientas.
    let history: Vec<ChatMessage> = env
        .history
        .iter()
        .cloned()
        .map(|mut m| {
            if m.agent.is_some() && m.agent != agent_id {
                m.transcript = None;
            }
            m
        })
        .collect();
    let base = native_history(provider, &history);
    // Cada agente sólo VE sus herramientas.
    let defs: Vec<_> = if env.tools.is_some() {
        tools::definitions()
            .into_iter()
            .filter(|d| env.agent.is_none_or(|a| a.allows(d.name)))
            .collect()
    } else {
        vec![]
    };
    let mut transcript: Vec<Value> = Vec::new();
    let mut tainted = false;
    let mut usage = super::pricing::Usage::default();
    let mut last_model = None;
    let mut actions: Vec<ActionRecord> = Vec::new();
    let mut fetched = String::new();
    // Texto visible de la respuesta (para el informe final).
    let answer = Arc::new(Mutex::new(String::new()));
    let sink = answer.clone();
    let outer = emit.clone();
    let emit: Emit = Arc::new(move |e| {
        if let StreamEvent::Delta { text } = &e {
            sink.lock().unwrap().push_str(text);
        }
        outer(e)
    });
    let goal = env.goal.clone().unwrap_or_else(|| {
        env.history
            .iter()
            .rev()
            .find(|m| m.role == super::Role::User)
            .map(|m| m.text.clone())
            .unwrap_or_default()
    });
    let finish = |emit: &Emit, actions: &[ActionRecord], fetched: &str, done: StreamEvent| {
        let text = answer.lock().unwrap().clone();
        let (warnings, notice, verdict) = final_report(&goal, actions, &text, fetched);
        if !actions.is_empty() || !warnings.is_empty() || !verdict.items.is_empty() {
            emit(StreamEvent::Report {
                agent: agent_name.clone(),
                actions: actions.to_vec(),
                warnings,
                verdict: Some(verdict),
            });
        }
        let done = match done {
            StreamEvent::Done {
                stop_reason,
                model,
                notice: n,
                provider,
                transcript,
                usage,
                cost_usd,
            } => StreamEvent::Done {
                stop_reason,
                model,
                notice: merge_notice(n, notice),
                provider,
                transcript,
                usage,
                cost_usd,
            },
            other => other,
        };
        emit(done);
    };

    for step in 0..MAX_STEPS {
        let mut messages = base.clone();
        messages.extend(transcript.iter().cloned());
        let emit_ref: &(dyn Fn(StreamEvent) + Send + Sync) = &*emit;
        let outcome = match super::complete(&env.http, &env.settings, &env.api_key, &env.system, &messages, &defs, emit_ref).await {
            Ok(o) => o,
            Err(message) => {
                // Lo ya hecho antes del error también se informa.
                if !actions.is_empty() {
                    let text = answer.lock().unwrap().clone();
                    emit(StreamEvent::Report {
                        agent: agent_name.clone(),
                        actions: actions.clone(),
                        warnings: vec![],
                        verdict: Some(crate::verification::assess(&goal, &actions, &text, &fetched)),
                    });
                }
                emit(StreamEvent::Error {
                    message: super::pricing::redact_secrets(&message),
                });
                return;
            }
        };
        last_model = outcome.model.clone().or(last_model);
        usage.add(&outcome.usage);
        let spent_here = super::pricing::price(provider, &model).map(|p| super::pricing::cost(p, &usage));
        let over_budget = matches!((env.max_cost_usd, spent_here), (Some(max), Some(c)) if c > max);
        if let Some(a) = &outcome.assistant {
            transcript.push(a.clone());
        }
        let ctx = match (&env.tools, outcome.tool_calls.is_empty()) {
            (Some(_), false) if over_budget => {
                // Seguir llamando al modelo superaría el límite de gasto.
                finish(
                    &emit,
                    &actions,
                    &fetched,
                    StreamEvent::Done {
                        stop_reason: Some("budget".into()),
                        model: last_model,
                        notice: Some(format!(
                            "Me detuve para no superar tu límite de gasto mensual (esta respuesta ya costó ≈ {:.3} US$ según los tokens que informó el proveedor). Súbelo en Ajustes si quieres que continúe.",
                            spent_here.unwrap_or(0.0)
                        )),
                        provider,
                        transcript,
                        usage,
                        cost_usd: spent_here,
                    },
                );
                return;
            }
            (Some(ctx), false) => ctx.clone(),
            _ => {
                finish(
                    &emit,
                    &actions,
                    &fetched,
                    StreamEvent::Done {
                        stop_reason: outcome.stop_reason,
                        model: last_model,
                        notice: outcome.notice,
                        provider,
                        transcript,
                        usage,
                        cost_usd: spent_here,
                    },
                );
                return;
            }
        };
        let mut results = Vec::new();
        for call in &outcome.tool_calls {
            let (mut result, untrusted, rec) = execute(&env, &ctx, call, &emit, tainted).await;
            tainted |= untrusted;
            actions.push(rec);
            if !result.is_error && matches!(call.name.as_str(), "buscar_wikipedia" | "leer_pagina") {
                fetched.push_str(&result.text);
                fetched.push('\n');
            }
            // Con un modelo externo, lo que leen las herramientas (archivos,
            // documentos) se revisa antes de enviarlo: los secretos no salen.
            if provider.is_cloud() {
                if let Some(safe) = crate::privacy::redact_for_external(&result.text, super::anthropic_or_openai_label(provider)) {
                    result.text = safe;
                    result.images.clear();
                }
            }
            results.push(result);
        }
        transcript.extend(tool_results(provider, &results));
        if step + 1 == MAX_STEPS {
            finish(
                &emit,
                &actions,
                &fetched,
                StreamEvent::Done {
                    stop_reason: Some("max_steps".into()),
                    model: last_model,
                    notice: Some("Me detuve tras varios pasos seguidos para no excederme. Dime si quieres que continúe.".into()),
                    provider,
                    transcript,
                    usage,
                    cost_usd: spent_here,
                },
            );
            return;
        }
        emit(StreamEvent::Delta { text: "\n\n".into() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn confirmations_resolve_once() {
        let c = Confirmations::default();
        let rx = c.ask("a");
        assert!(c.resolve("a", true));
        assert!(rx.await.unwrap());
        assert!(!c.resolve("a", true));
        let rx = c.ask("b");
        drop(c);
        assert!(rx.await.is_err(), "si se cancela la respuesta, la confirmación se descarta");
    }
}
