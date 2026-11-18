//! Bucle del agente: pide una respuesta al modelo, ejecuta las herramientas
//! que solicite (con permiso del usuario cuando corresponde) y repite hasta
//! que el modelo termine, con un límite de pasos.

use super::{build_turn, model_of, native_history, run_turn, tool_results, ChatMessage, StreamEvent, ToolCall, ToolResult};
use crate::settings::Settings;
use crate::tools::{self, Permission, ToolCtx};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::oneshot;

pub const MAX_STEPS: usize = 12;
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
}

type Emit = Arc<dyn Fn(StreamEvent) + Send + Sync>;

async fn execute(env: &AgentEnv, ctx: &ToolCtx, call: &ToolCall, emit: &Emit) -> ToolResult {
    let fail = |text: String| ToolResult {
        id: call.id.clone(),
        text,
        is_error: true,
        images: vec![],
    };
    let input = match &call.input {
        Ok(v) => v.clone(),
        Err(e) => return fail(e.clone()),
    };
    let prepared = match tools::prepare_async(ctx, &call.name, &input).await {
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
    if let Permission::Confirm {
        title,
        details,
        diff,
        image,
    } = &prepared.permission
    {
        let rx = env.confirms.ask(&call.id);
        emit(StreamEvent::ConfirmRequest {
            confirm_id: call.id.clone(),
            title: title.clone(),
            details: details.clone(),
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
    match tools::run(ctx, prepared).await {
        Ok(out) => {
            emit(StreamEvent::ToolEnd {
                call_id: call.id.clone(),
                ok: true,
                summary: label,
            });
            ToolResult {
                id: call.id.clone(),
                text: out.text,
                is_error: false,
                images: out.images,
            }
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

/// Ejecuta una respuesta completa. Siempre termina con Done o Error.
pub async fn run(env: AgentEnv, emit: Emit) {
    let provider = env.settings.provider;
    let model = model_of(&env.settings);
    let base = native_history(provider, &env.history);
    let defs = if env.tools.is_some() { tools::definitions() } else { vec![] };
    let mut transcript: Vec<Value> = Vec::new();
    let mut last_model = None;

    for step in 0..MAX_STEPS {
        let mut messages = base.clone();
        messages.extend(transcript.iter().cloned());
        let (req, parser) = build_turn(&env.http, &env.settings, &env.api_key, &env.system, &messages, &defs);
        let emit_ref: &(dyn Fn(StreamEvent) + Send + Sync) = &*emit;
        let outcome = match run_turn(provider, &model, req, parser, emit_ref).await {
            Ok(o) => o,
            Err(message) => {
                emit(StreamEvent::Error { message });
                return;
            }
        };
        last_model = outcome.model.clone().or(last_model);
        if let Some(a) = &outcome.assistant {
            transcript.push(a.clone());
        }
        let ctx = match (&env.tools, outcome.tool_calls.is_empty()) {
            (Some(ctx), false) => ctx.clone(),
            _ => {
                emit(StreamEvent::Done {
                    stop_reason: outcome.stop_reason,
                    model: last_model,
                    notice: outcome.notice,
                    provider,
                    transcript,
                });
                return;
            }
        };
        let mut results = Vec::new();
        for call in &outcome.tool_calls {
            results.push(execute(&env, &ctx, call, &emit).await);
        }
        transcript.extend(tool_results(provider, &results));
        if step + 1 == MAX_STEPS {
            emit(StreamEvent::Done {
                stop_reason: Some("max_steps".into()),
                model: last_model,
                notice: Some("Me detuve tras varios pasos seguidos para no excederme. Dime si quieres que continúe.".into()),
                provider,
                transcript,
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
