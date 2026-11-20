//! CANELA AI — núcleo nativo (Tauri 2).

mod ai;
mod commands;
mod data_cmds;
mod db;
mod docs;
mod fullscreen;
mod jobs;
mod media;
mod memdiag;
mod model_cmds;
mod privacy;
mod reminders;
mod resources;
mod router;
#[cfg(test)]
mod router_eval;
mod secrets;
mod settings;
mod system;
mod tools;
mod vision;
mod vision_cmds;
mod voice;
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
/// Ctrl+Alt+V: hablar con Canela (pulsa otra vez para terminar o interrumpirla).
fn shortcut_voice() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyV)
}
/// Ctrl+Alt+Espacio: abrir/cerrar el panel de asistente.
fn shortcut_toggle_panel() -> Shortcut {
    Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Space)
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle_pet = MenuItem::with_id(app, "toggle_pet", "Mostrar u ocultar a Canela   (Ctrl+Alt+C)", true, None::<&str>)?;
    let panel = MenuItem::with_id(app, "panel", "Abrir asistente   (Ctrl+Alt+Espacio)", true, None::<&str>)?;
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
            let enabled = app.state::<AppState>().current_settings().hide_in_fullscreen;
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
            } else if shortcut == &shortcut_voice() {
                // El panel coordina la conversación por voz (también si está oculto).
                let _ = app.emit_to(windows::PANEL, "voice-toggle", ());
            } else if shortcut == &shortcut_toggle_panel() {
                windows::show_pet(app);
                windows::toggle_panel(app);
            }
        })
        .build();

    tauri::Builder::default()
        .plugin(shortcuts)
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .setup(|app| {
            let config_dir = app.path().app_config_dir()?;
            let db_path = app.path().app_data_dir()?.join("canela.db");
            let db = match db::Db::open(&db_path) {
                Ok(db) => db,
                Err(e) => {
                    // Sin base de datos Canela sigue funcionando, pero sin memoria persistente.
                    eprintln!("[canela] {e}; usando memoria temporal");
                    db::Db::open_in_memory().expect("SQLite en memoria")
                }
            };
            let data_dir = app.path().app_data_dir()?;
            let mut known_dirs = Vec::new();
            for (alias, dir) in [
                ("documentos", app.path().document_dir()),
                ("descargas", app.path().download_dir()),
                ("escritorio", app.path().desktop_dir()),
            ] {
                if let Ok(d) = dir {
                    known_dirs.push((alias.to_string(), d));
                }
            }
            let job_app = app.handle().clone();
            let jobs = jobs::Jobs::new(move |e| match e {
                jobs::JobEvent::Changed(list) => {
                    let _ = job_app.emit("jobs-changed", list);
                }
                jobs::JobEvent::Finished(info) => {
                    let _ = job_app.emit("job-finished", info);
                }
            });
            // Olvida los recuerdos temporales que ya caducaron.
            if let Ok(n) = db.memory_purge_expired() {
                if n > 0 {
                    db.log("memoria", &format!("Olvidé {n} recuerdos temporales caducados"));
                }
            }
            app.manage(AppState::new(config_dir, data_dir, std::sync::Arc::new(db), jobs, known_dirs));
            build_tray(app.handle())?;
            spawn_fullscreen_watch(app.handle().clone());
            let at_login = std::env::args().any(|a| a == "--autostart");
            reminders::spawn(app.handle().clone(), at_login);
            resources::spawn(app.handle().clone());
            // «Hola, Canela» sólo si el usuario la dejó activada.
            if let Err(e) = voice::wake_start_if_enabled(app.handle()) {
                eprintln!("[canela] «Hola, Canela» no se pudo iniciar: {e}");
            }

            // Si otra app ya usa el atajo, Canela sigue funcionando sin él.
            for sc in [shortcut_toggle_pet(), shortcut_toggle_panel(), shortcut_voice()] {
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
            commands::confirm_action,
            commands::jobs_list,
            commands::job_cancel,
            commands::allowed_folders,
            voice::voice_devices,
            voice::voice_start,
            voice::voice_stop,
            voice::voice_cancel,
            voice::tts_cloud_speak,
            voice::tts_cloud_stop,
            voice::tts_local_speak,
            voice::wake_set,
            voice::wake_status,
            voice::wake_pause,
            voice::local_voice_status,
            voice::local_voice_download,
            voice::local_voice_cancel,
            voice::local_voice_delete,
            vision_cmds::screen_sources,
            vision_cmds::screen_capture,
            vision_cmds::screen_finalize,
            vision_cmds::screen_discard,
            vision_cmds::prepare_attachment,
            model_cmds::model_info,
            model_cmds::ollama_status,
            resources::memory_status,
            resources::memory_measure_now,
            resources::memory_release,
            resources::memory_export,
            resources::ai_preload,
            router::route_decide,
            router::spend_summary,
            model_cmds::summarize_conversation,
            data_cmds::memory_set_temporary,
            data_cmds::memory_list,
            data_cmds::memory_add,
            data_cmds::memory_update,
            data_cmds::memory_delete,
            data_cmds::memory_clear,
            data_cmds::task_list,
            data_cmds::task_add,
            data_cmds::task_update,
            data_cmds::task_set_done,
            data_cmds::task_delete,
            data_cmds::activity_list,
            data_cmds::conversation_save,
            data_cmds::conversation_list,
            data_cmds::conversation_get,
            data_cmds::conversation_delete,
            data_cmds::conversation_clear,
            data_cmds::conversation_context,
            data_cmds::doc_index_stats,
            data_cmds::doc_index_clear,
            data_cmds::routine_list,
            data_cmds::routine_delete,
        ])
        .run(tauri::generate_context!())
        .expect("error al iniciar Canela AI");
}
