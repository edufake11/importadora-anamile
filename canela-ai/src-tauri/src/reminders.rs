//! Recordatorios de tareas y saludo al iniciar.

use crate::commands::AppState;
use crate::db::{now_str, TaskFilter};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

#[derive(Clone, Serialize)]
struct Mood {
    mood: &'static str,
    bubble: String,
}

fn pet_say(app: &AppHandle, mood: &'static str, bubble: String) {
    let _ = app.emit_to(crate::windows::PET, "canela-mood", Mood { mood, bubble });
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        eprintln!("[canela] notificación: {e}");
    }
}

/// Texto del saludo según la hora y las tareas de hoy.
pub fn greeting(hour: u32, name: &str, today: &[String]) -> String {
    let hello = match hour {
        5..=11 => "¡Buenos días",
        12..=19 => "¡Buenas tardes",
        _ => "¡Buenas noches",
    };
    let who = if name.trim().is_empty() {
        String::new()
    } else {
        format!(", {}", name.trim())
    };
    match today.len() {
        0 => format!("{hello}{who}! No tienes tareas para hoy 🐾"),
        1 => format!("{hello}{who}! Hoy tienes 1 tarea: {}", today[0]),
        n => format!(
            "{hello}{who}! Hoy tienes {n} tareas: {}{}",
            today.iter().take(3).cloned().collect::<Vec<_>>().join(", "),
            if n > 3 { "…" } else { "" }
        ),
    }
}

/// Comprueba los recordatorios cada 30 s y saluda al iniciar.
pub fn spawn(app: AppHandle, launched_at_login: bool) {
    tauri::async_runtime::spawn(async move {
        // Saludo, unos segundos después de arrancar.
        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
        {
            let state = app.state::<AppState>();
            let settings = state.current_settings();
            if settings.greet_on_startup {
                let today: Vec<String> = state
                    .db
                    .task_list(TaskFilter::Today)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|t| t.title)
                    .collect();
                use chrono::Timelike;
                let text = greeting(chrono::Local::now().hour(), &settings.user_name, &today);
                pet_say(&app, "happy", text.clone());
                if launched_at_login && !today.is_empty() {
                    notify(&app, "Canela", &text);
                }
            }
        }
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tick.tick().await;
            let state = app.state::<AppState>();
            let due = match state.db.tasks_due_for_reminder(&now_str()) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("[canela] recordatorios: {e}");
                    continue;
                }
            };
            for t in due {
                notify(&app, "Recordatorio de Canela", &t.title);
                pet_say(&app, "listen", format!("⏰ Recordatorio: {}", t.title));
                let _ = state.db.task_mark_reminded(t.id);
                state.db.log("recordatorio", &format!("Te recordé: {}", t.title));
                crate::data_cmds::notify(&app, "tasks");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::greeting;

    #[test]
    fn greetings() {
        assert_eq!(greeting(8, "Eduardo", &[]), "¡Buenos días, Eduardo! No tienes tareas para hoy 🐾");
        assert!(greeting(15, "", &["Estudiar".into()]).starts_with("¡Buenas tardes! Hoy tienes 1 tarea: Estudiar"));
        let many: Vec<String> = (1..=5).map(|i| format!("t{i}")).collect();
        assert!(greeting(22, "Ana", &many).ends_with("t1, t2, t3…"));
    }
}
