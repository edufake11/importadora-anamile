//! Capa de IA desacoplada del proveedor.
//!
//! Para añadir un proveedor nuevo (Gemini, un modelo local, etc.):
//! 1. Crea `ai/<proveedor>.rs` con `build_request(...)` y un tipo que
//!    implemente [`ProviderStream`] (traduce eventos SSE a [`StreamEvent`]).
//! 2. Añade la variante en `settings::ProviderId` y en `start_chat()`.
//!
//! La interfaz no cambia: siempre recibe el mismo flujo de `StreamEvent`.

pub mod anthropic;
pub mod openai;
pub mod sse;
#[cfg(test)]
mod stream_test;

use crate::settings::{ProviderId, Settings};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

/// Un mensaje del historial tal como lo guarda la interfaz.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: Role,
    pub text: String,
    /// Bloques originales de Claude (razonamiento firmado incluido) para
    /// reenviarlos tal cual. `None` para mensajes del usuario u OpenAI.
    #[serde(default)]
    pub anthropic_content: Option<Value>,
}

/// Eventos que viajan de Rust a la interfaz por un `tauri::ipc::Channel`.
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum StreamEvent {
    Started {
        model: String,
    },
    /// El modelo está razonando (no hay texto visible todavía).
    Thinking,
    Delta {
        text: String,
    },
    Done {
        stop_reason: Option<String>,
        model: Option<String>,
        anthropic_content: Option<Value>,
        notice: Option<String>,
    },
    Error {
        message: String,
    },
    Cancelled,
}

impl StreamEvent {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            StreamEvent::Done { .. } | StreamEvent::Error { .. } | StreamEvent::Cancelled
        )
    }
}

/// Traduce eventos SSE de un proveedor concreto a `StreamEvent`.
pub trait ProviderStream: Send {
    fn on_event(&mut self, ev: &sse::SseEvent) -> Vec<StreamEvent>;
    /// Llamado si la conexión termina sin evento final.
    fn finish(&mut self) -> Option<StreamEvent>;
}

/// Personalidad de Canela. Debe ser estable durante una conversación
/// (cambiarla invalida la caché y el razonamiento preservado de Claude).
pub fn system_prompt(user_name: &str) -> String {
    let who = if user_name.trim().is_empty() {
        "tu humano".to_string()
    } else {
        user_name.trim().to_string()
    };
    format!(
        "Eres Canela, una perrita salchicha color chocolate con audífonos violetas y un pañuelo morado, \
que vive en el escritorio de Windows de {who} como mascota virtual y asistente personal.\n\n\
Personalidad: cálida, curiosa, leal y con buen humor. De vez en cuando sueltas un guiño perruno breve \
(\"¡guau!\", mover la colita), pero sin exagerar ni interrumpir las respuestas técnicas.\n\n\
Ayudas con: programación (explicar código y errores, proponer soluciones paso a paso), análisis de textos \
y documentos que te peguen o adjunten, organización de tareas y proyectos, y preguntas generales.\n\n\
Reglas:\n\
- Responde en español salvo que te escriban en otro idioma.\n\
- Sé concisa por defecto; extiéndete cuando la tarea lo requiera. Usa bloques ``` para código.\n\
- En esta versión NO puedes ejecutar acciones en la computadora (abrir programas, mover archivos), \
NI ver la pantalla, NI usar el micrófono, NI recordar conversaciones anteriores a esta. \
Si te lo piden, dilo con honestidad y da instrucciones para hacerlo manualmente. \
Nunca finjas haber realizado una acción.\n\
- Si no sabes algo, dilo."
    )
}

/// Convierte una respuesta HTTP de error en un mensaje claro en español.
pub fn http_error_message(provider: ProviderId, status: u16, body: &str, model: &str) -> String {
    let name = match provider {
        ProviderId::Anthropic => "Claude",
        ProviderId::Openai => "OpenAI",
    };
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .unwrap_or_default();
    let base = match status {
        400 => format!("{name} rechazó la solicitud (400)."),
        401 => format!("La clave API de {name} no es válida o fue revocada. Revísala en Ajustes."),
        403 => format!("Tu clave de {name} no tiene permiso para esta operación (403)."),
        404 => format!("El modelo «{model}» no existe o tu cuenta no tiene acceso. Cámbialo en Ajustes."),
        413 => "La conversación es demasiado grande. Empieza una nueva o adjunta archivos más pequeños.".to_string(),
        429 => format!("Límite de uso de {name} alcanzado (429). Espera un momento o revisa tu plan/saldo."),
        500..=599 => format!("{name} tiene problemas temporales ({status}). Inténtalo de nuevo en unos segundos."),
        _ => format!("{name} respondió con un error HTTP {status}."),
    };
    if detail.is_empty() {
        base
    } else {
        format!("{base}\nDetalle: {detail}")
    }
}

/// Construye la petición HTTP y el traductor de eventos según el proveedor.
pub fn start_chat(
    http: &reqwest::Client,
    settings: &Settings,
    api_key: &str,
    user_name: &str,
    messages: &[ChatMessage],
) -> (reqwest::RequestBuilder, Box<dyn ProviderStream>, String) {
    let system = system_prompt(user_name);
    match settings.provider {
        ProviderId::Anthropic => (
            anthropic::build_request(
                http,
                api_key,
                &settings.anthropic_model,
                &settings.anthropic_effort,
                &system,
                messages,
            ),
            Box::new(anthropic::AnthropicStream::new()),
            settings.anthropic_model.clone(),
        ),
        ProviderId::Openai => (
            openai::build_request(http, api_key, &settings.openai_model, &system, messages),
            Box::new(openai::OpenAiStream::new()),
            settings.openai_model.clone(),
        ),
    }
}

/// Ejecuta la petición y reenvía cada evento mediante `emit` hasta un evento final.
pub async fn run_stream(
    provider: ProviderId,
    model: String,
    request: reqwest::RequestBuilder,
    mut parser: Box<dyn ProviderStream>,
    emit: impl Fn(StreamEvent),
) {
    let response = match request.send().await {
        Ok(r) => r,
        Err(e) => {
            let message = if e.is_timeout() {
                "La solicitud tardó demasiado. Revisa tu conexión.".to_string()
            } else if e.is_connect() {
                "No hay conexión con el servicio de IA. Revisa tu internet o firewall.".to_string()
            } else {
                format!("Error de red: {e}")
            };
            emit(StreamEvent::Error { message });
            return;
        }
    };

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        emit(StreamEvent::Error {
            message: http_error_message(provider, status.as_u16(), &body, &model),
        });
        return;
    }

    let mut sse = sse::SseParser::new();
    let mut bytes = response.bytes_stream();
    while let Some(chunk) = bytes.next().await {
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                emit(StreamEvent::Error {
                    message: format!("Se interrumpió la conexión: {e}"),
                });
                return;
            }
        };
        for ev in sse.push(&chunk) {
            for out in parser.on_event(&ev) {
                let terminal = out.is_terminal();
                emit(out);
                if terminal {
                    return;
                }
            }
        }
    }
    emit(parser.finish().unwrap_or(StreamEvent::Error {
        message: "La conexión terminó antes de completar la respuesta.".into(),
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_mentions_user_and_limits() {
        let p = system_prompt("Eduardo");
        assert!(p.contains("Eduardo"));
        assert!(p.contains("NO puedes ejecutar acciones"));
        assert!(system_prompt("").contains("tu humano"));
    }

    #[test]
    fn http_errors_are_friendly() {
        let m = http_error_message(
            ProviderId::Anthropic,
            401,
            r#"{"error":{"message":"invalid x-api-key"}}"#,
            "m",
        );
        assert!(m.contains("no es válida"));
        assert!(m.contains("invalid x-api-key"));
        let m = http_error_message(ProviderId::Openai, 404, "", "gpt-x");
        assert!(m.contains("gpt-x"));
    }

    #[test]
    fn stream_event_serializes_camel_case() {
        let v = serde_json::to_value(StreamEvent::Done {
            stop_reason: Some("end_turn".into()),
            model: None,
            anthropic_content: None,
            notice: None,
        })
        .unwrap();
        assert_eq!(v["type"], "done");
        assert_eq!(v["stopReason"], "end_turn");
    }
}
