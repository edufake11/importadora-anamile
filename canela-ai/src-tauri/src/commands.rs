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
    /// Capturas pendientes de revisar (sólo en memoria).
    pub captures: Mutex<HashMap<String, image::RgbaImage>>,
}

impl AppState {
    pub fn new(config_dir: PathBuf, data_dir: PathBuf, db: Arc<Db>, jobs: Arc<Jobs>, known_dirs: Vec<(String, PathBuf)>) -> Self {
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
            captures: Mutex::new(HashMap::new()),
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
pub fn save_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> Result<Settings, String> {
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
    // «Hola, Canela»: encender o apagar la escucha local según el ajuste.
    if clean.wake_word && clean.voice_consent && !crate::voice::wake::is_running() {
        let _ = crate::voice::wake_start_if_enabled(&app);
    } else if !(clean.wake_word && clean.voice_consent) && crate::voice::wake::is_running() {
        crate::voice::wake::stop();
        let _ = app.emit("wake-state", serde_json::json!({ "active": false }));
    }
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
    // v0.6: modelo elegido para ESTE mensaje (p. ej. aceptaste usar Claude
    // para una tarea compleja). Se valida aquí: presupuesto y privacidad.
    provider_override: Option<ProviderId>,
    // v0.6.1: tipos de datos personales que autorizaste enviar fuera en ESTE
    // mensaje (correo, teléfono, imagen…). Los secretos nunca se autorizan.
    privacy_ack: Option<Vec<String>>,
    on_event: Channel<StreamEvent>,
) -> Result<(), String> {
    if messages.last().map(|m| m.role) != Some(Role::User) {
        return Err("El último mensaje debe ser del usuario.".into());
    }
    let total: usize = messages.iter().map(|m| m.text.len()).sum();
    if total > MAX_CONVERSATION_CHARS {
        return Err("La conversación es demasiado larga. Empieza una nueva.".into());
    }

    let mut settings = state.current_settings();
    let spent = state.db.spend_month(&crate::router::month_key()).unwrap_or(0.0);
    let last = messages.last().cloned().unwrap();
    // v0.7.1: ¿un agente, un plan entre varios o una aclaración? (sin imágenes)
    let analysis = if last.images.is_empty() {
        crate::coordination::analyze(&last.text)
    } else {
        crate::coordination::Analysis::Single
    };
    let switching = provider_override.is_some_and(|p| p != settings.provider);
    if let Some(p) = provider_override {
        settings.provider = p;
    }
    let provider = settings.provider;
    // Control final de privacidad y presupuesto para servicios externos.
    let mut reservation: Option<(String, f64)> = None;
    if provider.is_cloud() {
        // Si se cambia de modelo, el historial también viaja (por primera vez).
        let outgoing = crate::privacy::Outgoing {
            text: last.text.clone(),
            attachments: vec![],
            history: if switching {
                messages[..messages.len() - 1].iter().map(|m| m.text.clone()).collect()
            } else {
                vec![]
            },
            images: if switching {
                messages.iter().map(|m| m.images.len()).sum()
            } else {
                last.images.len()
            },
        };
        let reserved = crate::router::RESERVATIONS.total();
        crate::router::check_cloud_use(
            provider,
            &outgoing,
            privacy_ack.as_deref().unwrap_or(&[]),
            spent + reserved,
            settings.monthly_budget_usd,
        )?;
        // Reserva el costo ESTIMADO mientras dure la respuesta.
        let req = crate::router::Request {
            text: last.text.clone(),
            history_chars: messages.iter().map(|m| m.text.len()).sum::<usize>(),
            ..Default::default()
        };
        let (level, _) = crate::router::complexity(&req);
        // Un plan hace una respuesta por paso: se reserva el costo de todos.
        let est = crate::router::estimate_cost(provider, &ai::model_of(&settings), &req, level).unwrap_or(0.01)
            * match &analysis {
                crate::coordination::Analysis::Plan(p) => p.steps.len() as f64,
                // Consulta de planificación + pasos (estimación prudente).
                crate::coordination::Analysis::ModelPlan => 3.0,
                _ => 1.0,
            };
        crate::router::RESERVATIONS.reserve(&request_id, est, spent, settings.monthly_budget_usd)?;
        reservation = Some((request_id.clone(), est));
    }
    let max_cost_usd = reservation
        .as_ref()
        .map(|_| (settings.monthly_budget_usd - spent - (crate::router::RESERVATIONS.total() - reservation.as_ref().unwrap().1)).max(0.0));
    let model_name = ai::model_of(&settings);
    // Sólo en desarrollo: clave simulada para probar con un servidor local.
    #[cfg(debug_assertions)]
    let dev_key = std::env::var("CANELA_DEV_API_KEY").ok();
    #[cfg(not(debug_assertions))]
    let dev_key: Option<String> = None;
    let key = match dev_key {
        // Ollama no usa clave.
        _ if !provider.needs_key() => String::new(),
        Some(k) => k,
        None => tauri::async_runtime::spawn_blocking(move || secrets::get_key(provider))
            .await
            .map_err(|e| e.to_string())??
            .ok_or_else(|| {
                let name = match provider {
                    ProviderId::Anthropic => "Claude (Anthropic)",
                    _ => "ChatGPT (OpenAI)",
                };
                format!("Falta la clave API de {name}. Agrégala en Ajustes.")
            })?,
    };

    let user_name = user_name.unwrap_or_else(|| settings.user_name.clone());
    // v0.7: el orquestador elige el agente (perfil) para este mensaje.
    let base_system = ai::system_prompt(&user_name, context.as_deref().unwrap_or(""), true);
    let mut system = base_system.clone();
    // v0.7: el orquestador elige el agente (perfil) para este mensaje; si la
    // petición necesita varios, el coordinador (v0.7.1) los reparte.
    let profile = if matches!(
        analysis,
        crate::coordination::Analysis::Plan(_) | crate::coordination::Analysis::Clarify(_)
    ) {
        None
    } else {
        let previous_agent = messages.iter().rev().find(|m| m.role == Role::Assistant).and_then(|m| m.agent);
        let selection = crate::agents::select(&last.text, previous_agent);
        let profile = crate::agents::profile(selection.agent);
        if !profile.prompt.is_empty() {
            system.push_str("\n\n");
            system.push_str(profile.prompt);
        }
        let _ = on_event.send(StreamEvent::Agent {
            id: selection.agent,
            name: selection.name.clone(),
            reason: selection.reason.clone(),
        });
        Some(profile)
    };
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
            db: Some(state.db.clone()),
        }),
        notify: Arc::new(move |kind| crate::data_cmds::notify(&notify_app, kind)),
        screen: Some(Arc::new(crate::vision_cmds::AppCapturer { app: app.clone() })),
    };
    let env = AgentEnv {
        http: state.http.clone(),
        settings,
        api_key: key,
        system,
        history: messages,
        tools: Some(Arc::new(tools)),
        confirms: state.confirms.clone(),
        max_cost_usd,
        agent: profile,
        goal: None,
    };

    let channel = on_event.clone();
    let spend_db = state.db.clone();
    // Al terminar (bien, con error o cancelada) se libera la reserva; si la
    // respuesta se cortó sin datos de consumo, se apunta la estimación.
    let guard = SpendGuard {
        reservation,
        db: state.db.clone(),
        provider,
        model: model_name,
        started: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        recorded: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    let (started, recorded) = (guard.started.clone(), guard.recorded.clone());
    let handle = tauri::async_runtime::spawn(async move {
        let _guard = guard;
        let emit: agent::Emit = Arc::new(move |ev| {
            use std::sync::atomic::Ordering;
            if matches!(ev, StreamEvent::Started { .. }) {
                started.store(true, Ordering::SeqCst);
            }
            // Gasto con los tokens REALES que informó el proveedor.
            if let StreamEvent::Done {
                provider,
                cost_usd: Some(c),
                usage,
                model,
                ..
            } = &ev
            {
                if provider.is_cloud() && *c > 0.0 {
                    let _ = spend_db.spend_add(
                        &crate::router::month_key(),
                        provider.as_str(),
                        model.as_deref().unwrap_or("?"),
                        usage.input + usage.cache_read + usage.cache_write,
                        usage.output,
                        *c,
                        "api",
                    );
                    recorded.store(true, Ordering::SeqCst);
                }
            }
            let _ = channel.send(ev);
        });
        crate::coordination::respond(env, base_system, analysis, emit).await;
    });
    let mut chats = state.chats.lock().unwrap();
    chats.retain(|_, (h, _)| !h.inner().is_finished());
    chats.insert(request_id, (handle, on_event));
    Ok(())
}

/// Libera la reserva de presupuesto al terminar una respuesta. Si el modelo
/// externo empezó a responder pero no llegaron datos de consumo (cancelada o
/// cortada), apunta el costo estimado como «estimado» para no subestimar.
struct SpendGuard {
    reservation: Option<(String, f64)>,
    db: Arc<crate::db::Db>,
    provider: ProviderId,
    model: String,
    started: Arc<std::sync::atomic::AtomicBool>,
    recorded: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for SpendGuard {
    fn drop(&mut self) {
        use std::sync::atomic::Ordering;
        if let Some((id, est)) = self.reservation.take() {
            crate::router::RESERVATIONS.release(&id);
            if self.started.load(Ordering::SeqCst) && !self.recorded.load(Ordering::SeqCst) {
                let _ = self.db.spend_add(
                    &crate::router::month_key(),
                    self.provider.as_str(),
                    &self.model,
                    0,
                    0,
                    est,
                    "estimado",
                );
            }
        }
    }
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
