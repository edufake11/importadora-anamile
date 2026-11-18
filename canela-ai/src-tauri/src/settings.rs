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
}

impl ProviderId {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderId::Anthropic => "anthropic",
            ProviderId::Openai => "openai",
        }
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
    /// Carpetas a las que Canela puede acceder. `None` = Documentos, Descargas y Escritorio.
    pub allowed_folders: Option<Vec<String>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: ProviderId::Anthropic,
            anthropic_model: "claude-opus-5-5".into(),
            anthropic_effort: "medium".into(),
            // Editable desde Ajustes: usa cualquier modelo de chat de tu cuenta de OpenAI.
            openai_model: "gpt-5".into(),
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
            allowed_folders: None,
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
        self.user_name = self.user_name.trim().chars().take(40).collect();
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
    fs::create_dir_all(config_dir)
        .map_err(|e| format!("No se pudo crear la carpeta de configuración: {e}"))?;
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    // Escritura atómica: archivo temporal + rename.
    let tmp = config_dir.join("settings.json.tmp");
    fs::write(&tmp, json).map_err(|e| format!("No se pudo guardar la configuración: {e}"))?;
    fs::rename(&tmp, settings_path(config_dir))
        .map_err(|e| format!("No se pudo guardar la configuración: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_use_claude() {
        let s = Settings::default();
        assert_eq!(s.provider, ProviderId::Anthropic);
        assert_eq!(s.anthropic_model, "claude-opus-5-5");
        assert_eq!(s.pet_scale, 3);
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
