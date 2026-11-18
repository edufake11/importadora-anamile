//! Visión de pantalla: capturas SÓLO cuando las pides, con vista previa.
//!
//! - Pantalla completa (cualquier monitor) o una ventana concreta; la región
//!   se recorta en la vista previa.
//! - Canela y su panel se ocultan durante la captura para no salir en ella.
//! - Las ventanas que parecen sensibles (gestores de contraseñas, bancos…)
//!   no se ofrecen ni se capturan.
//! - La imagen queda en memoria hasta que la envías o la descartas.

use base64::Engine;
use image::{imageops::FilterType, DynamicImage, RgbaImage};
use serde::{Deserialize, Serialize};

/// Imagen lista para enviar a un modelo multimodal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImageData {
    pub media_type: String,
    /// Base64 sin prefijo.
    pub data: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
}

impl ImageData {
    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.media_type, self.data)
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Lado máximo y megapíxeles recomendados para modelos de visión.
pub const MAX_SIDE: u32 = 1568;
pub const MAX_PIXELS: u64 = 1_150_000;

const SENSITIVE: [&str; 22] = [
    "contraseña",
    "password",
    "keepass",
    "1password",
    "bitwarden",
    "lastpass",
    "dashlane",
    "keeper",
    "credencial",
    "credential",
    "authenticator",
    "autenticador",
    "banco",
    "bank",
    "paypal",
    "tarjeta",
    "wallet",
    "billetera",
    "seguridad de windows",
    "windows security",
    "inicio de sesión",
    "sign in",
];

pub fn is_sensitive(title: &str, app: &str) -> bool {
    let t = format!("{} {}", title, app).to_lowercase();
    SENSITIVE.iter().any(|w| t.contains(w))
}

/// Recorta (opcional), reduce al tamaño recomendado y codifica en JPEG.
pub fn encode(img: &RgbaImage, crop: Option<Rect>, max_side: u32, max_pixels: u64, quality: u8) -> Result<ImageData, String> {
    let mut dynimg = DynamicImage::ImageRgba8(img.clone());
    if let Some(r) = crop {
        let (w, h) = (img.width(), img.height());
        if r.w < 8 || r.h < 8 || r.x >= w || r.y >= h {
            return Err("La región elegida es demasiado pequeña.".into());
        }
        dynimg = dynimg.crop_imm(r.x, r.y, r.w.min(w - r.x), r.h.min(h - r.y));
    }
    let (w, h) = (dynimg.width(), dynimg.height());
    let side_scale = max_side as f64 / w.max(h) as f64;
    let px_scale = (max_pixels as f64 / (w as f64 * h as f64)).sqrt();
    let scale = side_scale.min(px_scale).min(1.0);
    if scale < 1.0 {
        let nw = ((w as f64 * scale).round() as u32).max(1);
        let nh = ((h as f64 * scale).round() as u32).max(1);
        dynimg = dynimg.resize_exact(nw, nh, FilterType::Triangle);
    }
    let rgb = dynimg.to_rgb8();
    let mut out = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    enc.encode_image(&rgb).map_err(|e| format!("No pude preparar la imagen: {e}"))?;
    Ok(ImageData {
        media_type: "image/jpeg".into(),
        data: base64::engine::general_purpose::STANDARD.encode(&out),
        width: rgb.width(),
        height: rgb.height(),
    })
}

pub fn for_model(img: &RgbaImage, crop: Option<Rect>) -> Result<ImageData, String> {
    encode(img, crop, MAX_SIDE, MAX_PIXELS, 82)
}

pub fn preview(img: &RgbaImage) -> Result<ImageData, String> {
    encode(img, None, 960, 960 * 600, 70)
}

/// Lee una imagen del disco (PNG/JPG) y la prepara para el modelo.
pub fn load_file(path: &std::path::Path) -> Result<ImageData, String> {
    let md = std::fs::metadata(path).map_err(|e| format!("No pude leer la imagen: {e}"))?;
    if md.len() > 40 * 1024 * 1024 {
        return Err("La imagen es demasiado grande (máximo 40 MB).".into());
    }
    let img = image::open(path).map_err(|e| format!("No pude abrir la imagen: {e}"))?.to_rgba8();
    for_model(&img, None)
}

// ----------------------------------------------------------- captura real

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    pub id: u32,
    pub title: String,
    pub app: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Sources {
    pub monitors: Vec<MonitorInfo>,
    pub windows: Vec<WindowInfo>,
}

pub fn sources() -> Result<Sources, String> {
    let monitors = xcap::Monitor::all()
        .map_err(|e| format!("No pude listar las pantallas: {e}"))?
        .iter()
        .enumerate()
        .map(|(i, m)| MonitorInfo {
            id: m.id().unwrap_or(i as u32),
            name: m
                .friendly_name()
                .or_else(|_| m.name())
                .unwrap_or_else(|_| format!("Pantalla {}", i + 1)),
            width: m.width().unwrap_or(0),
            height: m.height().unwrap_or(0),
            primary: m.is_primary().unwrap_or(false),
        })
        .collect();
    let me = std::process::id();
    let windows = xcap::Window::all()
        .unwrap_or_default()
        .iter()
        .filter_map(|w| {
            let title = w.title().ok()?;
            let app = w.app_name().unwrap_or_default();
            let ok = !title.trim().is_empty()
                && w.pid().ok() != Some(me)
                && !w.is_minimized().unwrap_or(false)
                && w.width().unwrap_or(0) > 120
                && w.height().unwrap_or(0) > 80
                && !is_sensitive(&title, &app);
            ok.then(|| WindowInfo {
                id: w.id().unwrap_or(0),
                title,
                app,
            })
        })
        .take(40)
        .collect();
    Ok(Sources { monitors, windows })
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase", tag = "kind", content = "id")]
pub enum Target {
    Monitor(u32),
    Window(u32),
    /// Monitor donde está la ventana activa (o el principal).
    Active,
}

/// Captura. Debe llamarse con Canela y el panel ya ocultos.
pub fn capture(target: Target) -> Result<RgbaImage, String> {
    match target {
        Target::Monitor(id) => {
            let m = xcap::Monitor::all()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|m| m.id().ok() == Some(id))
                .ok_or("Esa pantalla ya no está disponible.")?;
            m.capture_image().map_err(|e| format!("No pude capturar la pantalla: {e}"))
        }
        Target::Window(id) => {
            let w = xcap::Window::all()
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|w| w.id().ok() == Some(id))
                .ok_or("Esa ventana ya no está abierta.")?;
            if is_sensitive(&w.title().unwrap_or_default(), &w.app_name().unwrap_or_default()) {
                return Err("Esa ventana parece contener información privada; no la capturo.".into());
            }
            w.capture_image().map_err(|e| format!("No pude capturar la ventana: {e}"))
        }
        Target::Active => {
            let windows = xcap::Window::all().unwrap_or_default();
            if let Some(f) = windows.iter().find(|w| w.is_focused().unwrap_or(false)) {
                if is_sensitive(&f.title().unwrap_or_default(), &f.app_name().unwrap_or_default()) {
                    return Err(
                        "La ventana activa parece contener información privada (contraseñas, banco…); no capturo la pantalla.".into(),
                    );
                }
                if let Ok(m) = f.current_monitor() {
                    return m.capture_image().map_err(|e| format!("No pude capturar la pantalla: {e}"));
                }
            }
            let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
            let m = monitors
                .iter()
                .find(|m| m.is_primary().unwrap_or(false))
                .or(monitors.first())
                .ok_or("No encontré ninguna pantalla.")?;
            m.capture_image().map_err(|e| format!("No pude capturar la pantalla: {e}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x % 255) as u8, (y % 255) as u8, 128, 255]))
    }

    #[test]
    fn downscales_to_model_limits() {
        let d = for_model(&img(3840, 2160), None).unwrap();
        assert!(d.width <= MAX_SIDE && d.height <= MAX_SIDE);
        assert!((d.width as u64 * d.height as u64) <= MAX_PIXELS + 2000);
        assert_eq!(d.media_type, "image/jpeg");
        let bytes = base64::engine::general_purpose::STANDARD.decode(&d.data).unwrap();
        assert_eq!(&bytes[..2], &[0xFF, 0xD8], "JPEG válido");
    }

    #[test]
    fn small_images_are_not_enlarged_and_crop_works() {
        let d = for_model(&img(400, 300), None).unwrap();
        assert_eq!((d.width, d.height), (400, 300));
        let c = for_model(
            &img(1000, 800),
            Some(Rect {
                x: 100,
                y: 100,
                w: 300,
                h: 200,
            }),
        )
        .unwrap();
        assert_eq!((c.width, c.height), (300, 200));
        assert!(for_model(
            &img(100, 100),
            Some(Rect {
                x: 500,
                y: 0,
                w: 50,
                h: 50
            })
        )
        .is_err());
    }

    #[test]
    fn sensitive_windows_are_detected() {
        assert!(is_sensitive("Bitwarden - Vault", ""));
        assert!(is_sensitive("Mi Banco | Inicio", "chrome"));
        assert!(!is_sensitive("main.rs - canela-ai - Visual Studio Code", "Code"));
    }

    #[test]
    fn target_serialization() {
        assert_eq!(
            serde_json::from_str::<Target>(r#"{"kind":"monitor","id":3}"#).unwrap(),
            Target::Monitor(3)
        );
        assert_eq!(serde_json::from_str::<Target>(r#"{"kind":"active"}"#).unwrap(), Target::Active);
    }
}
