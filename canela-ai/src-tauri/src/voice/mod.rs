//! Voz de Canela.
//!
//! - Escuchar: micrófono (cpal) → detector de fin de frase → WAV en memoria →
//!   transcripción con la API oficial de OpenAI (`/v1/audio/transcriptions`).
//! - Hablar: por defecto con las voces de Windows desde la interfaz (gratis,
//!   sin conexión). Opcional: voz de OpenAI (`/v1/audio/speech`) reproducida
//!   aquí por el altavoz elegido.
//! - El micrófono sólo se abre cuando lo activas (botón o atajo) y se cierra
//!   al terminar la frase. El audio nunca se guarda en disco.

pub mod audio;
pub mod local;
pub mod vad;

use crate::commands::AppState;
use crate::secrets;
use crate::settings::ProviderId;
use audio::{RecEvent, CTRL_CANCEL, CTRL_FINISH, CTRL_RUN};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

pub const STT_URL: &str = "https://api.openai.com/v1/audio/transcriptions";
pub const TTS_URL: &str = "https://api.openai.com/v1/audio/speech";
/// La voz de OpenAI entrega PCM 16 bits mono a 24 kHz con `response_format: "pcm"`.
pub const TTS_RATE: u32 = 24_000;

fn url(default: &str, env: &str) -> String {
    #[cfg(any(test, debug_assertions))]
    if let Ok(u) = std::env::var(env) {
        return u;
    }
    let _ = env;
    // En pruebas nunca se sale a internet.
    if cfg!(test) {
        return "http://127.0.0.1:9/sin-red".into();
    }
    default.to_string()
}

// ------------------------------------------------------------------ WAV

pub fn encode_wav(samples: &[f32], rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    out
}

// ---------------------------------------------------------- transcripción

pub async fn transcribe(http: &reqwest::Client, api_key: &str, model: &str, wav: Vec<u8>) -> Result<String, String> {
    let part = reqwest::multipart::Part::bytes(wav)
        .file_name("canela.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;
    let form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("model", model.to_string())
        .text("language", "es");
    let resp = http
        .post(url(STT_URL, "CANELA_TEST_STT_URL"))
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| {
            if e.is_connect() {
                "Sin conexión con OpenAI para entender tu voz.".to_string()
            } else {
                format!("Error de red: {e}")
            }
        })?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(crate::ai::http_error_message(ProviderId::Openai, status.as_u16(), &body, model));
    }
    let v: serde_json::Value = serde_json::from_str(&body).map_err(|_| "Respuesta de transcripción no válida.".to_string())?;
    Ok(v["text"].as_str().unwrap_or_default().trim().to_string())
}

pub async fn synthesize(
    http: &reqwest::Client,
    api_key: &str,
    model: &str,
    voice: &str,
    text: &str,
    speed: f32,
) -> Result<Vec<i16>, String> {
    let body = serde_json::json!({
        "model": model,
        "voice": voice,
        "input": text,
        "response_format": "pcm",
        "speed": speed.clamp(0.25, 4.0),
        "instructions": "Habla en español latino, con tono cálido, cercano y alegre.",
    });
    let resp = http
        .post(url(TTS_URL, "CANELA_TEST_TTS_URL"))
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Error de red: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let b = resp.text().await.unwrap_or_default();
        return Err(crate::ai::http_error_message(ProviderId::Openai, status.as_u16(), &b, model));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    Ok(bytes.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect())
}

// ------------------------------------------------------- estado en vivo

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceStateEvent {
    /// listening | processing | idle | error
    pub state: &'static str,
    pub level: Option<f32>,
    pub message: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct TranscriptEvent {
    pub text: String,
}

#[derive(Default)]
pub struct VoiceRuntime {
    recording: Mutex<Option<Arc<AtomicU8>>>,
    tts_tx: Mutex<Option<mpsc::Sender<(u64, String, TtsEngine)>>>,
    tts_gen: Arc<AtomicU64>,
    tts_stop: Arc<AtomicBool>,
}

fn emit_state(app: &AppHandle, state: &'static str, level: Option<f32>, message: Option<String>) {
    let _ = app.emit("voice-state", VoiceStateEvent { state, level, message });
}

/// Con un proveedor local (Ollama) la conversación no sale del equipo: la
/// voz de OpenAI (que envía audio o texto a internet) queda bloqueada salvo
/// que el usuario la permita expresamente.
pub fn cloud_voice_allowed(settings: &crate::settings::Settings) -> Result<(), String> {
    if settings.provider.is_cloud() || settings.ollama_cloud_voice {
        Ok(())
    } else {
        Err("Con Ollama tu conversación no sale de este equipo. Para entender tu voz o hablar con la voz de OpenAI habría que enviar audio o texto a internet: si lo aceptas, actívalo en Ajustes › Voz. Mientras tanto puedes escribirle a Canela y usar la voz de Windows.".into())
    }
}

fn openai_key() -> Result<String, String> {
    #[cfg(debug_assertions)]
    if let Ok(k) = std::env::var("CANELA_DEV_API_KEY") {
        return Ok(k);
    }
    secrets::get_key(ProviderId::Openai)?.ok_or_else(|| {
        "Para entender tu voz necesito una clave de OpenAI (aunque el chat use Claude). Agrégala en Ajustes › Voz.".to_string()
    })
}

#[tauri::command]
pub fn voice_devices() -> audio::Devices {
    audio::list_devices()
}

/// Abre el micrófono. Termina sola al detectar que dejaste de hablar.
#[tauri::command]
pub async fn voice_start(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let settings = state.current_settings();
    if !settings.voice_consent {
        return Err("Primero acepta el aviso de privacidad de la voz en Ajustes › Voz.".into());
    }
    // Reconocimiento local (whisper.cpp): nada sale del equipo.
    let use_local = settings.stt_engine == "local";
    let voice_dir = local::voice_dir(&state.data_dir);
    let key = if use_local {
        if !local::stt_available() {
            return Err("Esta versión no incluye el reconocimiento de voz local.".into());
        }
        let a = local::asset(&settings.stt_local_model).ok_or("Modelo de voz desconocido.")?;
        if !local::is_installed(&voice_dir, a) {
            return Err(format!(
                "Para entender tu voz sin internet falta descargar «{}» ({} MB) en Ajustes › Voz.",
                a.label,
                local::total_size(a) / 1_000_000
            ));
        }
        String::new()
    } else {
        cloud_voice_allowed(&settings)?;
        tauri::async_runtime::spawn_blocking(openai_key)
            .await
            .map_err(|e| e.to_string())??
    };
    let local_model = settings.stt_local_model.clone();
    let voice_idle = crate::resources::mode(&settings).voice_idle;
    let ctrl = Arc::new(AtomicU8::new(CTRL_RUN));
    {
        let mut rec = state.voice.recording.lock().unwrap();
        if rec.is_some() {
            return Err("Ya te estoy escuchando.".into());
        }
        *rec = Some(ctrl.clone());
    }
    emit_state(&app, "listening", Some(0.0), None);
    let device = settings.voice_input_device.clone();
    let model = settings.stt_model.clone();
    let http = state.http.clone();
    let db = state.db.clone();
    let app2 = app.clone();
    let on_event: Arc<dyn Fn(RecEvent) + Send + Sync> = Arc::new(move |ev| {
        let app = app2.clone();
        match ev {
            RecEvent::Level(l) => emit_state(&app, "listening", Some(l), None),
            RecEvent::SpeechStarted => {}
            RecEvent::Finished(samples) => {
                clear_recording(&app);
                if samples.len() < audio::TARGET_RATE as usize * 3 / 10 {
                    return emit_state(&app, "idle", None, Some("No alcancé a escucharte.".into()));
                }
                emit_state(&app, "processing", None, None);
                let (http, key, model, db) = (http.clone(), key.clone(), model.clone(), db.clone());
                let (voice_dir, local_model) = (voice_dir.clone(), local_model.clone());
                tauri::async_runtime::spawn(async move {
                    let seconds = samples.len() as f32 / audio::TARGET_RATE as f32;
                    let result = if use_local {
                        let started = std::time::Instant::now();
                        let r = tokio::task::spawn_blocking(move || {
                            let r = local::transcribe(&voice_dir, &local_model, &samples, Some("Hola, Canela."));
                            // Modo Ahorro: el modelo de voz se descarga al terminar.
                            if voice_idle.is_zero() {
                                local::unload_all();
                            }
                            r
                        })
                        .await
                        .map_err(|e| e.to_string())
                        .and_then(|r| r);
                        if r.is_ok() {
                            db.log(
                                "voz",
                                &format!(
                                    "Transcribí {seconds:.0} s de audio en este equipo ({:.1} s)",
                                    started.elapsed().as_secs_f32()
                                ),
                            );
                        }
                        r
                    } else {
                        let wav = encode_wav(&samples, audio::TARGET_RATE);
                        drop(samples);
                        let r = transcribe(&http, &key, &model, wav).await;
                        if r.is_ok() {
                            db.log("voz", &format!("Transcribí {seconds:.0} s de audio con OpenAI"));
                        }
                        r
                    };
                    match result {
                        Ok(text) if !text.is_empty() => {
                            let _ = app.emit("voice-transcript", TranscriptEvent { text });
                            emit_state(&app, "idle", None, None);
                        }
                        Ok(_) => emit_state(&app, "idle", None, Some("No entendí nada. ¿Lo repites?".into())),
                        Err(e) => emit_state(&app, "error", None, Some(e)),
                    }
                });
            }
            RecEvent::NoSpeech => {
                clear_recording(&app);
                emit_state(&app, "idle", None, Some("No te escuché hablar.".into()));
            }
            RecEvent::Cancelled => {
                clear_recording(&app);
                emit_state(&app, "idle", None, None);
            }
            RecEvent::Error(e) => {
                clear_recording(&app);
                emit_state(&app, "error", None, Some(e));
            }
        }
    });
    std::thread::spawn(move || audio::record(device, ctrl, on_event));
    Ok(())
}

fn clear_recording(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        *state.voice.recording.lock().unwrap() = None;
    }
}

/// Termina la grabación ya (y transcribe lo escuchado).
#[tauri::command]
pub fn voice_stop(state: State<'_, AppState>) -> bool {
    match state.voice.recording.lock().unwrap().as_ref() {
        Some(c) => {
            c.store(CTRL_FINISH, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

/// Descarta la grabación sin enviar nada.
#[tauri::command]
pub fn voice_cancel(state: State<'_, AppState>) -> bool {
    match state.voice.recording.lock().unwrap().as_ref() {
        Some(c) => {
            c.store(CTRL_CANCEL, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

// -------------------------------------------------- voz de OpenAI (opcional)

#[derive(Debug, Clone, Copy, PartialEq)]
enum TtsEngine {
    /// Voz de OpenAI (internet).
    Cloud,
    /// Piper (este equipo).
    Piper,
}

/// Encola un fragmento para decirlo con la voz de OpenAI.
#[tauri::command]
pub fn tts_cloud_speak(app: AppHandle, state: State<'_, AppState>, text: String) -> Result<(), String> {
    cloud_voice_allowed(&state.current_settings())?;
    enqueue(app, state, text, TtsEngine::Cloud)
}

/// Encola un fragmento para decirlo con Piper (voz natural sin internet).
#[tauri::command]
pub fn tts_local_speak(app: AppHandle, state: State<'_, AppState>, text: String) -> Result<(), String> {
    let s = state.current_settings();
    let dir = local::voice_dir(&state.data_dir);
    let voice = local::asset(&s.piper_voice).ok_or("Voz desconocida.")?;
    if !local::piper_exe(&dir).exists() || !local::is_installed(&dir, voice) {
        return Err("Para la voz natural sin internet descarga Piper y una voz en Ajustes › Voz.".into());
    }
    enqueue(app, state, text, TtsEngine::Piper)
}

fn enqueue(app: AppHandle, state: State<'_, AppState>, text: String, engine: TtsEngine) -> Result<(), String> {
    let text: String = text.chars().take(4000).collect();
    if text.trim().is_empty() {
        return Ok(());
    }
    let mut tx = state.voice.tts_tx.lock().unwrap();
    if tx.is_none() {
        let (sender, rx) = mpsc::channel::<(u64, String, TtsEngine)>();
        let gen = state.voice.tts_gen.clone();
        let stop = state.voice.tts_stop.clone();
        let app2 = app.clone();
        std::thread::spawn(move || {
            while let Ok((g, text, engine)) = rx.recv() {
                if g != gen.load(Ordering::SeqCst) {
                    continue; // se pidió silencio después de encolarlo
                }
                let state = app2.state::<AppState>();
                let s = state.current_settings();
                let http = state.http.clone();
                stop.store(false, Ordering::SeqCst);
                let synth: Result<(Vec<i16>, u32), String> = match engine {
                    TtsEngine::Piper => {
                        let dir = local::voice_dir(&state.data_dir);
                        match local::asset(&s.piper_voice) {
                            Some(v) => local::piper_say(&dir, v, &text, s.tts_rate, &stop).map(|pcm| (pcm, local::voice_rate(&dir, v))),
                            None => Err("Voz desconocida.".into()),
                        }
                    }
                    TtsEngine::Cloud => match openai_key() {
                        Ok(key) => tauri::async_runtime::block_on(synthesize(
                            &http,
                            &key,
                            &s.tts_cloud_model,
                            &s.tts_cloud_voice,
                            &text,
                            s.tts_rate,
                        ))
                        .map(|pcm| (pcm, TTS_RATE)),
                        Err(e) => Err(e),
                    },
                };
                if g != gen.load(Ordering::SeqCst) {
                    continue;
                }
                match synth {
                    Ok((pcm, rate)) => {
                        stop.store(false, Ordering::SeqCst);
                        let _ = app2.emit("tts-state", serde_json::json!({ "speaking": true }));
                        if let Err(e) = audio::play_pcm(s.voice_output_device.as_deref(), &pcm, rate, s.tts_volume, stop.clone()) {
                            let _ = app2.emit("tts-state", serde_json::json!({ "speaking": false, "error": e }));
                            continue;
                        }
                    }
                    Err(e) => {
                        let _ = app2.emit("tts-state", serde_json::json!({ "speaking": false, "error": e }));
                        continue;
                    }
                }
                let _ = app2.emit("tts-state", serde_json::json!({ "speaking": false }));
            }
        });
        *tx = Some(sender);
    }
    let g = state.voice.tts_gen.load(Ordering::SeqCst);
    tx.as_ref().unwrap().send((g, text, engine)).map_err(|e| e.to_string())
}

/// Calla a Canela (voz de OpenAI) y descarta lo pendiente.
#[tauri::command]
pub fn tts_cloud_stop(state: State<'_, AppState>) {
    state.voice.tts_gen.fetch_add(1, Ordering::SeqCst);
    state.voice.tts_stop.store(true, Ordering::SeqCst);
}

// ---------------------------------------------------- modelos de voz locales

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalAsset {
    pub id: &'static str,
    pub kind: local::Kind,
    pub label: &'static str,
    pub note: &'static str,
    pub license: &'static str,
    pub size: u64,
    pub installed: bool,
    /// El programa Piper sólo existe para Windows en esta versión.
    pub supported: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalVoiceStatus {
    pub stt_available: bool,
    pub assets: Vec<LocalAsset>,
    pub loaded_stt: Option<String>,
    pub downloading: Option<String>,
    pub dir: String,
}

#[tauri::command]
pub fn local_voice_status(state: State<'_, AppState>) -> LocalVoiceStatus {
    let dir = local::voice_dir(&state.data_dir);
    LocalVoiceStatus {
        stt_available: local::stt_available(),
        assets: local::CATALOG
            .iter()
            .map(|a| LocalAsset {
                id: a.id,
                kind: a.kind,
                label: a.label,
                note: a.note,
                license: a.license,
                size: local::total_size(a),
                installed: local::is_installed(&dir, a),
                supported: a.kind != local::Kind::Engine || cfg!(windows),
            })
            .collect(),
        loaded_stt: local::loaded_stt(),
        downloading: local::downloading(),
        dir: dir.display().to_string(),
    }
}

/// Descarga un modelo de voz. Sólo se llama cuando el usuario pulsa
/// «Descargar» viendo el tamaño (consentimiento explícito).
#[tauri::command]
pub async fn local_voice_download(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let a = local::asset(&id).ok_or("Recurso desconocido.")?;
    if a.kind == local::Kind::Engine && !cfg!(windows) {
        return Err("El programa Piper descargable es sólo para Windows.".into());
    }
    let dir = local::voice_dir(&state.data_dir);
    let app2 = app.clone();
    let id2 = id.clone();
    let last = Mutex::new(std::time::Instant::now() - std::time::Duration::from_secs(1));
    let progress = move |done: u64, total: u64| {
        let mut l = last.lock().unwrap();
        if l.elapsed() >= std::time::Duration::from_millis(250) || done == total {
            *l = std::time::Instant::now();
            let _ = app2.emit("voice-download", serde_json::json!({ "id": id2, "done": done, "total": total }));
        }
    };
    let r = local::download(&state.http, &dir, a, &progress).await;
    let _ = app.emit(
        "voice-download",
        serde_json::json!({ "id": id, "finished": true, "error": r.as_ref().err() }),
    );
    if r.is_ok() {
        state
            .db
            .log("voz", &format!("Descargué {} ({} MB)", a.label, local::total_size(a) / 1_000_000));
    }
    r
}

#[tauri::command]
pub fn local_voice_cancel() {
    local::cancel_download();
}

#[tauri::command]
pub fn local_voice_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let a = local::asset(&id).ok_or("Recurso desconocido.")?;
    local::remove(&local::voice_dir(&state.data_dir), a)?;
    state.db.log("voz", &format!("Borré {}", a.label));
    Ok(())
}

#[cfg(test)]
mod tests {

    #[test]
    fn cloud_voice_is_blocked_with_ollama_unless_allowed() {
        use crate::settings::{ProviderId, Settings};
        let mut s = Settings {
            provider: ProviderId::Ollama,
            ..Settings::default()
        };
        assert!(cloud_voice_allowed(&s).unwrap_err().contains("no sale de este equipo"));
        s.ollama_cloud_voice = true;
        assert!(cloud_voice_allowed(&s).is_ok());
        s.ollama_cloud_voice = false;
        s.provider = ProviderId::Anthropic;
        assert!(cloud_voice_allowed(&s).is_ok());
    }

    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn wav_header_is_valid() {
        let w = encode_wav(&[0.0, 0.5, -0.5, 1.0], 16_000);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes([w[24], w[25], w[26], w[27]]), 16_000);
        assert_eq!(u32::from_le_bytes([w[40], w[41], w[42], w[43]]), 8);
        assert_eq!(w.len(), 44 + 8);
    }

    /// Transcripción contra un servidor local que imita a OpenAI.
    #[tokio::test]
    async fn transcribes_with_multipart_request() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(String::new()));
        let s2 = seen.clone();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 256 * 1024];
            let mut req = Vec::new();
            loop {
                let n = sock.read(&mut buf).await.unwrap();
                req.extend_from_slice(&buf[..n]);
                let t = String::from_utf8_lossy(&req).to_string();
                if t.contains("--\r\n") && t.matches("--").count() > 4 || n == 0 {
                    break;
                }
            }
            *s2.lock().unwrap() = String::from_utf8_lossy(&req).to_string();
            let body = r#"{"text":"  Canela, ¿qué tenemos pendiente hoy?  "}"#;
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            sock.write_all(head.as_bytes()).await.unwrap();
            sock.write_all(body.as_bytes()).await.unwrap();
        });
        std::env::set_var("CANELA_TEST_STT_URL", format!("http://{addr}/v1/audio/transcriptions"));
        let wav = encode_wav(&vec![0.1; 16_000], 16_000);
        let text = transcribe(&reqwest::Client::new(), "sk-test", "gpt-transcribe", wav).await.unwrap();
        std::env::remove_var("CANELA_TEST_STT_URL");
        assert_eq!(text, "Canela, ¿qué tenemos pendiente hoy?");
        let req = seen.lock().unwrap().clone();
        assert!(req.contains("name=\"model\""));
        assert!(req.contains("gpt-transcribe"));
        assert!(req.contains("name=\"language\""));
        assert!(req.contains("Bearer sk-test"));
    }
}
