//! Gestión de recursos: modos Equilibrado / Ahorro, descarga del modelo de
//! Ollama por inactividad, botón «Liberar memoria de IA» y registro de
//! memoria en segundo plano.
//!
//! Reglas:
//! - Nunca se descarga el modelo mientras Canela responde o ejecuta
//!   herramientas (contador de actividad). Si se pide liberar memoria en ese
//!   momento, se hace en cuanto termina.
//! - Cada petición a Ollama lleva `keep_alive` (2 min en Equilibrado): si
//!   Canela se cierra, Ollama descarga el modelo solo. Mientras una respuesta
//!   larga ejecuta herramientas, Canela renueva ese plazo (sólo si el modelo
//!   sigue cargado, para no volver a cargarlo sin necesidad).
//! - El modelo se vuelve a cargar solo con el siguiente mensaje.
//! - Nunca se cierran procesos de otras aplicaciones ni se toca el archivo
//!   de paginación.

use crate::memdiag::{self, AiModelMem, Detector, MemEvent, ProcMem, Sample};
use crate::settings::{ProviderId, Settings};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ------------------------------------------------------------ modos

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModeParams {
    /// `keep_alive` que se envía a Ollama.
    pub keep_alive: &'static str,
    /// Inactividad tras la que Canela descarga el modelo.
    pub idle: Duration,
    /// Contexto máximo permitido en este modo.
    pub max_ctx: u32,
    pub allow_think: bool,
    /// Cada cuánto se registra la memoria.
    pub sample_every: Duration,
    /// Cuánto se mantiene cargado el modelo de voz tras usarlo (0 = descargar al terminar).
    pub voice_idle: Duration,
    /// Precargar el modelo de IA al abrir el micrófono.
    pub preload_on_mic: bool,
}

pub const BALANCED: ModeParams = ModeParams {
    keep_alive: "2m",
    idle: Duration::from_secs(120),
    max_ctx: crate::ai::ollama::MAX_NUM_CTX,
    allow_think: true,
    sample_every: Duration::from_secs(60),
    voice_idle: Duration::from_secs(300),
    preload_on_mic: true,
};

pub const SAVER: ModeParams = ModeParams {
    keep_alive: "30s",
    idle: Duration::from_secs(30),
    max_ctx: 8192,
    allow_think: false,
    sample_every: Duration::from_secs(120),
    voice_idle: Duration::from_secs(0),
    preload_on_mic: false,
};

pub fn mode(settings: &Settings) -> ModeParams {
    if settings.resource_mode == "saver" {
        SAVER
    } else {
        BALANCED
    }
}

// ------------------------------------------------------------ actividad

/// Estado compartido de uso de la IA local.
pub struct Activity {
    active: AtomicUsize,
    last_end: Mutex<Instant>,
    /// Modelo de Ollama que Canela usó y podría seguir cargado.
    model: Mutex<Option<String>>,
    release_requested: AtomicBool,
    last_heartbeat: Mutex<Option<Instant>>,
}

impl Default for Activity {
    fn default() -> Self {
        Self {
            active: AtomicUsize::new(0),
            last_end: Mutex::new(Instant::now()),
            model: Mutex::new(None),
            release_requested: AtomicBool::new(false),
            last_heartbeat: Mutex::new(None),
        }
    }
}

pub struct ActivityGuard(&'static Activity);

impl Drop for ActivityGuard {
    fn drop(&mut self) {
        *self.0.last_end.lock().unwrap() = Instant::now();
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Activity {
    /// Marca el inicio de un uso (respuesta, herramientas, resumen…).
    pub fn begin(&'static self) -> ActivityGuard {
        self.active.fetch_add(1, Ordering::SeqCst);
        ActivityGuard(self)
    }

    pub fn is_busy(&self) -> bool {
        self.active.load(Ordering::SeqCst) > 0
    }

    pub fn note_model(&self, model: &str) {
        *self.model.lock().unwrap() = Some(model.to_string());
    }

    pub fn model(&self) -> Option<String> {
        self.model.lock().unwrap().clone()
    }

    pub fn idle_for(&self, now: Instant) -> Duration {
        now.saturating_duration_since(*self.last_end.lock().unwrap())
    }
}

pub static ACTIVITY: std::sync::LazyLock<Activity> = std::sync::LazyLock::new(Activity::default);

/// Atajo: `let _g = resources::busy();` mientras se usa la IA.
pub fn busy() -> ActivityGuard {
    ACTIVITY.begin()
}

// ------------------------------------------------------------ Ollama

pub async fn ollama_ps(http: &reqwest::Client, base: &str) -> Option<Vec<AiModelMem>> {
    let r = http
        .get(format!("{base}/api/ps"))
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .ok()?;
    let v: Value = r.json().await.ok()?;
    Some(memdiag::parse_ollama_ps(&v))
}

/// Descarga un modelo de la memoria (`keep_alive: 0`).
pub async fn ollama_unload(http: &reqwest::Client, base: &str, model: &str) -> Result<(), String> {
    let r = http
        .post(format!("{base}/api/generate"))
        .json(&json!({ "model": model, "keep_alive": 0 }))
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("No pude hablar con Ollama: {e}"))?;
    if r.status().is_success() {
        Ok(())
    } else {
        Err(format!("Ollama respondió {} al descargar «{model}».", r.status()))
    }
}

/// Carga el modelo (o renueva su plazo) sin generar texto.
pub async fn ollama_keep(http: &reqwest::Client, base: &str, model: &str, keep_alive: &str) -> Result<(), String> {
    http.post(format!("{base}/api/generate"))
        .json(&json!({ "model": model, "keep_alive": keep_alive }))
        .timeout(Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn same(a: &str, b: &str) -> bool {
    crate::ai::ollama::same_model(a, b)
}

/// Una vuelta del gestor (cada pocos segundos). Devuelve eventos para el registro.
pub async fn tick(act: &Activity, http: &reqwest::Client, settings: &Settings, now: Instant) -> Vec<MemEvent> {
    let params = mode(settings);
    let base = crate::ai::ollama::base_url(settings);
    let mut out = Vec::new();
    let ts = || chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    if act.is_busy() {
        // Renovar el plazo mientras ejecuta herramientas largas.
        let Some(model) = act.model() else { return out };
        let due = act
            .last_heartbeat
            .lock()
            .unwrap()
            .is_none_or(|h| now.saturating_duration_since(h) >= Duration::from_secs(45));
        if due {
            *act.last_heartbeat.lock().unwrap() = Some(now);
            if let Some(ps) = ollama_ps(http, &base).await {
                if ps.iter().any(|m| same(&m.name, &model)) {
                    let _ = ollama_keep(http, &base, &model, params.keep_alive).await;
                }
            }
        }
        return out;
    }
    *act.last_heartbeat.lock().unwrap() = None;

    if act.release_requested.swap(false, Ordering::SeqCst) {
        let msg = release_all(http, &base).await;
        *act.model.lock().unwrap() = None;
        out.push(MemEvent {
            ts: ts(),
            kind: "ia_liberacion".into(),
            message: msg,
            pid: None,
            name: None,
            bytes: None,
        });
        return out;
    }

    let Some(model) = act.model() else { return out };
    if act.idle_for(now) < params.idle {
        return out;
    }
    *act.model.lock().unwrap() = None;
    let Some(ps) = ollama_ps(http, &base).await else { return out };
    let Some(_) = ps.iter().find(|m| same(&m.name, &model)) else {
        // Ollama ya lo descargó solo gracias a keep_alive.
        out.push(MemEvent {
            ts: ts(),
            kind: "ia_descargada".into(),
            message: format!(
                "{model} ya no está en memoria: Ollama lo descargó por inactividad (keep_alive {}).",
                params.keep_alive
            ),
            pid: None,
            name: Some(model),
            bytes: None,
        });
        return out;
    };
    if let Some(m) = ps.iter().find(|m| same(&m.name, &model)) {
        if ollama_unload(http, &base, &m.name).await.is_ok() {
            out.push(MemEvent {
                ts: ts(),
                kind: "ia_descargada".into(),
                message: format!(
                    "Descargué {} de la memoria tras {} s sin uso (liberó ≈ {}). Se volverá a cargar con el próximo mensaje.",
                    m.name,
                    params.idle.as_secs(),
                    memdiag::gb(m.size)
                ),
                pid: None,
                name: Some(m.name.clone()),
                bytes: Some(m.size),
            });
        }
    }
    out
}

async fn release_all(http: &reqwest::Client, base: &str) -> String {
    let mut freed = Vec::new();
    if let Some(ps) = ollama_ps(http, base).await {
        for m in ps {
            if ollama_unload(http, base, &m.name).await.is_ok() {
                freed.push(format!("{} (≈ {})", m.name, memdiag::gb(m.size)));
            }
        }
    }
    let voice = crate::voice::local::unload_all();
    let mut parts = Vec::new();
    if !freed.is_empty() {
        parts.push(format!("modelos de IA: {}", freed.join(", ")));
    }
    if voice {
        parts.push("modelo de voz".to_string());
    }
    if parts.is_empty() {
        "No había modelos de IA cargados.".into()
    } else {
        format!("Liberé {}. Se cargarán de nuevo cuando los necesites.", parts.join(" y "))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseResult {
    pub pending: bool,
    pub message: String,
}

/// Botón «Liberar memoria de IA».
pub async fn release(act: &Activity, http: &reqwest::Client, settings: &Settings) -> ReleaseResult {
    if act.is_busy() {
        act.release_requested.store(true, Ordering::SeqCst);
        return ReleaseResult {
            pending: true,
            message: "Canela está respondiendo o usando una herramienta: liberaré la memoria en cuanto termine.".into(),
        };
    }
    act.release_requested.store(false, Ordering::SeqCst);
    *act.model.lock().unwrap() = None;
    let message = release_all(http, &crate::ai::ollama::base_url(settings)).await;
    ReleaseResult { pending: false, message }
}

// ------------------------------------------------------------ registro

const MAX_SAMPLES: usize = 1500;
const MAX_EVENTS: usize = 300;

#[derive(Default)]
pub struct DiagStore {
    pub samples: VecDeque<Sample>,
    pub events: VecDeque<MemEvent>,
    pub detector: Detector,
    pub latest_all: Vec<ProcMem>,
}

impl DiagStore {
    pub fn push_sample(&mut self, mut s: Sample) {
        if let Some(last) = self.samples.back_mut() {
            // Sólo la última muestra guarda la lista completa de procesos.
            last.top.truncate(5);
        }
        s.top.truncate(12);
        self.samples.push_back(s);
        while self.samples.len() > MAX_SAMPLES {
            self.samples.pop_front();
        }
    }

    pub fn push_events(&mut self, evs: &[MemEvent]) {
        for e in evs {
            self.events.push_back(e.clone());
        }
        while self.events.len() > MAX_EVENTS {
            self.events.pop_front();
        }
    }
}

/// Toma una muestra completa.
pub async fn take_sample(
    http: &reqwest::Client,
    settings: &Settings,
    sys: Arc<Mutex<Option<sysinfo::System>>>,
    t: u64,
) -> (Sample, Vec<ProcMem>) {
    let base = crate::ai::ollama::base_url(settings);
    // Sólo consulta este equipo (127.0.0.1); si Ollama no está abierto, `None`.
    let ai = ollama_ps(http, &base).await;
    let (sysmem, gpu, all) = tokio::task::spawn_blocking(move || {
        let mut guard = sys.lock().unwrap();
        if guard.is_none() {
            *guard = sysinfo::System::new().ok();
        }
        let all = guard.as_mut().map(memdiag::read_processes).unwrap_or_default();
        (memdiag::read_sys().unwrap_or_default(), memdiag::read_gpu(), all)
    })
    .await
    .unwrap_or_default();
    let (cp, cw) = memdiag::canela_usage(&all, std::process::id());
    let sample = Sample {
        ts: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        t,
        sys: sysmem,
        gpu,
        ai,
        canela_private: cp,
        canela_working_set: cw,
        top: memdiag::top(&all, 12),
    };
    (sample, all)
}

/// Escribe una línea JSON en el registro diario (y borra los de hace más de 7 días).
pub fn append_log(dir: &std::path::Path, value: &Value) {
    let _ = std::fs::create_dir_all(dir);
    let day = chrono::Local::now().format("%Y-%m-%d").to_string();
    let path = dir.join(format!("memoria-{day}.jsonl"));
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        use std::io::Write;
        let _ = writeln!(f, "{value}");
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        let cutoff = chrono::Local::now() - chrono::Duration::days(7);
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(d) = name.strip_prefix("memoria-").and_then(|n| n.strip_suffix(".jsonl")) {
                if let Ok(date) = chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d") {
                    if date < cutoff.date_naive() {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------ servicio

pub struct Diag {
    pub store: Mutex<DiagStore>,
    sys: Arc<Mutex<Option<sysinfo::System>>>,
    start: Instant,
}

pub static DIAG: std::sync::LazyLock<Diag> = std::sync::LazyLock::new(|| Diag {
    store: Mutex::new(DiagStore::default()),
    sys: Arc::new(Mutex::new(None)),
    start: Instant::now(),
});

fn log_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    use tauri::Manager;
    app.path().app_data_dir().unwrap_or_default().join("diagnostico")
}

/// Toma una muestra, la analiza y la registra. Devuelve los eventos nuevos.
async fn measure(app: &tauri::AppHandle, http: &reqwest::Client, settings: &Settings) -> Vec<MemEvent> {
    let t = DIAG.start.elapsed().as_secs();
    let (sample, all) = take_sample(http, settings, DIAG.sys.clone(), t).await;
    let events = {
        let mut st = DIAG.store.lock().unwrap();
        let ev = st.detector.observe(&sample, &all);
        st.latest_all = all;
        st.push_sample(sample.clone());
        ev
    };
    if settings.memory_log {
        let mut compact = sample.clone();
        compact.top.truncate(8);
        append_log(&log_dir(app), &json!({ "muestra": compact }));
    }
    events
}

fn publish(app: &tauri::AppHandle, settings: &Settings, events: &[MemEvent]) {
    use tauri::Emitter;
    if events.is_empty() {
        return;
    }
    DIAG.store.lock().unwrap().push_events(events);
    for e in events {
        if settings.memory_log {
            append_log(&log_dir(app), &json!({ "evento": e }));
        }
        let _ = app.emit("memory-event", e);
        if e.kind == "ram_baja" || e.kind == "commit_alto" {
            let _ = app.emit("memory-warning", e);
        }
        if e.kind == "ram_ok" {
            let _ = app.emit("memory-warning-clear", ());
        }
        if e.kind == "ia_descargada" || e.kind == "ia_liberacion" {
            let _ = app.emit("ai-memory", json!({ "loaded": false, "message": e.message }));
        }
    }
}

/// Bucle en segundo plano: gestor de inactividad cada 5 s y registro de
/// memoria cada 60 s (120 s en Ahorro de recursos).
pub fn spawn(app: tauri::AppHandle) {
    use tauri::Manager;
    tauri::async_runtime::spawn(async move {
        let mut next_sample = Instant::now() + Duration::from_secs(15);
        loop {
            let state = app.state::<crate::commands::AppState>();
            let settings = state.current_settings();
            let http = state.http.clone();
            let mut events = tick(&ACTIVITY, &http, &settings, Instant::now()).await;
            // El modelo de reconocimiento de voz también se libera si no se usa.
            let voice_idle = mode(&settings).voice_idle.max(Duration::from_secs(5));
            if crate::voice::local::maybe_unload(voice_idle) {
                events.push(MemEvent {
                    ts: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    kind: "voz_descargada".into(),
                    message: format!("Liberé el modelo de reconocimiento de voz tras {} s sin uso.", voice_idle.as_secs()),
                    pid: None,
                    name: None,
                    bytes: None,
                });
            }
            if Instant::now() >= next_sample {
                events.extend(measure(&app, &http, &settings).await);
                next_sample = Instant::now() + mode(&settings).sample_every;
            }
            publish(&app, &settings, &events);
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistPoint {
    pub ts: String,
    pub available: u64,
    pub commit_used: u64,
    pub vram_used: Option<u64>,
    pub ai_loaded: bool,
    pub canela: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemStatus {
    pub mode: String,
    pub latest: Option<Sample>,
    pub events: Vec<MemEvent>,
    pub history: Vec<HistPoint>,
    pub busy: bool,
    pub low_ram: bool,
    pub idle_unload_secs: u64,
    pub log_dir: String,
    pub logging: bool,
}

fn status(app: &tauri::AppHandle, settings: &Settings) -> MemStatus {
    let st = DIAG.store.lock().unwrap();
    let latest = st.samples.back().cloned();
    MemStatus {
        mode: settings.resource_mode.clone(),
        low_ram: latest.as_ref().is_some_and(|s| memdiag::ram_is_low(&s.sys)),
        latest,
        events: st.events.iter().rev().take(60).cloned().collect(),
        history: st
            .samples
            .iter()
            .rev()
            .take(180)
            .rev()
            .map(|s| HistPoint {
                ts: s.ts.clone(),
                available: s.sys.available,
                commit_used: s.sys.commit_used,
                vram_used: s.gpu.as_ref().map(|g| g.used),
                ai_loaded: s.ai.as_ref().is_some_and(|m| !m.is_empty()),
                canela: s.canela_private,
            })
            .collect(),
        busy: ACTIVITY.is_busy(),
        idle_unload_secs: mode(settings).idle.as_secs(),
        log_dir: log_dir(app).display().to_string(),
        logging: settings.memory_log,
    }
}

#[tauri::command]
pub fn memory_status(app: tauri::AppHandle, state: tauri::State<'_, crate::commands::AppState>) -> MemStatus {
    status(&app, &state.current_settings())
}

/// Mide ahora mismo (botón «Medir ahora»).
#[tauri::command]
pub async fn memory_measure_now(app: tauri::AppHandle, state: tauri::State<'_, crate::commands::AppState>) -> Result<MemStatus, String> {
    let settings = state.current_settings();
    let events = measure(&app, &state.http, &settings).await;
    publish(&app, &settings, &events);
    Ok(status(&app, &settings))
}

/// Botón «Liberar memoria de IA».
#[tauri::command]
pub async fn memory_release(app: tauri::AppHandle, state: tauri::State<'_, crate::commands::AppState>) -> Result<ReleaseResult, String> {
    let settings = state.current_settings();
    let r = release(&ACTIVITY, &state.http, &settings).await;
    if !r.pending {
        let e = MemEvent {
            ts: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            kind: "ia_liberacion".into(),
            message: r.message.clone(),
            pid: None,
            name: None,
            bytes: None,
        };
        publish(&app, &settings, &[e]);
    }
    state.db.log("memoria", &r.message);
    Ok(r)
}

/// Precarga el modelo local (al abrir el micrófono en modo Equilibrado), para
/// que esté listo cuando termines de hablar.
#[tauri::command]
pub async fn ai_preload(state: tauri::State<'_, crate::commands::AppState>) -> Result<bool, String> {
    let settings = state.current_settings();
    let params = mode(&settings);
    if settings.provider != ProviderId::Ollama || !params.preload_on_mic {
        return Ok(false);
    }
    let base = crate::ai::ollama::base_url(&settings);
    let model = settings.ollama_model.clone();
    ACTIVITY.note_model(&model);
    *ACTIVITY.last_end.lock().unwrap() = Instant::now();
    let http = state.http.clone();
    tauri::async_runtime::spawn(async move {
        let _ = ollama_keep(&http, &base, &model, params.keep_alive).await;
    });
    Ok(true)
}

/// Guarda un informe JSON con las mediciones (para analizar o compartir).
#[tauri::command]
pub fn memory_export(state: tauri::State<'_, crate::commands::AppState>, path: String) -> Result<String, String> {
    let path = std::path::PathBuf::from(path.trim());
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    if !["json", "txt"].contains(&ext.as_str()) {
        return Err("Elige un nombre de archivo terminado en .json".into());
    }
    let settings = state.current_settings();
    let st = DIAG.store.lock().unwrap();
    let report = json!({
        "aplicacion": concat!("Canela AI ", env!("CARGO_PKG_VERSION")),
        "generado": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        "modo": settings.resource_mode,
        "explicacion": {
            "available": "RAM física libre",
            "commitUsed": "memoria comprometida (RAM + archivo de paginación prometida a los programas; en Windows incluye reservas de la GPU)",
            "private": "memoria privada del proceso (cuenta para la memoria comprometida)",
            "workingSet": "parte del proceso que está ahora en la RAM física",
            "sizeVram": "parte del modelo de IA en la memoria de la tarjeta gráfica"
        },
        "resumen": st.samples.back().map(memdiag::sample_summary),
        "procesosAhora": memdiag::top(&st.latest_all, 25),
        "eventos": st.events,
        "muestras": st.samples,
    });
    drop(st);
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("No pude guardar el informe: {e}"))?;
    Ok(path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Ollama simulado: responde /api/ps con `loaded` y registra peticiones.
    async fn fake_ollama(loaded: Arc<Mutex<Vec<String>>>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let log = Arc::new(Mutex::new(Vec::new()));
        let seen = log.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                let loaded = loaded.clone();
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 16384];
                    let mut req = Vec::new();
                    loop {
                        let n = sock.read(&mut buf).await.unwrap_or(0);
                        req.extend_from_slice(&buf[..n]);
                        let t = String::from_utf8_lossy(&req).to_string();
                        if let Some(pos) = t.find("\r\n\r\n") {
                            let len = t
                                .lines()
                                .find_map(|l| {
                                    l.to_lowercase()
                                        .strip_prefix("content-length:")
                                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                                })
                                .unwrap_or(0);
                            if req.len() >= pos + 4 + len || n == 0 {
                                break;
                            }
                        }
                        if n == 0 {
                            break;
                        }
                    }
                    let t = String::from_utf8_lossy(&req).to_string();
                    let line = t.lines().next().unwrap_or("").to_string();
                    let body = t.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
                    seen.lock().unwrap().push(format!("{line} {body}"));
                    let resp = if line.starts_with("GET /api/ps") {
                        let models: Vec<Value> = loaded
                            .lock()
                            .unwrap()
                            .iter()
                            .map(|m| json!({"name": m, "size": 8053063680u64, "size_vram": 8053063680u64}))
                            .collect();
                        json!({ "models": models }).to_string()
                    } else {
                        if body.contains("\"keep_alive\":0") {
                            let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                            let m = v["model"].as_str().unwrap_or("").to_string();
                            loaded.lock().unwrap().retain(|x| x != &m);
                        }
                        json!({"done": true}).to_string()
                    };
                    let _ = sock
                        .write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{resp}", resp.len()).as_bytes())
                        .await;
                });
            }
        });
        (format!("http://{addr}"), log)
    }

    fn settings(base: &str, mode: &str) -> Settings {
        Settings {
            provider: ProviderId::Ollama,
            ollama_url: base.into(),
            resource_mode: mode.into(),
            ..Settings::default()
        }
    }

    fn leak(a: Activity) -> &'static Activity {
        Box::leak(Box::new(a))
    }

    #[tokio::test]
    async fn unloads_after_two_minutes_idle_but_never_while_busy() {
        let loaded = Arc::new(Mutex::new(vec!["qwen3:8b".to_string()]));
        let (base, log) = fake_ollama(loaded.clone()).await;
        let s = settings(&base, "balanced");
        let http = reqwest::Client::new();
        let act = leak(Activity::default());
        act.note_model("qwen3:8b");
        let t0 = Instant::now();

        // Respondiendo / ejecutando herramientas durante 5 minutos: no se descarga.
        let g = act.begin();
        for sec in [10u64, 60, 130, 300] {
            let e = tick(act, &http, &s, t0 + Duration::from_secs(sec)).await;
            assert!(e.is_empty());
        }
        assert_eq!(loaded.lock().unwrap().len(), 1, "no se descarga mientras trabaja");
        let renewals = log
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.contains("/api/generate") && l.contains("\"2m\""))
            .count();
        assert!(
            renewals >= 2,
            "renueva keep_alive durante herramientas largas: {:?}",
            log.lock().unwrap()
        );
        drop(g);

        // Inactivo menos de 2 min: sigue cargado.
        let now = Instant::now();
        assert!(tick(act, &http, &s, now + Duration::from_secs(90)).await.is_empty());
        assert_eq!(loaded.lock().unwrap().len(), 1);
        // A los 2 min: se descarga.
        let e = tick(act, &http, &s, now + Duration::from_secs(121)).await;
        assert_eq!(e[0].kind, "ia_descargada", "{e:?}");
        assert!(loaded.lock().unwrap().is_empty());
        assert!(log.lock().unwrap().iter().any(|l| l.contains("\"keep_alive\":0")));
        // Ya no vuelve a intentarlo.
        assert!(tick(act, &http, &s, now + Duration::from_secs(400)).await.is_empty());
    }

    #[tokio::test]
    async fn saver_mode_unloads_sooner_and_renewal_never_reloads() {
        let loaded = Arc::new(Mutex::new(vec!["qwen3:8b".to_string()]));
        let (base, log) = fake_ollama(loaded.clone()).await;
        let s = settings(&base, "saver");
        let http = reqwest::Client::new();
        let act = leak(Activity::default());
        act.note_model("qwen3:8b");
        let now = Instant::now();
        let e = tick(act, &http, &s, now + Duration::from_secs(31)).await;
        assert_eq!(e[0].kind, "ia_descargada");

        // Si el usuario ya lo descargó (ollama stop), renovar no lo vuelve a cargar.
        let act = leak(Activity::default());
        act.note_model("qwen3:8b");
        let _g = act.begin();
        log.lock().unwrap().clear();
        tick(act, &http, &s, Instant::now()).await;
        assert!(
            !log.lock().unwrap().iter().any(|l| l.contains("/api/generate")),
            "{:?}",
            log.lock().unwrap()
        );
    }

    #[tokio::test]
    async fn release_button_waits_for_the_answer_to_finish() {
        let loaded = Arc::new(Mutex::new(vec!["qwen3:8b".to_string(), "llama3.2:3b".to_string()]));
        let (base, _) = fake_ollama(loaded.clone()).await;
        let s = settings(&base, "balanced");
        let http = reqwest::Client::new();
        let act = leak(Activity::default());
        let g = act.begin();
        let r = release(act, &http, &s).await;
        assert!(r.pending);
        assert_eq!(loaded.lock().unwrap().len(), 2, "no se toca mientras responde");
        assert!(tick(act, &http, &s, Instant::now()).await.is_empty());
        drop(g);
        let e = tick(act, &http, &s, Instant::now()).await;
        assert_eq!(e[0].kind, "ia_liberacion");
        assert!(
            e[0].message.contains("qwen3:8b") && e[0].message.contains("llama3.2:3b"),
            "{}",
            e[0].message
        );
        assert!(loaded.lock().unwrap().is_empty());
        // Sin nada cargado.
        let r = release(act, &http, &s).await;
        assert!(!r.pending);
        assert!(r.message.contains("No había"));
    }

    #[test]
    fn modes() {
        let mut s = Settings::default();
        assert_eq!(mode(&s).keep_alive, "2m");
        assert_eq!(mode(&s).idle, Duration::from_secs(120));
        s.resource_mode = "saver".into();
        assert_eq!(mode(&s).max_ctx, 8192);
        assert!(!mode(&s).allow_think);
    }

    #[test]
    fn log_files_rotate() {
        let dir = std::env::temp_dir().join(format!("canela-memlog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("memoria-2020-01-01.jsonl"), "{}\n").unwrap();
        append_log(&dir, &json!({"a": 1}));
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
        assert!(names[0].starts_with("memoria-20"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
