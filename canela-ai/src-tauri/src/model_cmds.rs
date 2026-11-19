//! Información del modelo elegido (capacidades y precio) y resúmenes de
//! conversaciones largas.

use crate::ai::{self, pricing, ChatMessage, Role, StreamEvent};
use crate::commands::AppState;
use crate::secrets;
use crate::settings::ProviderId;
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tauri::State;

#[derive(Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub provider: String,
    pub model: String,
    /// None = no se pudo comprobar.
    pub exists: Option<bool>,
    pub supports_images: Option<bool>,
    pub supports_thinking: Option<bool>,
    pub supports_effort: Option<bool>,
    pub context_tokens: Option<u64>,
    pub max_output: Option<u64>,
    pub price: Option<pricing::Price>,
    /// "api" (comprobado en vivo) o "tabla" (valores conocidos).
    pub source: &'static str,
    pub note: Option<String>,
}

/// Lee la respuesta de `GET /v1/models/{id}` de Anthropic.
pub fn parse_anthropic_model(v: &Value, info: &mut ModelInfo) {
    let caps = &v["capabilities"];
    let flag = |x: &Value| x["supported"].as_bool();
    info.exists = Some(true);
    info.supports_images = flag(&caps["image_input"]).or(info.supports_images);
    info.supports_thinking = flag(&caps["thinking"]["types"]["adaptive"]).or_else(|| flag(&caps["thinking"]));
    info.supports_effort = flag(&caps["effort"]);
    info.context_tokens = v["max_input_tokens"].as_u64();
    info.max_output = v["max_tokens"].as_u64();
    info.source = "api";
}

fn key_for(provider: ProviderId) -> Option<String> {
    if !provider.needs_key() {
        return Some(String::new());
    }
    #[cfg(debug_assertions)]
    if let Ok(k) = std::env::var("CANELA_DEV_API_KEY") {
        return Some(k);
    }
    secrets::get_key(provider).ok().flatten()
}

#[tauri::command]
pub async fn model_info(state: State<'_, AppState>) -> Result<ModelInfo, String> {
    let settings = state.current_settings();
    let provider = settings.provider;
    let model = ai::model_of(&settings);
    let mut info = ModelInfo {
        provider: provider.as_str().into(),
        model: model.clone(),
        supports_images: ai::model_supports_images(provider, &model),
        supports_thinking: (provider == ProviderId::Anthropic).then(|| ai::anthropic::supports_adaptive_thinking(&model)),
        supports_effort: (provider == ProviderId::Anthropic).then(|| ai::anthropic::supports_adaptive_thinking(&model)),
        price: pricing::price(provider, &model),
        source: "tabla",
        ..Default::default()
    };
    if provider == ProviderId::Ollama {
        let st = ai::ollama::status(&state.http, &settings).await;
        info.exists = st.running.then_some(st.model_installed);
        info.source = "ollama";
        if !st.capabilities.is_empty() {
            let has = |c: &str| st.capabilities.iter().any(|x| x == c);
            info.supports_images = Some(has("vision"));
            info.supports_thinking = Some(has("thinking"));
        }
        info.context_tokens = Some(settings.ollama_num_ctx as u64);
        info.note = Some(st.message);
        return Ok(info);
    }
    let key = tauri::async_runtime::spawn_blocking(move || key_for(provider))
        .await
        .map_err(|e| e.to_string())?;
    let Some(key) = key else {
        info.note = Some("Añade la clave en Ajustes para comprobar el modelo en vivo.".into());
        return Ok(info);
    };
    let resp = match provider {
        ProviderId::Anthropic => {
            state
                .http
                .get(format!("https://api.anthropic.com/v1/models/{model}"))
                .header("x-api-key", &key)
                .header("anthropic-version", ai::anthropic::API_VERSION)
                .send()
                .await
        }
        ProviderId::Openai | ProviderId::Ollama => {
            state
                .http
                .get(format!("https://api.openai.com/v1/models/{model}"))
                .bearer_auth(&key)
                .send()
                .await
        }
    };
    match resp {
        Ok(r) if r.status().as_u16() == 404 => {
            info.exists = Some(false);
            info.note = Some(format!("El modelo «{model}» no existe o tu cuenta no tiene acceso."));
        }
        Ok(r) if r.status().is_success() => {
            let v: Value = r.json().await.unwrap_or(Value::Null);
            match provider {
                ProviderId::Anthropic => parse_anthropic_model(&v, &mut info),
                ProviderId::Openai | ProviderId::Ollama => {
                    info.exists = Some(true);
                    info.note = Some("OpenAI no publica las capacidades por modelo: se muestran valores conocidos.".into());
                }
            }
        }
        Ok(r) => {
            let status = r.status().as_u16();
            let body = r.text().await.unwrap_or_default();
            info.note = Some(ai::http_error_message(provider, status, &body, &model));
        }
        Err(e) => info.note = Some(pricing::redact_secrets(&format!("No pude comprobarlo: {e}"))),
    }
    Ok(info)
}

/// ¿Está Ollama encendido? ¿Qué modelos hay instalados? Sólo consulta este
/// equipo (127.0.0.1); nunca descarga nada.
#[tauri::command]
pub async fn ollama_status(state: State<'_, AppState>, model: Option<String>) -> Result<ai::ollama::OllamaStatus, String> {
    let mut settings = state.current_settings();
    if let Some(m) = model.map(|m| m.trim().to_string()).filter(|m| !m.is_empty()) {
        settings.ollama_model = m;
    }
    Ok(ai::ollama::status(&state.http, &settings).await)
}

/// Texto plano de la conversación para resumirla (sin bloques internos).
pub fn plain_transcript(messages: &[ChatMessage], max_chars: usize) -> String {
    let lines: Vec<String> = messages
        .iter()
        .filter(|m| !m.text.trim().is_empty())
        .map(|m| {
            let who = if m.role == Role::User { "Usuario" } else { "Canela" };
            let t: String = m.text.chars().take(4000).collect();
            format!("{who}: {t}")
        })
        .collect();
    let all = lines.join("\n\n");
    let n = all.chars().count();
    if n > max_chars {
        all.chars().skip(n - max_chars).collect()
    } else {
        all
    }
}

/// Resume la parte antigua de una conversación larga (sin herramientas).
#[tauri::command]
pub async fn summarize_conversation(state: State<'_, AppState>, messages: Vec<ChatMessage>, user_name: String) -> Result<String, String> {
    let settings = state.current_settings();
    let provider = settings.provider;
    if provider.is_cloud() {
        let spent = state.db.spend_month(&crate::router::month_key()).unwrap_or(0.0);
        if settings.monthly_budget_usd <= 0.0 || spent >= settings.monthly_budget_usd {
            return Err("Límite de gasto mensual alcanzado: no resumiré con un modelo de pago.".into());
        }
    }
    let key = tauri::async_runtime::spawn_blocking(move || key_for(provider))
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Falta la clave API.")?;
    let system = ai::system_prompt(&user_name, "", false);
    let prompt = format!(
        "Resume la siguiente conversación para poder continuarla sin perder el hilo. Incluye: temas tratados, decisiones, \
proyectos/archivos/rutas mencionados, tareas o pendientes, preferencias del usuario y la última pregunta abierta. \
Máximo 350 palabras, en español, en viñetas.\n\n{}",
        plain_transcript(&messages, 200_000)
    );
    let msgs = vec![json!({ "role": "user", "content": prompt })];
    let text = Arc::new(Mutex::new(String::new()));
    let sink = text.clone();
    let emit = move |e: StreamEvent| {
        if let StreamEvent::Delta { text } = e {
            sink.lock().unwrap().push_str(&text);
        }
    };
    let outcome = ai::complete(&state.http, &settings, &key, &system, &msgs, &[], &emit)
        .await
        .map_err(|e| pricing::redact_secrets(&e))?;
    crate::router::record_spend(&state.db, provider, &ai::model_of(&settings), &outcome.usage);
    let out = text.lock().unwrap().trim().to_string();
    if out.is_empty() {
        Err("No pude resumir la conversación.".into())
    } else {
        state.db.log("contexto", "Resumí una conversación larga");
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_models_api_capabilities() {
        let v = json!({
            "id": "claude-opus-5-5",
            "max_input_tokens": 1000000,
            "max_tokens": 128000,
            "capabilities": {
                "image_input": {"supported": true},
                "thinking": {"supported": true, "types": {"adaptive": {"supported": true}, "enabled": {"supported": false}}},
                "effort": {"supported": true}
            }
        });
        let mut info = ModelInfo::default();
        parse_anthropic_model(&v, &mut info);
        assert_eq!(info.exists, Some(true));
        assert_eq!(info.supports_images, Some(true));
        assert_eq!(info.supports_thinking, Some(true));
        assert_eq!(info.context_tokens, Some(1_000_000));
        assert_eq!(info.source, "api");
    }

    #[test]
    fn transcript_keeps_the_end() {
        let msgs: Vec<ChatMessage> = (0..10)
            .map(|i| ChatMessage {
                role: if i % 2 == 0 { Role::User } else { Role::Assistant },
                text: format!("mensaje {i}"),
                provider: None,
                transcript: None,
                images: vec![],
            })
            .collect();
        let t = plain_transcript(&msgs, 1000);
        assert!(t.starts_with("Usuario: mensaje 0"));
        assert!(t.contains("Canela: mensaje 9"));
        let short = plain_transcript(&msgs, 20);
        assert!(short.ends_with("mensaje 9"));
    }
}
