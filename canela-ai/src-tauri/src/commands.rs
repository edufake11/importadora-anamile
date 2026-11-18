//! Comandos invocables desde la interfaz (`invoke(...)`).

use crate::ai::{self, ChatMessage, Role, StreamEvent};
use crate::settings::{self, ProviderId, Settings};
use crate::{secrets, windows};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::async_runtime::JoinHandle;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};

/// Límite de tamaño de la conversación enviada (caracteres).
const MAX_CONVERSATION_CHARS: usize = 1_500_000;

/// Respuestas en curso: tarea + canal hacia la interfaz (para poder cancelarlas).
type ActiveChats = HashMap<String, (JoinHandle<()>, Channel<StreamEvent>)>;

pub struct AppState {
    pub config_dir: PathBuf,
    pub settings: Mutex<Settings>,
    pub chats: Mutex<ActiveChats>,
    pub http: reqwest::Client,
}

impl AppState {
    pub fn new(config_dir: PathBuf) -> Self {
        let settings = settings::load(&config_dir);
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .user_agent(concat!("canela-ai/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("cliente HTTP");
        Self {
            config_dir,
            settings: Mutex::new(settings),
            chats: Mutex::new(HashMap::new()),
            http,
        }
    }

    pub fn current_settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }
}

// ----------------------------- Ajustes -------------------------------------

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.current_settings()
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, String> {
    let clean = settings.sanitized();
    settings::save(&state.config_dir, &clean)?;
    *state.settings.lock().unwrap() = clean.clone();
    let _ = app.emit("settings-changed", &clean);
    Ok(clean)
}

#[derive(Serialize)]
pub struct KeyStatus {
    anthropic: bool,
    openai: bool,
}

#[tauri::command]
pub async fn api_key_status() -> KeyStatus {
    // El llavero puede bloquear brevemente: fuera del hilo principal.
    tauri::async_runtime::spawn_blocking(|| KeyStatus {
        anthropic: secrets::has_key(ProviderId::Anthropic),
        openai: secrets::has_key(ProviderId::Openai),
    })
    .await
    .unwrap_or(KeyStatus {
        anthropic: false,
        openai: false,
    })
}

#[tauri::command]
pub async fn set_api_key(provider: ProviderId, key: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || secrets::set_key(provider, &key))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_api_key(provider: ProviderId) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || secrets::delete_key(provider))
        .await
        .map_err(|e| e.to_string())?
}

// ------------------------------- Chat --------------------------------------

#[tauri::command]
pub async fn chat_send(
    state: State<'_, AppState>,
    request_id: String,
    messages: Vec<ChatMessage>,
    user_name: Option<String>,
    on_event: Channel<StreamEvent>,
) -> Result<(), String> {
    if messages.last().map(|m| m.role) != Some(Role::User) {
        return Err("El último mensaje debe ser del usuario.".into());
    }
    let total: usize = messages.iter().map(|m| m.text.len()).sum();
    if total > MAX_CONVERSATION_CHARS {
        return Err("La conversación es demasiado larga. Empieza una nueva.".into());
    }

    let settings = state.current_settings();
    let provider = settings.provider;
    let key = tauri::async_runtime::spawn_blocking(move || secrets::get_key(provider))
        .await
        .map_err(|e| e.to_string())??
        .ok_or_else(|| {
            let name = if provider == ProviderId::Anthropic {
                "Claude (Anthropic)"
            } else {
                "ChatGPT (OpenAI)"
            };
            format!("Falta la clave API de {name}. Agrégala en Ajustes.")
        })?;

    let user_name = user_name.unwrap_or_else(|| settings.user_name.clone());
    let (request, parser, model) =
        ai::start_chat(&state.http, &settings, &key, &user_name, &messages);
    drop(key);

    let channel = on_event.clone();
    let handle = tauri::async_runtime::spawn(async move {
        ai::run_stream(provider, model, request, parser, move |ev| {
            let _ = channel.send(ev);
        })
        .await;
    });
    state
        .chats
        .lock()
        .unwrap()
        .insert(request_id, (handle, on_event));

    // Limpieza de tareas ya terminadas.
    state
        .chats
        .lock()
        .unwrap()
        .retain(|_, (h, _)| !h.inner().is_finished());
    Ok(())
}

#[tauri::command]
pub fn chat_cancel(state: State<'_, AppState>, request_id: String) -> bool {
    if let Some((handle, channel)) = state.chats.lock().unwrap().remove(&request_id) {
        handle.abort();
        let _ = channel.send(StreamEvent::Cancelled);
        true
    } else {
        false
    }
}

// ----------------------------- Ventanas ------------------------------------

#[tauri::command]
pub fn show_panel(app: AppHandle) {
    windows::show_panel(&app);
}

#[tauri::command]
pub fn hide_panel(app: AppHandle) {
    windows::hide_panel(&app);
}

#[tauri::command]
pub fn toggle_pet(app: AppHandle) {
    windows::toggle_pet(&app);
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}
