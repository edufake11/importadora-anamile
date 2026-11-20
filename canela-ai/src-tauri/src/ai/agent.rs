//! Bucle del agente: pide una respuesta al modelo, ejecuta las herramientas
//! que solicite (con permiso del usuario cuando corresponde) y repite hasta
//! que el modelo termine, con un límite de pasos.

use super::{model_of, native_history, tool_results, ChatMessage, StreamEvent, ToolCall, ToolResult};
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
}

type Emit = Arc<dyn Fn(StreamEvent) + Send + Sync>;

/// Ejecuta una llamada. `tainted` = ya se leyó contenido externo en esta respuesta.
/// Devuelve el resultado y si trae contenido externo.
async fn execute(env: &AgentEnv, ctx: &ToolCtx, call: &ToolCall, emit: &Emit, tainted: bool) -> (ToolResult, bool) {
    let fail = |text: String| {
        (
            ToolResult {
                id: call.id.clone(),
                name: call.name.clone(),
                text,
                is_error: true,
                images: vec![],
            },
            false,
        )
    };
    let input = match &call.input {
        Ok(v) => v.clone(),
        Err(e) => return fail(e.clone()),
    };
    let mut prepared = match tools::prepare_async(ctx, &call.name, &input).await {
        Ok(p) => p,
        Err(e) => {
            emit(StreamEvent::ToolEnd {
                call_id: call.id.clone(),
                ok: false,
                summary: e.clone(),
            });
            return fail(e);
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
            return fail("El usuario NO autorizó esta acción; no se realizó. No la repitas salvo que te lo pida.".into());
        }
    }
    if let Some((title, steps)) = prepared.take_plan() {
        let (text, untrusted) = run_plan(env, ctx, &call.id, &title, &steps, emit).await;
        emit(StreamEvent::ToolEnd {
            call_id: call.id.clone(),
            ok: true,
            summary: label,
        });
        return (
            ToolResult {
                id: call.id.clone(),
                name: call.name.clone(),
                text,
                is_error: false,
                images: vec![],
            },
            untrusted,
        );
    }
    match tools::run(ctx, prepared).await {
        Ok(out) => {
            emit(StreamEvent::ToolEnd {
                call_id: call.id.clone(),
                ok: true,
                summary: label,
            });
            (
                ToolResult {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    text: out.text,
                    is_error: false,
                    images: out.images,
                },
                out.untrusted,
            )
        }
        Err(e) => {
            emit(StreamEvent::ToolEnd {
                call_id: call.id.clone(),
                ok: false,
                summary: e.clone(),
            });
            fail(e)
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

/// Ejecuta una respuesta completa. Siempre termina con Done o Error.
pub async fn run(env: AgentEnv, emit: Emit) {
    // Toda la respuesta (incluidas las herramientas) cuenta como actividad:
    // el modelo local no se descarga hasta que termine.
    let _busy = crate::resources::busy();
    let provider = env.settings.provider;
    let model = model_of(&env.settings);
    let base = native_history(provider, &env.history);
    let defs = if env.tools.is_some() { tools::definitions() } else { vec![] };
    let mut transcript: Vec<Value> = Vec::new();
    let mut tainted = false;
    let mut usage = super::pricing::Usage::default();
    let mut last_model = None;

    for step in 0..MAX_STEPS {
        let mut messages = base.clone();
        messages.extend(transcript.iter().cloned());
        let emit_ref: &(dyn Fn(StreamEvent) + Send + Sync) = &*emit;
        let outcome = match super::complete(&env.http, &env.settings, &env.api_key, &env.system, &messages, &defs, emit_ref).await {
            Ok(o) => o,
            Err(message) => {
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
                emit(StreamEvent::Done {
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
                });
                return;
            }
            (Some(ctx), false) => ctx.clone(),
            _ => {
                emit(StreamEvent::Done {
                    stop_reason: outcome.stop_reason,
                    model: last_model,
                    notice: outcome.notice,
                    provider,
                    transcript,
                    usage,
                    cost_usd: super::pricing::price(provider, &model).map(|p| super::pricing::cost(p, &usage)),
                });
                return;
            }
        };
        let mut results = Vec::new();
        for call in &outcome.tool_calls {
            let (mut result, untrusted) = execute(&env, &ctx, call, &emit, tainted).await;
            tainted |= untrusted;
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
            emit(StreamEvent::Done {
                stop_reason: Some("max_steps".into()),
                model: last_model,
                notice: Some("Me detuve tras varios pasos seguidos para no excederme. Dime si quieres que continúe.".into()),
                provider,
                transcript,
                usage,
                cost_usd: super::pricing::price(provider, &model).map(|p| super::pricing::cost(p, &usage)),
            });
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
