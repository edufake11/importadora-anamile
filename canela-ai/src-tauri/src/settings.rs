//! Ajustes no secretos de Canela, guardados como JSON en la carpeta de
//! configuración de la app (en Windows: %APPDATA%\com.canela.ai\settings.json).
//! Las claves API NUNCA se guardan aquí: ver `secrets.rs`.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    Anthropic,
    Openai,
    /// Modelos locales con Ollama: sin clave y sin salir del equipo.
    Ollama,
}

impl ProviderId {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderId::Anthropic => "anthropic",
            ProviderId::Openai => "openai",
            ProviderId::Ollama => "ollama",
        }
    }

    /// Ollama funciona sin clave API.
    pub fn needs_key(&self) -> bool {
        !matches!(self, ProviderId::Ollama)
    }

    /// ¿La conversación se envía a un servicio en internet?
    pub fn is_cloud(&self) -> bool {
        !matches!(self, ProviderId::Ollama)
    }
}

pub const ANTHROPIC_EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Proveedor activo para el chat.
    pub provider: ProviderId,
    pub anthropic_model: String,
    /// Nivel de esfuerzo de razonamiento de Claude (`output_config.effort`).
    pub anthropic_effort: String,
    pub openai_model: String,
    /// Dirección de Ollama (sólo este equipo).
    pub ollama_url: String,
    pub ollama_model: String,
    /// Tamaño de contexto en tokens (`options.num_ctx`).
    pub ollama_num_ctx: u32,
    /// Dejar que el modelo razone antes de responder (más lento; el
    /// razonamiento nunca se muestra).
    pub ollama_think: bool,
    /// Con Ollama, permitir igualmente la voz de OpenAI (envía audio/texto a internet).
    pub ollama_cloud_voice: bool,
    /// «balanced» (Equilibrado) o «saver» (Ahorro de recursos).
    pub resource_mode: String,
    /// Guardar el registro de memoria en archivos (7 días) para diagnosticar.
    pub memory_log: bool,
    /// Cómo quiere el usuario que Canela le llame.
    pub user_name: String,
    /// Escala entera del pixel art (1..=6).
    pub pet_scale: u8,
    /// Si Canela pasea sola por el escritorio.
    pub wander: bool,
    /// Si Canela muestra en una burbuja el inicio de cada respuesta.
    pub reply_bubbles: bool,
    /// Animaciones a media velocidad y sin pasear (menos CPU).
    pub reduce_motion: bool,
    /// Ocultar a Canela mientras hay un juego o app a pantalla completa (Windows).
    pub hide_in_fullscreen: bool,
    /// Saludar y mostrar las tareas pendientes al iniciar.
    pub greet_on_startup: bool,
    /// Iniciar Canela AI junto con Windows.
    pub launch_at_startup: bool,
    /// Guardar el historial de conversaciones en la base de datos local.
    pub save_conversations: bool,
    /// Abrir aplicaciones y carpetas sin pedir confirmación.
    pub auto_open_apps: bool,
    /// Abrir páginas web sin pedir confirmación.
    pub auto_open_web: bool,
    /// Controlar música y volumen sin pedir confirmación.
    pub auto_media_control: bool,
    /// Carpetas a las que Canela puede acceder. `None` = Documentos, Descargas y Escritorio.
    pub allowed_folders: Option<Vec<String>>,
    // ------------------------------------------------------------ voz
    /// Aceptó el aviso: el audio se envía a OpenAI para transcribirlo.
    pub voice_consent: bool,
    /// Micrófono (nombre). `None` = el predeterminado de Windows.
    pub voice_input_device: Option<String>,
    /// Altavoz para la voz de OpenAI. `None` = el predeterminado.
    pub voice_output_device: Option<String>,
    /// Reconocimiento de voz: "local" (whisper.cpp en este equipo) u "openai".
    pub stt_engine: String,
    /// Modelo local de reconocimiento (id del catálogo de voz).
    pub stt_local_model: String,
    /// Modelo de transcripción de OpenAI.
    pub stt_model: String,
    /// "local" (voces de Windows), "piper" (voz natural local) u "openai".
    pub tts_engine: String,
    /// Voz de Piper (id del catálogo de voz).
    pub piper_voice: String,
    /// Nombre de la voz de Windows ("" = la primera en español).
    pub tts_voice: String,
    pub tts_cloud_model: String,
    pub tts_cloud_voice: String,
    /// Velocidad de la voz (0.5 – 2.0).
    pub tts_rate: f32,
    /// Volumen (0 – 1).
    pub tts_volume: f32,
    /// "voice" (sólo si le hablé), "always" o "never".
    pub speak_replies: String,
    /// "Hola, Canela" (preparado; desactivado y aún no disponible).
    pub wake_word: bool,
    /// Permite que Canela pida ver la pantalla (siempre con tu permiso).
    pub vision_enabled: bool,
    /// Pedir permiso antes de enviar a la IA el contenido de tus archivos.
    pub confirm_file_sharing: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // v0.4: el cerebro local es el predeterminado; Claude y ChatGPT son opcionales.
            provider: ProviderId::Ollama,
            anthropic_model: "claude-opus-5-5".into(),
            anthropic_effort: "medium".into(),
            // Editable desde Ajustes: usa cualquier modelo de chat de tu cuenta de OpenAI.
            openai_model: "gpt-5".into(),
            ollama_url: crate::ai::ollama::DEFAULT_URL.into(),
            ollama_model: crate::ai::ollama::DEFAULT_MODEL.into(),
            ollama_num_ctx: crate::ai::ollama::DEFAULT_NUM_CTX,
            ollama_think: false,
            ollama_cloud_voice: false,
            resource_mode: "balanced".into(),
            memory_log: true,
            user_name: String::new(),
            pet_scale: 3,
            wander: true,
            reply_bubbles: true,
            reduce_motion: false,
            hide_in_fullscreen: true,
            greet_on_startup: true,
            launch_at_startup: false,
            save_conversations: false,
            auto_open_apps: true,
            auto_open_web: false,
            auto_media_control: true,
            allowed_folders: None,
            voice_consent: false,
            voice_input_device: None,
            voice_output_device: None,
            stt_engine: "local".into(),
            stt_local_model: "stt-small".into(),
            stt_model: "gpt-transcribe".into(),
            tts_engine: "local".into(),
            piper_voice: "voice-claude".into(),
            tts_voice: String::new(),
            tts_cloud_model: "gpt-4o-mini-tts".into(),
            tts_cloud_voice: "coral".into(),
            tts_rate: 1.0,
            tts_volume: 1.0,
            speak_replies: "voice".into(),
            wake_word: false,
            vision_enabled: true,
            confirm_file_sharing: true,
        }
    }
}

impl Settings {
    /// Corrige valores fuera de rango en vez de fallar (un JSON editado a mano
    /// no debe impedir que la app arranque).
    pub fn sanitized(mut self) -> Self {
        let d = Settings::default();
        self.pet_scale = self.pet_scale.clamp(1, 6);
        if !ANTHROPIC_EFFORTS.contains(&self.anthropic_effort.as_str()) {
            self.anthropic_effort = d.anthropic_effort;
        }
        if self.anthropic_model.trim().is_empty() {
            self.anthropic_model = d.anthropic_model;
        }
        if self.openai_model.trim().is_empty() {
            self.openai_model = d.openai_model;
        }
        self.anthropic_model = self.anthropic_model.trim().to_string();
        self.openai_model = self.openai_model.trim().to_string();
        // Ollama: sólo direcciones de este equipo; modelo y contexto válidos.
        self.ollama_url = crate::ai::ollama::loopback_url(&self.ollama_url).unwrap_or(d.ollama_url.clone());
        self.ollama_model = self.ollama_model.trim().chars().take(120).collect();
        if self.ollama_model.is_empty() {
            self.ollama_model = d.ollama_model.clone();
        }
        if !["balanced", "saver"].contains(&self.resource_mode.as_str()) {
            self.resource_mode = d.resource_mode.clone();
        }
        self.ollama_num_ctx = self
            .ollama_num_ctx
            .clamp(crate::ai::ollama::MIN_NUM_CTX, crate::ai::ollama::MAX_NUM_CTX);
        self.user_name = self.user_name.trim().chars().take(40).collect();
        self.tts_rate = if self.tts_rate.is_finite() {
            self.tts_rate.clamp(0.5, 2.0)
        } else {
            1.0
        };
        self.tts_volume = if self.tts_volume.is_finite() {
            self.tts_volume.clamp(0.0, 1.0)
        } else {
            1.0
        };
        if !["voice", "always", "never"].contains(&self.speak_replies.as_str()) {
            self.speak_replies = d.speak_replies.clone();
        }
        if !["local", "openai", "piper"].contains(&self.tts_engine.as_str()) {
            self.tts_engine = d.tts_engine.clone();
        }
        if !["local", "openai"].contains(&self.stt_engine.as_str()) {
            self.stt_engine = d.stt_engine.clone();
        }
        let kind_ok = |id: &str, k: crate::voice::local::Kind| crate::voice::local::asset(id).is_some_and(|a| a.kind == k);
        if !kind_ok(&self.stt_local_model, crate::voice::local::Kind::Stt) {
            self.stt_local_model = d.stt_local_model.clone();
        }
        if !kind_ok(&self.piper_voice, crate::voice::local::Kind::Voice) {
            self.piper_voice = d.piper_voice.clone();
        }
        for (v, def) in [
            (&mut self.stt_model, &d.stt_model),
            (&mut self.tts_cloud_model, &d.tts_cloud_model),
            (&mut self.tts_cloud_voice, &d.tts_cloud_voice),
        ] {
            if v.trim().is_empty() {
                *v = def.clone();
            }
        }
        // "Hola, Canela" todavía no está disponible: siempre desactivado.
        self.wake_word = false;
        if let Some(folders) = self.allowed_folders.as_mut() {
            folders.retain(|f| !f.trim().is_empty());
            folders.dedup();
            folders.truncate(50);
        }
        self
    }
}

pub fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join("settings.json")
}

pub fn load(config_dir: &Path) -> Settings {
    let path = settings_path(config_dir);
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str::<Settings>(&text)
            .map(Settings::sanitized)
            .unwrap_or_else(|e| {
                eprintln!("[canela] settings.json inválido ({e}); usando valores por defecto");
                Settings::default()
            }),
        Err(_) => Settings::default(),
    }
}

pub fn save(config_dir: &Path, settings: &Settings) -> Result<(), String> {
    fs::create_dir_all(config_dir).map_err(|e| format!("No se pudo crear la carpeta de configuración: {e}"))?;
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    // Escritura atómica: archivo temporal + rename.
    let tmp = config_dir.join("settings.json.tmp");
    fs::write(&tmp, json).map_err(|e| format!("No se pudo guardar la configuración: {e}"))?;
    fs::rename(&tmp, settings_path(config_dir)).map_err(|e| format!("No se pudo guardar la configuración: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_use_local_ollama() {
        let s = Settings::default();
        assert_eq!(s.provider, ProviderId::Ollama);
        assert_eq!(s.ollama_model, "qwen3:8b");
        assert_eq!(s.ollama_url, "http://127.0.0.1:11434");
        assert!(!s.ollama_cloud_voice, "con Ollama nada sale del equipo por defecto");
        assert_eq!(s.anthropic_model, "claude-opus-5-5");
        assert_eq!(s.pet_scale, 3);
        assert!(!s.voice_consent, "la voz necesita consentimiento explícito");
        assert_eq!(s.stt_model, "gpt-transcribe");
        assert_eq!(s.tts_engine, "local");
    }

    #[test]
    fn voice_values_are_clamped() {
        let s = Settings {
            tts_rate: 9.0,
            tts_volume: -1.0,
            speak_replies: "x".into(),
            wake_word: true,
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.tts_rate, 2.0);
        assert_eq!(s.tts_volume, 0.0);
        assert_eq!(s.speak_replies, "voice");
        assert!(!s.wake_word);
    }

    #[test]
    fn ollama_settings_are_sanitized() {
        let s = Settings {
            ollama_url: "http://192.168.0.10:11434".into(),
            ollama_model: "   ".into(),
            ollama_num_ctx: 10,
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.ollama_url, "http://127.0.0.1:11434", "sólo direcciones de este equipo");
        assert_eq!(s.ollama_model, "qwen3:8b");
        assert_eq!(s.ollama_num_ctx, 4096);
        // Un archivo de ajustes de la v0.3 (sin campos de Ollama) se lee bien.
        let old: Settings = serde_json::from_str(r#"{"provider":"anthropic","userName":"Eduardo"}"#).unwrap();
        assert_eq!(old.provider, ProviderId::Anthropic);
        assert_eq!(old.ollama_model, "qwen3:8b");
    }

    #[test]
    fn sanitizes_out_of_range_values() {
        let s = Settings {
            pet_scale: 42,
            anthropic_effort: "ultra".into(),
            anthropic_model: "   ".into(),
            user_name: "  Eduardo  ".into(),
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.pet_scale, 6);
        assert_eq!(s.anthropic_effort, "medium");
        assert_eq!(s.anthropic_model, "claude-opus-5-5");
        assert_eq!(s.user_name, "Eduardo");
    }

    #[test]
    fn partial_json_fills_defaults() {
        let s: Settings = serde_json::from_str(r#"{"provider":"openai","petScale":2}"#).unwrap();
        assert_eq!(s.provider, ProviderId::Openai);
        assert_eq!(s.pet_scale, 2);
        assert!(s.wander);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("canela-test-{}", std::process::id()));
        let s = Settings {
            user_name: "Ana".into(),
            wander: false,
            ..Settings::default()
        };
        save(&dir, &s).unwrap();
        assert_eq!(load(&dir), s);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let dir = std::env::temp_dir().join(format!("canela-test-bad-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(settings_path(&dir), "{ no es json").unwrap();
        assert_eq!(load(&dir), Settings::default());
        let _ = fs::remove_dir_all(&dir);
    }
}
