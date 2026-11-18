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
