//! CANELA AI — núcleo nativo (Tauri 2).

mod ai;
mod commands;
mod fullscreen;
mod secrets;
mod settings;
mod windows;

use commands::AppState;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// Ctrl+Alt+C: mostrar/ocultar a Canela.
fn shortcut_toggle_pet() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyC)
}
/// Ctrl+Alt+Espacio: abrir/cerrar el panel de asistente.
fn shortcut_toggle_panel() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space)
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle_pet = MenuItem::with_id(
        app,
        "toggle_pet",
        "Mostrar u ocultar a Canela   (Ctrl+Alt+C)",
        true,
        None::<&str>,
    )?;
    let panel = MenuItem::with_id(
        app,
        "panel",
        "Abrir asistente   (Ctrl+Alt+Espacio)",
        true,
        None::<&str>,
    )?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Salir de Canela AI", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle_pet, &panel, &sep, &quit])?;

    let mut tray = TrayIconBuilder::with_id("canela-tray")
        .tooltip("Canela AI")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle_pet" => windows::toggle_pet(app),
            "panel" => windows::show_panel(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                windows::show_pet(app);
                windows::toggle_panel(app);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Oculta a Canela mientras otra app está a pantalla completa (juegos, videos)
/// y la vuelve a mostrar al salir. Consulta ligera cada 1,5 s.
fn spawn_fullscreen_watch(app: AppHandle) {
    if !cfg!(windows) {
        return;
    }
    std::thread::spawn(move || {
        let mut hidden_by_us = false;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            let enabled = app
                .state::<AppState>()
                .current_settings()
                .hide_in_fullscreen;
            let fullscreen = enabled && fullscreen::foreground_is_fullscreen();
            let Some(pet) = app.get_webview_window(windows::PET) else {
                continue;
            };
            if fullscreen && !hidden_by_us && pet.is_visible().unwrap_or(false) {
                hidden_by_us = pet.hide().is_ok();
                let _ = app.emit("pet-visibility", false);
            } else if !fullscreen && hidden_by_us {
                hidden_by_us = false;
                let _ = pet.show();
                let _ = app.emit("pet-visibility", true);
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let shortcuts = tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            if shortcut == &shortcut_toggle_pet() {
                windows::toggle_pet(app);
            } else if shortcut == &shortcut_toggle_panel() {
                windows::show_pet(app);
                windows::toggle_panel(app);
            }
        })
        .build();

    tauri::Builder::default()
        .plugin(shortcuts)
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            app.manage(AppState::new(config_dir));
            build_tray(app.handle())?;
            spawn_fullscreen_watch(app.handle().clone());

            // Si otra app ya usa el atajo, Canela sigue funcionando sin él.
            for sc in [shortcut_toggle_pet(), shortcut_toggle_panel()] {
                if let Err(e) = app.global_shortcut().register(sc) {
                    eprintln!("[canela] no se pudo registrar el atajo {sc:?}: {e}");
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Cerrar una ventana la oculta: Canela sigue viva en la bandeja.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::api_key_status,
            commands::set_api_key,
            commands::delete_api_key,
            commands::chat_send,
            commands::chat_cancel,
            commands::show_panel,
            commands::hide_panel,
            commands::toggle_pet,
            commands::quit_app,
        ])
        .run(tauri::generate_context!())
        .expect("error al iniciar Canela AI");
}
