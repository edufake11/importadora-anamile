//! Gestión de las dos ventanas:
//! - `pet`: Canela, transparente, sin bordes, siempre encima.
//! - `panel`: Modo asistente (chat + ajustes), se muestra junto a Canela.

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow};

pub const PET: &str = "pet";
pub const PANEL: &str = "panel";
/// Tamaño lógico inicial del panel (igual que en tauri.conf.json).
const PANEL_SIZE: (f64, f64) = (440.0, 640.0);

fn win(app: &AppHandle, label: &str) -> Option<WebviewWindow> {
    app.get_webview_window(label)
}

pub fn toggle_pet(app: &AppHandle) {
    if let Some(w) = win(app, PET) {
        let visible = w.is_visible().unwrap_or(true);
        let _ = if visible { w.hide() } else { w.show() };
        let _ = app.emit("pet-visibility", !visible);
    }
}

pub fn show_pet(app: &AppHandle) {
    if let Some(w) = win(app, PET) {
        let _ = w.show();
        let _ = app.emit("pet-visibility", true);
    }
}

pub fn hide_panel(app: &AppHandle) {
    if let Some(w) = win(app, PANEL) {
        let _ = w.hide();
    }
}

pub fn toggle_panel(app: &AppHandle) {
    if let Some(w) = win(app, PANEL) {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            show_panel(app);
        }
    }
}

/// Muestra el panel a un lado de Canela (izquierda si hay espacio, si no derecha).
pub fn show_panel(app: &AppHandle) {
    let Some(panel) = win(app, PANEL) else { return };
    if !panel.is_visible().unwrap_or(false) {
        if let Some(pos) = panel_position(app, &panel) {
            let _ = panel.set_position(pos);
        }
    }
    let _ = panel.show();
    let _ = panel.unminimize();
    let _ = panel.set_focus();
    let _ = app.emit_to(PANEL, "panel-shown", ());
}

fn panel_position(app: &AppHandle, panel: &WebviewWindow) -> Option<PhysicalPosition<i32>> {
    let pet = win(app, PET)?;
    let pet_pos = pet.outer_position().ok()?;
    let pet_size = pet.outer_size().ok()?;
    let size = panel.outer_size().ok()?;
    let monitor = pet.current_monitor().ok()??;
    let area = monitor.work_area();
    let (ax, ay) = (area.position.x, area.position.y);
    let (aw, ah) = (area.size.width as i32, area.size.height as i32);
    let (mut w, mut h) = (size.width as i32, size.height as i32);
    // En Linux una ventana oculta aún no tiene tamaño real: usa el de tauri.conf.json.
    if w < 300 || h < 300 {
        let sf = panel.scale_factor().unwrap_or(1.0);
        w = (PANEL_SIZE.0 * sf) as i32;
        h = (PANEL_SIZE.1 * sf) as i32;
    }
    let gap = 12;

    let left = pet_pos.x - w - gap;
    let right = pet_pos.x + pet_size.width as i32 + gap;
    let x = if left >= ax {
        left
    } else if right + w <= ax + aw {
        right
    } else {
        ax + (aw - w) / 2
    };
    let y = (pet_pos.y + pet_size.height as i32 - h).clamp(ay, (ay + ah - h).max(ay));
    Some(PhysicalPosition::new(x, y))
}
