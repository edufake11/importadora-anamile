//! Proveedor OpenAI (ChatGPT) — Chat Completions API con streaming SSE y
//! llamadas a funciones (herramientas).

use super::{ChatMessage, ParseOut, ProviderStream, Role, StreamEvent, ToolCall, ToolDef, ToolResult, TurnOutcome};
use crate::ai::sse::SseEvent;
use crate::settings::ProviderId;
use crate::vision::ImageData;
use serde_json::{json, Value};

pub const API_URL: &str = "https://api.openai.com/v1/chat/completions";

pub fn native_messages(history: &[ChatMessage]) -> Vec<Value> {
    let mut out = Vec::new();
    for m in history {
        match m.role {
            Role::User if m.images.is_empty() => out.push(json!({ "role": "user", "content": m.text })),
            Role::User => out.push(user_with_images(&m.text, &m.images)),
            Role::Assistant => {
                if m.provider == Some(ProviderId::Openai) {
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

pub fn build_body(model: &str, system: &str, messages: &[Value], tools: &[ToolDef]) -> Value {
    let mut msgs = vec![json!({ "role": "system", "content": system })];
    msgs.extend(messages.iter().cloned());
    let mut body = json!({ "model": model, "stream": true, "messages": msgs });
    if !tools.is_empty() {
        body["tools"] = Value::Array(
            tools
                .iter()
                .map(
                    |t| json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.schema } }),
                )
                .collect(),
        );
    }
    body
}

pub fn build_request(
    http: &reqwest::Client,
    api_key: &str,
    model: &str,
    system: &str,
    messages: &[Value],
    tools: &[ToolDef],
) -> reqwest::RequestBuilder {
    // En pruebas y compilaciones de desarrollo se puede redirigir a un servidor
    // local simulado. Las versiones instalables (release) siempre usan la API real.
    #[cfg(any(test, debug_assertions))]
    let url = std::env::var("CANELA_TEST_OPENAI_URL").unwrap_or_else(|_| API_URL.to_string());
    #[cfg(not(any(test, debug_assertions)))]
    let url = API_URL;
    http.post(url)
        .bearer_auth(api_key)
        .json(&build_body(model, system, messages, tools))
}

fn user_with_images(text: &str, images: &[ImageData]) -> Value {
    let mut content = vec![json!({ "type": "text", "text": text })];
    content.extend(
        images
            .iter()
            .map(|i| json!({ "type": "image_url", "image_url": { "url": i.data_url() } })),
    );
    json!({ "role": "user", "content": content })
}

/// Mensajes "tool" con los resultados. Como OpenAI no admite imágenes dentro
/// de un mensaje "tool", las capturas autorizadas van en un mensaje de
/// usuario inmediatamente después.
pub fn tool_results_messages(results: &[ToolResult]) -> Vec<Value> {
    let mut out: Vec<Value> = results
        .iter()
        .map(|r| {
            let content = if r.is_error { format!("ERROR: {}", r.text) } else { r.text.clone() };
            json!({ "role": "tool", "tool_call_id": r.id, "content": content })
        })
        .collect();
    let images: Vec<ImageData> = results.iter().flat_map(|r| r.images.clone()).collect();
    if !images.is_empty() {
        out.push(user_with_images(
            "(Imagen entregada por la herramienta anterior, autorizada por el usuario.)",
            &images,
        ));
    }
    out
}

#[derive(Default, Clone)]
struct PartialCall {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Default)]
pub struct OpenAiStream {
    model: Option<String>,
    finish_reason: Option<String>,
    started: bool,
    text: String,
    calls: Vec<PartialCall>,
}

impl OpenAiStream {
    pub fn new() -> Self {
        Self::default()
    }

    fn outcome(&self) -> TurnOutcome {
        let notice = match self.finish_reason.as_deref() {
            Some("length") => Some("La respuesta se cortó por alcanzar el límite de longitud.".to_string()),
            Some("content_filter") => Some("OpenAI filtró parte de la respuesta por sus políticas de contenido.".to_string()),
            _ => None,
        };
        let calls: Vec<&PartialCall> = self.calls.iter().filter(|c| !c.name.is_empty()).collect();
        let tool_calls = calls
            .iter()
            .map(|c| ToolCall {
                id: c.id.clone(),
                name: c.name.clone(),
                input: match serde_json::from_str::<Value>(if c.arguments.trim().is_empty() { "{}" } else { &c.arguments }) {
                    Ok(v) if v.is_object() => Ok(v),
                    _ => Err("INVALID_JSON: los argumentos no son un JSON válido. Vuelve a intentarlo.".into()),
                },
            })
            .collect();
        let mut assistant =
            json!({ "role": "assistant", "content": if self.text.is_empty() { Value::Null } else { Value::String(self.text.clone()) } });
        if !calls.is_empty() {
            assistant["tool_calls"] = Value::Array(
                calls
                    .iter()
                    .map(|c| json!({ "id": c.id, "type": "function", "function": { "name": c.name, "arguments": c.arguments } }))
                    .collect(),
            );
        }
        let empty = self.text.is_empty() && calls.is_empty();
        TurnOutcome {
            assistant: (!empty).then_some(assistant),
            tool_calls,
            stop_reason: self.finish_reason.clone(),
            model: self.model.clone(),
            notice,
        }
    }
}

impl ProviderStream for OpenAiStream {
    fn on_event(&mut self, ev: &SseEvent) -> Vec<ParseOut> {
        if ev.data.trim() == "[DONE]" {
            return vec![ParseOut::Finished(self.outcome())];
        }
        let Ok(data) = serde_json::from_str::<Value>(&ev.data) else {
            return vec![];
        };
        if let Some(msg) = data["error"]["message"].as_str() {
            return vec![ParseOut::Failed(format!("Error de OpenAI: {msg}"))];
        }
        let mut out = Vec::new();
        if !self.started {
            if let Some(m) = data["model"].as_str() {
                self.started = true;
                self.model = Some(m.to_string());
                out.push(ParseOut::Event(StreamEvent::Started { model: m.to_string() }));
            }
        }
        let choice = &data["choices"][0];
        if let Some(t) = choice["delta"]["content"].as_str() {
            if !t.is_empty() {
                self.text.push_str(t);
                out.push(ParseOut::Event(StreamEvent::Delta { text: t.to_string() }));
            }
        }
        if let Some(calls) = choice["delta"]["tool_calls"].as_array() {
            for c in calls {
                let i = c["index"].as_u64().unwrap_or(0) as usize;
                while self.calls.len() <= i {
                    self.calls.push(PartialCall::default());
                }
                let p = &mut self.calls[i];
                if let Some(id) = c["id"].as_str() {
                    p.id = id.to_string();
                }
                if let Some(n) = c["function"]["name"].as_str() {
                    p.name.push_str(n);
                }
                if let Some(a) = c["function"]["arguments"].as_str() {
                    p.arguments.push_str(a);
                }
            }
        }
        if let Some(r) = choice["finish_reason"].as_str() {
            self.finish_reason = Some(r.to_string());
        }
        out
    }

    fn finish(&mut self) -> Option<TurnOutcome> {
        // Algunos servidores cierran sin enviar [DONE] tras finish_reason.
        self.finish_reason.is_some().then(|| self.outcome())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::sse::SseParser;

    fn run(raw: &str) -> (Vec<StreamEvent>, Option<TurnOutcome>) {
        let mut p = SseParser::new();
        let mut s = OpenAiStream::new();
        let mut evs = vec![];
        for e in p.push(raw.as_bytes()) {
            for o in s.on_event(&e) {
                match o {
                    ParseOut::Event(e) => evs.push(e),
                    ParseOut::Finished(o) => return (evs, Some(o)),
                    ParseOut::Failed(m) => panic!("{m}"),
                }
            }
        }
        (evs, None)
    }

    #[test]
    fn body_includes_system_history_and_tools() {
        let msgs = native_messages(&[
            ChatMessage {
                role: Role::User,
                text: "hola".into(),
                provider: None,
                transcript: None,
                images: vec![],
            },
            ChatMessage {
                role: Role::Assistant,
                text: "¡guau!".into(),
                provider: Some(ProviderId::Openai),
                transcript: None,
                images: vec![],
            },
        ]);
        let tools = vec![ToolDef {
            name: "crear_tarea",
            description: "d",
            schema: json!({"type":"object"}),
        }];
        let b = build_body("gpt-5", "Eres Canela", &msgs, &tools);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][2]["content"], "¡guau!");
        assert_eq!(b["tools"][0]["function"]["name"], "crear_tarea");
    }

    #[test]
    fn parses_text_stream() {
        let raw = concat!(
            "data: {\"model\":\"gpt-5\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"\"}}]}\n\n",
            "data: {\"model\":\"gpt-5\",\"choices\":[{\"delta\":{\"content\":\"Hola\"}}]}\n\n",
            "data: {\"model\":\"gpt-5\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let (evs, o) = run(raw);
        assert!(matches!(&evs[0], StreamEvent::Started { model } if model == "gpt-5"));
        let o = o.unwrap();
        assert_eq!(o.stop_reason.as_deref(), Some("stop"));
        assert_eq!(o.assistant.unwrap()["content"], "Hola");
    }

    #[test]
    fn parses_streamed_tool_calls() {
        let raw = concat!(
            "data: {\"model\":\"gpt-5\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"crear_tarea\",\"arguments\":\"{\\\"titulo\\\":\"}}]}}]}\n\n",
            "data: {\"model\":\"gpt-5\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\" \\\"Estudiar\\\"}\"}}]}}]}\n\n",
            "data: {\"model\":\"gpt-5\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let o = run(raw).1.unwrap();
        assert_eq!(o.tool_calls.len(), 1);
        assert_eq!(o.tool_calls[0].input.as_ref().unwrap()["titulo"], "Estudiar");
        let a = o.assistant.unwrap();
        assert_eq!(a["tool_calls"][0]["id"], "call_1");
        assert!(a["content"].is_null());
        let r = tool_results_messages(&[ToolResult {
            id: "call_1".into(),
            text: "ok".into(),
            is_error: false,
            images: vec![],
        }]);
        assert_eq!(r[0]["role"], "tool");
        let img = ImageData {
            media_type: "image/jpeg".into(),
            data: "AAAA".into(),
            width: 1,
            height: 1,
        };
        let r = tool_results_messages(&[ToolResult {
            id: "call_2".into(),
            text: "captura".into(),
            is_error: false,
            images: vec![img],
        }]);
        assert_eq!(r.len(), 2);
        assert_eq!(r[1]["role"], "user");
        assert_eq!(r[1]["content"][1]["image_url"]["url"], "data:image/jpeg;base64,AAAA");
    }
}
