//! Proveedor OpenAI (ChatGPT) — Chat Completions API con streaming SSE.

use super::{ChatMessage, ProviderStream, Role, StreamEvent};
use crate::ai::sse::SseEvent;
use serde_json::{json, Value};

pub const API_URL: &str = "https://api.openai.com/v1/chat/completions";

pub fn build_body(model: &str, system: &str, messages: &[ChatMessage]) -> Value {
    let mut msgs = vec![json!({ "role": "system", "content": system })];
    for m in messages {
        if m.text.trim().is_empty() {
            continue;
        }
        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        msgs.push(json!({ "role": role, "content": m.text }));
    }
    json!({ "model": model, "stream": true, "messages": msgs })
}

pub fn build_request(
    http: &reqwest::Client,
    api_key: &str,
    model: &str,
    system: &str,
    messages: &[ChatMessage],
) -> reqwest::RequestBuilder {
    http.post(API_URL)
        .bearer_auth(api_key)
        .json(&build_body(model, system, messages))
}

#[derive(Default)]
pub struct OpenAiStream {
    model: Option<String>,
    finish_reason: Option<String>,
    started: bool,
}

impl OpenAiStream {
    pub fn new() -> Self {
        Self::default()
    }

    fn done(&self) -> StreamEvent {
        let notice = match self.finish_reason.as_deref() {
            Some("length") => {
                Some("La respuesta se cortó por alcanzar el límite de longitud.".to_string())
            }
            Some("content_filter") => Some(
                "OpenAI filtró parte de la respuesta por sus políticas de contenido.".to_string(),
            ),
            _ => None,
        };
        StreamEvent::Done {
            stop_reason: self.finish_reason.clone(),
            model: self.model.clone(),
            anthropic_content: None,
            notice,
        }
    }
}

impl ProviderStream for OpenAiStream {
    fn on_event(&mut self, ev: &SseEvent) -> Vec<StreamEvent> {
        if ev.data.trim() == "[DONE]" {
            return vec![self.done()];
        }
        let Ok(data) = serde_json::from_str::<Value>(&ev.data) else {
            return vec![];
        };
        if let Some(msg) = data["error"]["message"].as_str() {
            return vec![StreamEvent::Error {
                message: format!("Error de OpenAI: {msg}"),
            }];
        }
        let mut out = Vec::new();
        if !self.started {
            if let Some(m) = data["model"].as_str() {
                self.started = true;
                self.model = Some(m.to_string());
                out.push(StreamEvent::Started {
                    model: m.to_string(),
                });
            }
        }
        let choice = &data["choices"][0];
        if let Some(t) = choice["delta"]["content"].as_str() {
            if !t.is_empty() {
                out.push(StreamEvent::Delta {
                    text: t.to_string(),
                });
            }
        }
        if let Some(r) = choice["finish_reason"].as_str() {
            self.finish_reason = Some(r.to_string());
        }
        out
    }

    fn finish(&mut self) -> Option<StreamEvent> {
        // Algunos servidores cierran sin enviar [DONE] tras finish_reason.
        self.finish_reason.is_some().then(|| self.done())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::sse::SseParser;

    #[test]
    fn body_includes_system_and_history() {
        let msgs = vec![
            ChatMessage {
                role: Role::User,
                text: "hola".into(),
                anthropic_content: None,
            },
            ChatMessage {
                role: Role::Assistant,
                text: "¡guau!".into(),
                anthropic_content: None,
            },
        ];
        let b = build_body("gpt-5", "Eres Canela", &msgs);
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["messages"][2]["content"], "¡guau!");
        assert_eq!(b["stream"], true);
    }

    #[test]
    fn parses_stream() {
        let raw = concat!(
            "data: {\"model\":\"gpt-5\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"\"}}]}\n\n",
            "data: {\"model\":\"gpt-5\",\"choices\":[{\"delta\":{\"content\":\"Hola\"}}]}\n\n",
            "data: {\"model\":\"gpt-5\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: [DONE]\n\n"
        );
        let mut p = SseParser::new();
        let mut s = OpenAiStream::new();
        let evs: Vec<StreamEvent> = p
            .push(raw.as_bytes())
            .iter()
            .flat_map(|e| s.on_event(e))
            .collect();
        assert!(matches!(&evs[0], StreamEvent::Started { model } if model == "gpt-5"));
        assert!(matches!(&evs[1], StreamEvent::Delta { text } if text == "Hola"));
        assert!(
            matches!(evs.last().unwrap(), StreamEvent::Done { stop_reason: Some(r), .. } if r == "stop")
        );
    }
}
