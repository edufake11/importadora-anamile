//! Proveedor Anthropic (Claude) — Messages API vía HTTP con streaming SSE.
//!
//! Rust no tiene SDK oficial de Anthropic, así que usamos la API REST
//! documentada (`POST /v1/messages`). Puntos clave:
//! - `anthropic-version: 2026-10-21` (sin temperature/top_p/top_k).
//! - Razonamiento adaptativo + `output_config.effort` configurable.
//! - Caché de prompt automática (`cache_control` de nivel superior).
//! - Fallback del servidor ante rechazos de clasificadores de seguridad
//!   (`fallbacks: "default"`) en los modelos que lo admiten.
//! - Herramientas con `eager_input_streaming`: la entrada se valida aquí
//!   (JSON completo) y en `tools::prepare` (esquema) antes de ejecutar nada.
//! - Historial de sólo-anexar: los mensajes del asistente (razonamiento
//!   firmado incluido) y los resultados de herramientas se reenvían tal cual.

use super::{ChatMessage, ParseOut, ProviderStream, Role, StreamEvent, ToolCall, ToolDef, ToolResult, TurnOutcome};
use crate::ai::sse::SseEvent;
use crate::settings::ProviderId;
use crate::vision::ImageData;
use serde_json::{json, Value};
use std::collections::HashMap;

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

pub fn native_messages(history: &[ChatMessage]) -> Vec<Value> {
    let mut out = Vec::new();
    for m in history {
        match m.role {
            Role::User if m.images.is_empty() => out.push(json!({ "role": "user", "content": m.text })),
            Role::User => {
                let mut content: Vec<Value> = m.images.iter().map(image_block).collect();
                if !m.text.trim().is_empty() {
                    content.push(json!({ "type": "text", "text": m.text }));
                }
                out.push(json!({ "role": "user", "content": content }));
            }
            Role::Assistant => {
                if m.provider == Some(ProviderId::Anthropic) {
                    if let Some(t) = m.transcript.as_ref().filter(|t| !t.is_empty()) {
                        out.extend(t.iter().cloned());
                        continue;
                    }
                }
                if !m.text.trim().is_empty() {
                    out.push(json!({ "role": "assistant", "content": m.text }));
                }
            }
        }
    }
    out
}

pub fn build_body(model: &str, effort: &str, system: &str, messages: &[Value], tools: &[ToolDef]) -> Value {
    let mut body = json!({
        "model": model,
        "max_tokens": MAX_TOKENS,
        "stream": true,
        "system": [{ "type": "text", "text": system }],
        "messages": messages,
        "cache_control": { "type": "ephemeral" },
    });
    if !tools.is_empty() {
        body["tools"] = Value::Array(
            tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.schema,
                        "eager_input_streaming": true,
                    })
                })
                .collect(),
        );
    }
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
    messages: &[Value],
    tools: &[ToolDef],
) -> reqwest::RequestBuilder {
    // En pruebas y compilaciones de desarrollo se puede redirigir a un servidor
    // local simulado. Las versiones instalables (release) siempre usan la API real.
    #[cfg(any(test, debug_assertions))]
    let url = std::env::var("CANELA_TEST_ANTHROPIC_URL").unwrap_or_else(|_| API_URL.to_string());
    #[cfg(not(any(test, debug_assertions)))]
    let url = API_URL;
    let mut req = http
        .post(url)
        .header("x-api-key", api_key)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json");
    if supports_fallbacks(model) {
        req = req.header("anthropic-beta", FALLBACK_BETA);
    }
    req.json(&build_body(model, effort, system, messages, tools))
}

pub fn image_block(img: &ImageData) -> Value {
    json!({ "type": "image", "source": { "type": "base64", "media_type": img.media_type, "data": img.data } })
}

/// Un único mensaje de usuario con todos los resultados de herramientas.
/// Las imágenes (capturas autorizadas) van dentro del propio tool_result.
pub fn tool_results_message(results: &[ToolResult]) -> Vec<Value> {
    let blocks: Vec<Value> = results
        .iter()
        .map(|r| {
            let content = if r.images.is_empty() {
                json!(r.text)
            } else {
                let mut c = vec![json!({ "type": "text", "text": r.text })];
                c.extend(r.images.iter().map(image_block));
                Value::Array(c)
            };
            json!({ "type": "tool_result", "tool_use_id": r.id, "content": content, "is_error": r.is_error })
        })
        .collect();
    vec![json!({ "role": "user", "content": blocks })]
}

/// Prepara los bloques del asistente para reenviarlos en el siguiente turno.
/// - Tras un fallback a mitad de respuesta, se omiten los bloques de
///   razonamiento/herramienta anteriores al último bloque `fallback`.
/// - Se descartan bloques de razonamiento incompletos (sin firma) y textos vacíos.
pub fn sanitize_for_replay(blocks: &[Value]) -> Vec<Value> {
    let last_fallback = blocks.iter().rposition(|b| b["type"] == "fallback");
    blocks
        .iter()
        .enumerate()
        .filter(|(i, b)| {
            let t = b["type"].as_str().unwrap_or("");
            if let Some(fb) = last_fallback {
                if *i < fb && matches!(t, "thinking" | "redacted_thinking" | "tool_use" | "server_tool_use") {
                    return false;
                }
            }
            match t {
                "thinking" => b.get("signature").and_then(Value::as_str).is_some_and(|s| !s.is_empty()),
                "text" => b["text"].as_str().is_some_and(|s| !s.is_empty()),
                _ => true,
            }
        })
        .map(|(_, b)| b.clone())
        .collect()
}

#[derive(Default)]
pub struct AnthropicStream {
    blocks: Vec<Value>,
    partial_json: Vec<String>,
    bad_input: HashMap<usize, String>,
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

    fn outcome(&mut self) -> TurnOutcome {
        let refused = self.stop_reason.as_deref() == Some("refusal");
        let mut notice = self.notice.take();
        if refused {
            notice =
                Some("Claude no pudo continuar con esta solicitud (rechazo por políticas de seguridad). Prueba a reformularla.".into());
        } else if self.stop_reason.as_deref() == Some("max_tokens") {
            notice = Some("La respuesta se cortó por alcanzar el límite de longitud.".into());
        }
        // Un fallback a mitad de respuesta descarta las herramientas anteriores al cambio.
        let last_fallback = self.blocks.iter().rposition(|b| b["type"] == "fallback");
        let tool_calls: Vec<ToolCall> = if refused || self.stop_reason.as_deref() == Some("max_tokens") {
            vec![]
        } else {
            self.blocks
                .iter()
                .enumerate()
                .filter(|(i, b)| b["type"] == "tool_use" && last_fallback.is_none_or(|fb| *i > fb))
                .map(|(i, b)| ToolCall {
                    id: b["id"].as_str().unwrap_or_default().to_string(),
                    name: b["name"].as_str().unwrap_or_default().to_string(),
                    input: match self.bad_input.get(&i) {
                        Some(e) => Err(e.clone()),
                        None => Ok(b["input"].clone()),
                    },
                })
                .collect()
        };
        let blocks: Vec<Value> = self.blocks.iter().filter(|b| !b.is_null()).cloned().collect();
        let kept = sanitize_for_replay(&blocks);
        let assistant = if refused || kept.is_empty() {
            None
        } else {
            Some(json!({ "role": "assistant", "content": kept }))
        };
        TurnOutcome {
            assistant,
            tool_calls,
            stop_reason: self.stop_reason.clone(),
            model: self.model.clone(),
            notice,
        }
    }
}

impl ProviderStream for AnthropicStream {
    fn on_event(&mut self, ev: &SseEvent) -> Vec<ParseOut> {
        let Ok(data) = serde_json::from_str::<Value>(&ev.data) else {
            return vec![];
        };
        let index = data["index"].as_u64().unwrap_or(0) as usize;
        match data["type"].as_str().unwrap_or("") {
            "message_start" => {
                let model = data["message"]["model"].as_str().unwrap_or_default().to_string();
                self.model = Some(model.clone());
                vec![ParseOut::Event(StreamEvent::Started { model })]
            }
            "content_block_start" => {
                self.slot(index);
                let mut block = data["content_block"].clone();
                let kind = block["type"].as_str().unwrap_or("").to_string();
                if kind == "tool_use" && block.get("input").is_none() {
                    block["input"] = json!({});
                }
                self.blocks[index] = block.clone();
                match kind.as_str() {
                    "thinking" | "redacted_thinking" => {
                        vec![ParseOut::Event(StreamEvent::Thinking)]
                    }
                    "fallback" => {
                        let to = block["to"]["model"].as_str().unwrap_or("otro modelo");
                        self.model = Some(to.to_string());
                        self.notice = Some(format!("Respuesta completada con el modelo de respaldo {to}."));
                        vec![]
                    }
                    "text" => {
                        let t = block["text"].as_str().unwrap_or("").to_string();
                        if t.is_empty() {
                            vec![]
                        } else {
                            vec![ParseOut::Event(StreamEvent::Delta { text: t })]
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
                        vec![ParseOut::Event(StreamEvent::Delta { text: t.to_string() })]
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
                        self.partial_json[index].push_str(delta["partial_json"].as_str().unwrap_or(""));
                        vec![]
                    }
                    _ => vec![],
                }
            }
            "content_block_stop" => {
                if index < self.partial_json.len() && self.blocks[index]["type"] == "tool_use" {
                    let raw = std::mem::take(&mut self.partial_json[index]);
                    if !raw.trim().is_empty() {
                        match serde_json::from_str::<Value>(&raw) {
                            Ok(input) if input.is_object() => self.blocks[index]["input"] = input,
                            _ => {
                                // JSON incompleto o inválido: no se ejecuta; se informa al modelo.
                                self.bad_input.insert(
                                    index,
                                    "INVALID_JSON: la entrada de la herramienta no es un JSON válido. Vuelve a intentarlo.".into(),
                                );
                            }
                        }
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
            "message_stop" => vec![ParseOut::Finished(self.outcome())],
            "error" => {
                let msg = data["error"]["message"].as_str().unwrap_or("error desconocido");
                let kind = data["error"]["type"].as_str().unwrap_or("");
                vec![ParseOut::Failed(if kind == "overloaded_error" {
                    "Claude está saturado en este momento. Inténtalo de nuevo en unos segundos.".to_string()
                } else {
                    format!("Error de Claude: {msg}")
                })]
            }
            _ => vec![],
        }
    }

    fn finish(&mut self) -> Option<TurnOutcome> {
        None
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
            provider: None,
            transcript: None,
            images: vec![],
        }
    }

    fn run(raw: &str) -> (Vec<StreamEvent>, Option<TurnOutcome>) {
        let mut sse = SseParser::new();
        let mut st = AnthropicStream::new();
        let mut events = vec![];
        for ev in sse.push(raw.as_bytes()) {
            for o in st.on_event(&ev) {
                match o {
                    ParseOut::Event(e) => events.push(e),
                    ParseOut::Finished(o) => return (events, Some(o)),
                    ParseOut::Failed(m) => panic!("{m}"),
                }
            }
        }
        (events, None)
    }

    #[test]
    fn body_has_expected_shape() {
        let tools = vec![ToolDef {
            name: "recordar",
            description: "d",
            schema: json!({"type":"object"}),
        }];
        let body = build_body(
            "claude-opus-5-5",
            "medium",
            "Eres Canela",
            &native_messages(&[user("hola")]),
            &tools,
        );
        assert_eq!(body["model"], "claude-opus-5-5");
        assert_eq!(body["stream"], true);
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(body["output_config"]["effort"], "medium");
        assert_eq!(body["fallbacks"], "default");
        assert_eq!(body["messages"][0]["content"], "hola");
        assert_eq!(body["tools"][0]["name"], "recordar");
        assert_eq!(body["tools"][0]["eager_input_streaming"], true);
        assert!(body.get("temperature").is_none());
        assert!(body.get("tool_choice").is_none());
    }

    #[test]
    fn legacy_models_skip_adaptive_and_fallbacks() {
        let body = build_body("claude-haiku-4-5", "low", "s", &[], &[]);
        assert!(body.get("thinking").is_none());
        assert!(body.get("fallbacks").is_none());
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn transcript_is_replayed_verbatim_only_for_same_provider() {
        let transcript = vec![
            json!({"role":"assistant","content":[{"type":"thinking","thinking":"","signature":"s"},{"type":"tool_use","id":"t1","name":"x","input":{}}]}),
            json!({"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok","is_error":false}]}),
            json!({"role":"assistant","content":[{"type":"text","text":"¡Listo!"}]}),
        ];
        let a = ChatMessage {
            role: Role::Assistant,
            text: "¡Listo!".into(),
            provider: Some(ProviderId::Anthropic),
            transcript: Some(transcript.clone()),
            images: vec![],
        };
        let msgs = native_messages(&[user("abre algo"), a.clone(), user("gracias")]);
        assert_eq!(msgs.len(), 5);
        assert_eq!(msgs[1], transcript[0]);
        // Si la respuesta vino de otro proveedor, sólo se usa el texto.
        let o = ChatMessage {
            provider: Some(ProviderId::Openai),
            ..a
        };
        let msgs = native_messages(&[user("abre algo"), o]);
        assert_eq!(msgs[1], json!({"role":"assistant","content":"¡Listo!"}));
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
        let types: Vec<String> = sanitize_for_replay(&blocks)
            .iter()
            .map(|b| b["type"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(types, vec!["text", "fallback", "thinking", "text"]);
    }

    #[test]
    fn parses_text_stream() {
        let raw = concat!(
            "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"model\":\"claude-opus-5-5\"}}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"SIG\"}}\n\n",
            "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"¡Hola, \"}}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"humano!\"}}\n\n",
            "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        );
        let (events, outcome) = run(raw);
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
        let o = outcome.unwrap();
        assert!(o.tool_calls.is_empty());
        let content = &o.assistant.unwrap()["content"];
        assert_eq!(content[0]["signature"], "SIG");
        assert_eq!(content[1]["text"], "¡Hola, humano!");
    }

    #[test]
    fn parses_tool_use_with_streamed_input() {
        let raw = concat!(
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"abrir_aplicacion\",\"input\":{}}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"nombre\\\": \\\"Visual\"}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\" Studio Code\\\"}\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
            "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_2\",\"name\":\"x\",\"input\":{}}}\n\n",
            "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"a\\\": tru\"}}\n\n",
            "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let (_, o) = run(raw);
        let o = o.unwrap();
        assert_eq!(o.tool_calls.len(), 2);
        assert_eq!(o.tool_calls[0].input.as_ref().unwrap()["nombre"], "Visual Studio Code");
        assert!(o.tool_calls[1].input.as_ref().unwrap_err().starts_with("INVALID_JSON"));
        // El bloque tool_use se conserva para reenviarlo con su resultado.
        assert_eq!(o.assistant.unwrap()["content"][0]["id"], "toolu_1");
    }

    #[test]
    fn refusal_is_not_replayed_and_runs_no_tools() {
        let raw = concat!(
            "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"x\",\"input\":{}}}\n\n",
            "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"refusal\"}}\n\n",
            "data: {\"type\":\"message_stop\"}\n\n",
        );
        let o = run(raw).1.unwrap();
        assert!(o.assistant.is_none());
        assert!(o.tool_calls.is_empty());
        assert!(o.notice.unwrap().contains("rechazo"));
    }

    fn img() -> ImageData {
        ImageData {
            media_type: "image/jpeg".into(),
            data: "AAAA".into(),
            width: 1,
            height: 1,
        }
    }

    #[test]
    fn tool_results_shape() {
        let m = tool_results_message(&[ToolResult {
            id: "t1".into(),
            text: "hecho".into(),
            is_error: false,
            images: vec![],
        }]);
        assert_eq!(m[0]["role"], "user");
        assert_eq!(m[0]["content"][0]["type"], "tool_result");
        assert_eq!(m[0]["content"][0]["tool_use_id"], "t1");
        let m = tool_results_message(&[ToolResult {
            id: "t2".into(),
            text: "captura".into(),
            is_error: false,
            images: vec![img()],
        }]);
        assert_eq!(m[0]["content"][0]["content"][1]["type"], "image");
        assert_eq!(m[0]["content"][0]["content"][1]["source"]["media_type"], "image/jpeg");
    }

    #[test]
    fn user_images_go_before_text() {
        let u = ChatMessage {
            role: Role::User,
            text: "¿qué falla?".into(),
            provider: None,
            transcript: None,
            images: vec![img()],
        };
        let msgs = native_messages(&[u]);
        assert_eq!(msgs[0]["content"][0]["type"], "image");
        assert_eq!(msgs[0]["content"][1]["text"], "¿qué falla?");
    }
}
