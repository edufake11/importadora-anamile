//! Comandos invocables desde la interfaz (`invoke(...)`).

use crate::ai::agent::{self, AgentEnv, Confirmations};
use crate::ai::{self, ChatMessage, Role, StreamEvent};
use crate::db::Db;
use crate::jobs::{JobInfo, Jobs};
use crate::settings::{self, ProviderId, Settings};
use crate::tools::files::{display, Scope};
use crate::tools::{LlmAccess, ToolCtx};
use crate::{secrets, windows};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::async_runtime::JoinHandle;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, State};

/// Límite de tamaño de la conversación enviada (caracteres).
const MAX_CONVERSATION_CHARS: usize = 1_500_000;

/// Respuestas en curso: tarea + canal hacia la interfaz (para poder cancelarlas).
type ActiveChats = HashMap<String, (JoinHandle<()>, Channel<StreamEvent>)>;

pub struct AppState {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub settings: Mutex<Settings>,
    pub chats: Mutex<ActiveChats>,
    pub http: reqwest::Client,
    pub db: Arc<Db>,
    pub jobs: Arc<Jobs>,
    pub confirms: Arc<Confirmations>,
    /// Documentos, Descargas y Escritorio del usuario (alias y carpetas por defecto).
    pub known_dirs: Vec<(String, PathBuf)>,
    pub voice: crate::voice::VoiceRuntime,
}

impl AppState {
    pub fn new(
        config_dir: PathBuf,
        data_dir: PathBuf,
        db: Arc<Db>,
        jobs: Arc<Jobs>,
        known_dirs: Vec<(String, PathBuf)>,
    ) -> Self {
        let settings = settings::load(&config_dir);
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .user_agent(concat!("canela-ai/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("cliente HTTP");
        Self {
            config_dir,
            data_dir,
            settings: Mutex::new(settings),
            chats: Mutex::new(HashMap::new()),
            http,
            db,
            jobs,
            confirms: Arc::new(Confirmations::default()),
            known_dirs,
            voice: Default::default(),
        }
    }

    pub fn current_settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    /// Carpetas autorizadas efectivas.
    pub fn allowed_folders(&self) -> Vec<PathBuf> {
        match self.current_settings().allowed_folders {
            Some(list) => list.into_iter().map(PathBuf::from).collect(),
            None => self.known_dirs.iter().map(|(_, p)| p.clone()).collect(),
        }
    }

    pub fn scope(&self) -> Scope {
        Scope::new(self.allowed_folders(), self.known_dirs.clone())
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
    let before = state.current_settings();
    if clean.launch_at_startup != before.launch_at_startup {
        use tauri_plugin_autostart::ManagerExt;
        let launcher = app.autolaunch();
        let r = if clean.launch_at_startup {
            launcher.enable()
        } else {
            launcher.disable()
        };
        r.map_err(|e| format!("No pude cambiar el inicio automático con Windows: {e}"))?;
    }
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
    #[cfg(debug_assertions)]
    if std::env::var("CANELA_DEV_API_KEY").is_ok() {
        return KeyStatus {
            anthropic: true,
            openai: true,
        };
    }
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

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn chat_send(
    app: AppHandle,
    state: State<'_, AppState>,
    request_id: String,
    messages: Vec<ChatMessage>,
    user_name: Option<String>,
    context: Option<String>,
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
    // Sólo en desarrollo: clave simulada para probar con un servidor local.
    #[cfg(debug_assertions)]
    let dev_key = std::env::var("CANELA_DEV_API_KEY").ok();
    #[cfg(not(debug_assertions))]
    let dev_key: Option<String> = None;
    let key = match dev_key {
        Some(k) => k,
        None => tauri::async_runtime::spawn_blocking(move || secrets::get_key(provider))
            .await
            .map_err(|e| e.to_string())??
            .ok_or_else(|| {
                let name = if provider == ProviderId::Anthropic {
                    "Claude (Anthropic)"
                } else {
                    "ChatGPT (OpenAI)"
                };
                format!("Falta la clave API de {name}. Agrégala en Ajustes.")
            })?,
    };

    let user_name = user_name.unwrap_or_else(|| settings.user_name.clone());
    let system = ai::system_prompt(&user_name, context.as_deref().unwrap_or(""), true);
    let notify_app = app.clone();
    let tools = ToolCtx {
        db: state.db.clone(),
        settings: settings.clone(),
        scope: state.scope(),
        backups_dir: state.data_dir.join("copias"),
        jobs: Some(state.jobs.clone()),
        llm: Some(LlmAccess {
            http: state.http.clone(),
            settings: settings.clone(),
            api_key: key.clone(),
        }),
        notify: Arc::new(move |kind| crate::data_cmds::notify(&notify_app, kind)),
    };
    let env = AgentEnv {
        http: state.http.clone(),
        settings,
        api_key: key,
        system,
        history: messages,
        tools: Some(Arc::new(tools)),
        confirms: state.confirms.clone(),
    };

    let channel = on_event.clone();
    let handle = tauri::async_runtime::spawn(async move {
        agent::run(
            env,
            Arc::new(move |ev| {
                let _ = channel.send(ev);
            }),
        )
        .await;
    });
    let mut chats = state.chats.lock().unwrap();
    chats.retain(|_, (h, _)| !h.inner().is_finished());
    chats.insert(request_id, (handle, on_event));
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

/// Respuesta del usuario a una tarjeta de permiso.
#[tauri::command]
pub fn confirm_action(state: State<'_, AppState>, confirm_id: String, approved: bool) -> bool {
    state.confirms.resolve(&confirm_id, approved)
}

// ------------------------------ Trabajos -----------------------------------

#[tauri::command]
pub fn jobs_list(state: State<'_, AppState>) -> Vec<JobInfo> {
    state.jobs.list()
}

#[tauri::command]
pub fn job_cancel(state: State<'_, AppState>, id: String) -> bool {
    state.jobs.cancel(&id)
}

// ------------------------------ Carpetas -----------------------------------

#[tauri::command]
pub fn allowed_folders(state: State<'_, AppState>) -> Vec<String> {
    state.allowed_folders().iter().map(|p| display(p)).collect()
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
