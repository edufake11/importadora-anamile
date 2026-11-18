//! Proveedor Anthropic (Claude) — Messages API vía HTTP con streaming SSE.
//!
//! Rust no tiene SDK oficial de Anthropic, así que usamos la API REST
//! documentada (`POST /v1/messages`). Puntos clave:
//! - `anthropic-version: 2026-10-21` (sin temperature/top_p/top_k).
//! - Razonamiento adaptativo + `output_config.effort` configurable.
//! - Caché de prompt automática (`cache_control` de nivel superior).
//! - Fallback del servidor ante rechazos de clasificadores de seguridad
//!   (`fallbacks: "default"`) en los modelos que lo admiten.
//! - Los bloques de contenido del asistente (incluidos los de razonamiento con
//!   su firma) se devuelven a la interfaz y se reenvían SIN modificar en los
//!   siguientes turnos: el historial es de sólo-anexar.

use super::{ChatMessage, ProviderStream, Role, StreamEvent};
use crate::ai::sse::SseEvent;
use serde_json::{json, Value};

pub const API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const API_VERSION: &str = "2026-10-21";
pub const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
pub const MAX_TOKENS: u32 = 64_000;

/// Modelos que admiten `fallbacks: "default"`.
const FALLBACK_MODELS: [&str; 5] = [
    "claude-fable-5-2",
    "claude-fable-5-1",
    "claude-opus-5-5",
    "claude-opus-5",
    "claude-sonnet-5-5",
];

pub fn supports_fallbacks(model: &str) -> bool {
    FALLBACK_MODELS.contains(&model)
}

/// Modelos antiguos que no aceptan razonamiento adaptativo ni `effort`.
pub fn supports_adaptive_thinking(model: &str) -> bool {
    const LEGACY: [&str; 8] = [
        "claude-3",
        "claude-haiku-4-5",
        "claude-sonnet-4-5",
        "claude-opus-4-5",
        "claude-opus-4-1",
        "claude-opus-4-0",
        "claude-sonnet-4-0",
        "claude-opus-4-2025",
    ];
    !LEGACY.iter().any(|p| model.starts_with(p))
}

pub fn build_body(model: &str, effort: &str, system: &str, messages: &[ChatMessage]) -> Value {
    let msgs: Vec<Value> = messages
        .iter()
        .filter_map(|m| match m.role {
            Role::User => Some(json!({ "role": "user", "content": m.text })),
            Role::Assistant => {
                if let Some(Value::Array(blocks)) = &m.anthropic_content {
                    if !blocks.is_empty() {
                        return Some(json!({ "role": "assistant", "content": blocks }));
                    }
                }
                if m.text.trim().is_empty() {
                    None
                } else {
                    Some(json!({ "role": "assistant", "content": m.text }))
                }
            }
        })
        .collect();

    let mut body = json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "stream": true,
        "system": [{ "type": "text", "text": system }],
        "messages": msgs,
        "cache_control": { "type": "ephemeral" },
    });
    if supports_adaptive_thinking(model) {
        body["thinking"] = json!({ "type": "adaptive" });
        body["output_config"] = json!({ "effort": effort });
    }
    if supports_fallbacks(model) {
        body["fallbacks"] = json!("default");
    }
    body
}

pub fn build_request(
    http: &reqwest::Client,
    api_key: &str,
    model: &str,
    effort: &str,
    system: &str,
    messages: &[ChatMessage],
) -> reqwest::RequestBuilder {
    let mut req = http
        .post(API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json");
    if supports_fallbacks(model) {
        req = req.header("anthropic-beta", FALLBACK_BETA);
    }
    req.json(&build_body(model, effort, system, messages))
}

/// Prepara los bloques del asistente para reenviarlos en el siguiente turno.
/// - Tras un fallback a mitad de respuesta, se omiten los bloques de
///   razonamiento/herramienta anteriores al último bloque `fallback`.
/// - Se descartan bloques de razonamiento incompletos (sin firma) y textos vacíos.
pub fn sanitize_for_replay(blocks: &[Value]) -> Option<Value> {
    let last_fallback = blocks.iter().rposition(|b| b["type"] == "fallback");
    let kept: Vec<Value> = blocks
        .iter()
        .enumerate()
        .filter(|(i, b)| {
            let t = b["type"].as_str().unwrap_or("");
            if let Some(fb) = last_fallback {
                if *i < fb
                    && matches!(
                        t,
                        "thinking" | "redacted_thinking" | "tool_use" | "server_tool_use"
                    )
                {
                    return false;
                }
            }
            match t {
                "thinking" => b
                    .get("signature")
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty()),
                "text" => b["text"].as_str().is_some_and(|s| !s.is_empty()),
                _ => true,
            }
        })
        .map(|(_, b)| b.clone())
        .collect();
    if kept.iter().any(|b| b["type"] == "text") {
        Some(Value::Array(kept))
    } else {
        None
    }
}

#[derive(Default)]
pub struct AnthropicStream {
    blocks: Vec<Value>,
    partial_json: Vec<String>,
    stop_reason: Option<String>,
    model: Option<String>,
    notice: Option<String>,
}

impl AnthropicStream {
    pub fn new() -> Self {
        Self::default()
    }

    fn slot(&mut self, index: usize) {
        while self.blocks.len() <= index {
            self.blocks.push(Value::Null);
            self.partial_json.push(String::new());
        }
    }
}

impl ProviderStream for AnthropicStream {
    fn on_event(&mut self, ev: &SseEvent) -> Vec<StreamEvent> {
        let Ok(data) = serde_json::from_str::<Value>(&ev.data) else {
            return vec![];
        };
        let index = data["index"].as_u64().unwrap_or(0) as usize;
        match data["type"].as_str().unwrap_or("") {
            "message_start" => {
                let model = data["message"]["model"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                self.model = Some(model.clone());
                vec![StreamEvent::Started { model }]
            }
            "content_block_start" => {
                self.slot(index);
                let block = data["content_block"].clone();
                let kind = block["type"].as_str().unwrap_or("").to_string();
                self.blocks[index] = block.clone();
                match kind.as_str() {
                    "thinking" | "redacted_thinking" => vec![StreamEvent::Thinking],
                    "fallback" => {
                        let to = block["to"]["model"].as_str().unwrap_or("otro modelo");
                        self.model = Some(to.to_string());
                        self.notice = Some(format!(
                            "Respuesta completada con el modelo de respaldo {to}."
                        ));
                        vec![]
                    }
                    "text" => {
                        let t = block["text"].as_str().unwrap_or("").to_string();
                        if t.is_empty() {
                            vec![]
                        } else {
                            vec![StreamEvent::Delta { text: t }]
                        }
                    }
                    _ => vec![],
                }
            }
            "content_block_delta" => {
                self.slot(index);
                let delta = &data["delta"];
                let block = &mut self.blocks[index];
                match delta["type"].as_str().unwrap_or("") {
                    "text_delta" => {
                        let t = delta["text"].as_str().unwrap_or("");
                        let prev = block["text"].as_str().unwrap_or("").to_string();
                        block["text"] = Value::String(prev + t);
                        vec![StreamEvent::Delta {
                            text: t.to_string(),
                        }]
                    }
                    "thinking_delta" => {
                        let t = delta["thinking"].as_str().unwrap_or("");
                        let prev = block["thinking"].as_str().unwrap_or("").to_string();
                        block["thinking"] = Value::String(prev + t);
                        vec![]
                    }
                    "signature_delta" => {
                        block["signature"] = delta["signature"].clone();
                        vec![]
                    }
                    "input_json_delta" => {
                        self.partial_json[index]
                            .push_str(delta["partial_json"].as_str().unwrap_or(""));
                        vec![]
                    }
                    _ => vec![],
                }
            }
            "content_block_stop" => {
                if index < self.partial_json.len() && !self.partial_json[index].is_empty() {
                    if let Ok(input) = serde_json::from_str::<Value>(&self.partial_json[index]) {
                        self.blocks[index]["input"] = input;
                    }
                }
                vec![]
            }
            "message_delta" => {
                if let Some(r) = data["delta"]["stop_reason"].as_str() {
                    self.stop_reason = Some(r.to_string());
                }
                vec![]
            }
            "message_stop" => vec![self.done()],
            "error" => {
                let msg = data["error"]["message"]
                    .as_str()
                    .unwrap_or("error desconocido");
                let kind = data["error"]["type"].as_str().unwrap_or("");
                let message = if kind == "overloaded_error" {
                    "Claude está saturado en este momento. Inténtalo de nuevo en unos segundos."
                        .to_string()
                } else {
                    format!("Error de Claude: {msg}")
                };
                vec![StreamEvent::Error { message }]
            }
            _ => vec![],
        }
    }

    fn finish(&mut self) -> Option<StreamEvent> {
        None
    }
}

impl AnthropicStream {
    fn done(&mut self) -> StreamEvent {
        let refused = self.stop_reason.as_deref() == Some("refusal");
        let blocks: Vec<Value> = self
            .blocks
            .iter()
            .filter(|b| !b.is_null())
            .cloned()
            .collect();
        let mut notice = self.notice.take();
        if refused {
            notice = Some("Claude no pudo continuar con esta solicitud (rechazo por políticas de seguridad). Prueba a reformularla.".into());
        } else if self.stop_reason.as_deref() == Some("max_tokens") {
            notice = Some("La respuesta se cortó por alcanzar el límite de longitud.".into());
        }
        StreamEvent::Done {
            stop_reason: self.stop_reason.clone(),
            model: self.model.clone(),
            anthropic_content: if refused {
                None
            } else {
                sanitize_for_replay(&blocks)
            },
            notice,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::sse::SseParser;

    fn user(t: &str) -> ChatMessage {
        ChatMessage {
            role: Role::User,
            text: t.into(),
            anthropic_content: None,
        }
    }

    #[test]
    fn body_has_expected_shape() {
        let body = build_body("claude-opus-5-5", "medium", "Eres Canela", &[user("hola")]);
        assert_eq!(body["model"], "claude-opus-5-5");
        assert_eq!(body["stream"], true);
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(body["output_config"]["effort"], "medium");
        assert_eq!(body["fallbacks"], "default");
        assert_eq!(body["messages"][0]["content"], "hola");
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn legacy_models_skip_adaptive_and_fallbacks() {
        let body = build_body("claude-haiku-4-5", "low", "s", &[user("x")]);
        assert!(body.get("thinking").is_none());
        assert!(body.get("output_config").is_none());
        assert!(body.get("fallbacks").is_none());
    }

    #[test]
    fn assistant_blocks_are_replayed_verbatim() {
        let blocks = json!([
            {"type": "thinking", "thinking": "", "signature": "sig123"},
            {"type": "text", "text": "¡Guau!"}
        ]);
        let msgs = vec![
            user("hola"),
            ChatMessage {
                role: Role::Assistant,
                text: "¡Guau!".into(),
                anthropic_content: Some(blocks.clone()),
            },
            user("¿y ahora?"),
        ];
        let body = build_body("claude-opus-5-5", "medium", "s", &msgs);
        assert_eq!(body["messages"][1]["content"], blocks);
    }

    #[test]
    fn empty_assistant_turns_are_skipped() {
        let msgs = vec![
            user("hola"),
            ChatMessage {
                role: Role::Assistant,
                text: "  ".into(),
                anthropic_content: None,
            },
            user("¿sigues ahí?"),
        ];
        let body = build_body("claude-opus-5-5", "medium", "s", &msgs);
        assert_eq!(body["messages"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn sanitize_drops_thinking_before_fallback_and_unsigned() {
        let blocks = vec![
            json!({"type": "thinking", "thinking": "", "signature": "a"}),
            json!({"type": "text", "text": "parcial"}),
            json!({"type": "fallback", "from": {"model": "claude-opus-5-5"}, "to": {"model": "claude-opus-4-8"}}),
            json!({"type": "thinking", "thinking": "", "signature": "b"}),
            json!({"type": "thinking", "thinking": "sin firma"}),
            json!({"type": "text", "text": "final"}),
            json!({"type": "text", "text": ""}),
        ];
        let out = sanitize_for_replay(&blocks).unwrap();
        let types: Vec<&str> = out
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["type"].as_str().unwrap())
            .collect();
        assert_eq!(types, vec!["text", "fallback", "thinking", "text"]);
    }

    #[test]
    fn parses_a_full_stream() {
        let raw = concat!(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-opus-5-5\"}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"SIG\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"¡Hola, \"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"humano!\"}}\n\n",
            "event: ping\ndata: {\"type\":\"ping\"}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        );
        let mut sse = SseParser::new();
        let mut st = AnthropicStream::new();
        let events: Vec<StreamEvent> = sse
            .push(raw.as_bytes())
            .iter()
            .flat_map(|e| st.on_event(e))
            .collect();
        let text: String = events
            .iter()
            .filter_map(|e| {
                if let StreamEvent::Delta { text } = e {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(text, "¡Hola, humano!");
        assert!(events.iter().any(|e| matches!(e, StreamEvent::Thinking)));
        match events.last().unwrap() {
            StreamEvent::Done {
                stop_reason,
                anthropic_content,
                model,
                ..
            } => {
                assert_eq!(stop_reason.as_deref(), Some("end_turn"));
                assert_eq!(model.as_deref(), Some("claude-opus-5-5"));
                let blocks = anthropic_content.as_ref().unwrap();
                assert_eq!(blocks[0]["signature"], "SIG");
                assert_eq!(blocks[1]["text"], "¡Hola, humano!");
            }
            other => panic!("esperaba Done, llegó {other:?}"),
        }
    }

    #[test]
    fn refusal_is_reported_and_not_replayed() {
        let mut st = AnthropicStream::new();
        st.on_event(&SseEvent {
            event: None,
            data: r#"{"type":"message_delta","delta":{"stop_reason":"refusal"}}"#.into(),
        });
        match st
            .on_event(&SseEvent {
                event: None,
                data: r#"{"type":"message_stop"}"#.into(),
            })
            .pop()
            .unwrap()
        {
            StreamEvent::Done {
                anthropic_content,
                notice,
                ..
            } => {
                assert!(anthropic_content.is_none());
                assert!(notice.unwrap().contains("rechazo"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn stream_error_event() {
        let mut st = AnthropicStream::new();
        let evs = st.on_event(&SseEvent {
            event: Some("error".into()),
            data: r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#
                .into(),
        });
        assert!(matches!(&evs[0], StreamEvent::Error { message } if message.contains("saturado")));
    }
}
