//! Comandos de captura de pantalla para la interfaz y la herramienta
//! `ver_pantalla`. La imagen sólo vive en memoria hasta que se envía o se
//! descarta.

use crate::commands::AppState;
use crate::tools::ScreenCapturer;
use crate::vision::{self, ImageData, Rect, Target};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};

static NEXT: AtomicU64 = AtomicU64::new(1);

/// Captura ocultando antes a Canela y al panel para que no salgan en la imagen.
pub struct AppCapturer {
    pub app: AppHandle,
}

impl AppCapturer {
    pub async fn capture_now(app: &AppHandle, target: Target) -> Result<image::RgbaImage, String> {
        let wins: Vec<_> = [crate::windows::PET, crate::windows::PANEL]
            .iter()
            .filter_map(|l| app.get_webview_window(l))
            .collect();
        let visible: Vec<_> = wins.iter().filter(|w| w.is_visible().unwrap_or(false)).cloned().collect();
        let _ = app.emit("screen-capturing", true);
        for w in &visible {
            let _ = w.hide();
        }
        // Da tiempo a Windows a redibujar sin nuestras ventanas.
        tokio::time::sleep(Duration::from_millis(280)).await;
        let result = tauri::async_runtime::spawn_blocking(move || vision::capture(target))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
        for w in &visible {
            let _ = w.show();
        }
        let _ = app.emit("screen-capturing", false);
        if let Some(state) = app.try_state::<AppState>() {
            state.db.log(
                "pantalla",
                if result.is_ok() {
                    "Tomaste una captura (aún no enviada)"
                } else {
                    "Captura fallida"
                },
            );
        }
        result
    }
}

impl ScreenCapturer for AppCapturer {
    fn capture(
        &self,
        target: Target,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<image::RgbaImage, String>> + Send + '_>> {
        Box::pin(Self::capture_now(&self.app, target))
    }

    fn find_window(&self, title: &str) -> Option<u32> {
        let t = title.to_lowercase();
        vision::sources()
            .ok()?
            .windows
            .into_iter()
            .find(|w| w.title.to_lowercase().contains(&t) || w.app.to_lowercase().contains(&t))
            .map(|w| w.id)
    }
}

#[tauri::command]
pub async fn screen_sources() -> Result<vision::Sources, String> {
    tauri::async_runtime::spawn_blocking(vision::sources)
        .await
        .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureResult {
    capture_id: String,
    preview: String,
    width: u32,
    height: u32,
}

/// Toma una captura y devuelve una vista previa. NO se envía a ningún sitio.
#[tauri::command]
pub async fn screen_capture(app: AppHandle, state: State<'_, AppState>, target: Target) -> Result<CaptureResult, String> {
    if !state.current_settings().vision_enabled {
        return Err("La visión de pantalla está desactivada en Ajustes.".into());
    }
    let img = AppCapturer::capture_now(&app, target).await?;
    let preview = vision::preview(&img)?;
    let id = format!("cap-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let (width, height) = (img.width(), img.height());
    let mut caps = state.captures.lock().unwrap();
    caps.retain(|_, _| false); // como mucho una captura pendiente a la vez
    caps.insert(id.clone(), img);
    Ok(CaptureResult {
        capture_id: id,
        preview: preview.data_url(),
        width,
        height,
    })
}

/// Recorta (opcional) y prepara la captura para adjuntarla al mensaje.
#[tauri::command]
pub fn screen_finalize(state: State<'_, AppState>, capture_id: String, crop: Option<Rect>) -> Result<ImageData, String> {
    let img = state
        .captures
        .lock()
        .unwrap()
        .remove(&capture_id)
        .ok_or("La captura ya no está disponible. Toma otra.")?;
    vision::for_model(&img, crop)
}

#[tauri::command]
pub fn screen_discard(state: State<'_, AppState>, capture_id: String) {
    state.captures.lock().unwrap().remove(&capture_id);
}

// ---------------------------------------------------------------- adjuntos

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentOut {
    name: String,
    /// "text" | "image"
    kind: &'static str,
    text: Option<String>,
    image: Option<ImageData>,
    note: Option<String>,
}

/// Máximo de caracteres de un documento adjunto directamente al chat.
const MAX_ATTACH_CHARS: usize = 60_000;

/// Prepara un archivo que TÚ elegiste en el diálogo: extrae el texto (PDF,
/// Word, texto) o reduce la imagen, todo en tu equipo. Nada se envía hasta
/// que mandas el mensaje.
#[tauri::command]
pub async fn prepare_attachment(path: String) -> Result<AttachmentOut, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let p = std::path::PathBuf::from(&path);
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "archivo".into());
        if crate::tools::files::is_blocked(&p) {
            return Err("Por seguridad no adjunto programas ni scripts.".to_string());
        }
        match crate::docs::kind_of(&p) {
            crate::docs::DocKind::Image => Ok(AttachmentOut { name, kind: "image", text: None, image: Some(vision::load_file(&p)?), note: None }),
            crate::docs::DocKind::Unsupported => Err(format!("«{name}» no es compatible. Puedes adjuntar texto, código, PDF, Word o imágenes.")),
            _ => {
                let doc = crate::docs::extract(&p)?;
                let total = doc.text.chars().count();
                let (text, note) = if total > MAX_ATTACH_CHARS {
                    (
                        doc.text.chars().take(MAX_ATTACH_CHARS).collect::<String>(),
                        Some(format!(
                            "Documento largo: adjunté las primeras {MAX_ATTACH_CHARS} de {total} letras. Para leerlo completo, guárdalo en una carpeta autorizada y pide «lee el documento {name}»."
                        )),
                    )
                } else {
                    (doc.text, None)
                };
                Ok(AttachmentOut { name, kind: "text", text: Some(text), image: None, note })
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
