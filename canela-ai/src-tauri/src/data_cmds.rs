//! Comandos de datos locales: memoria, tareas, actividad y conversaciones.
//! Cada cambio emite `data-changed` para que las ventanas se actualicen.

use crate::commands::AppState;
use crate::db::{Activity, ConversationMeta, Memory, Task, TaskFilter, TaskInput};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

#[derive(Clone, Serialize)]
pub struct DataChanged {
    pub kind: &'static str,
}

pub fn notify(app: &AppHandle, kind: &'static str) {
    let _ = app.emit("data-changed", DataChanged { kind });
}

// ---------------------------------------------------------------- memoria

#[tauri::command]
pub fn memory_list(
    state: State<'_, AppState>,
    kind: Option<String>,
    query: Option<String>,
) -> Result<Vec<Memory>, String> {
    state.db.memory_list(kind.as_deref(), query.as_deref(), 500)
}

#[tauri::command]
pub fn memory_add(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: String,
    content: String,
) -> Result<Memory, String> {
    let m = state.db.memory_add(&kind, &content, "usuario")?;
    state
        .db
        .log("memoria", &format!("Guardaste un recuerdo: {}", m.content));
    notify(&app, "memory");
    Ok(m)
}

#[tauri::command]
pub fn memory_update(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    kind: Option<String>,
    content: Option<String>,
) -> Result<Memory, String> {
    let m = state
        .db
        .memory_update(id, kind.as_deref(), content.as_deref())?;
    notify(&app, "memory");
    Ok(m)
}

#[tauri::command]
pub fn memory_delete(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<bool, String> {
    let ok = state.db.memory_delete(id)?;
    if ok {
        state
            .db
            .log("memoria", &format!("Borraste el recuerdo #{id}"));
        notify(&app, "memory");
    }
    Ok(ok)
}

#[tauri::command]
pub fn memory_clear(app: AppHandle, state: State<'_, AppState>) -> Result<usize, String> {
    let n = state.db.memory_clear()?;
    state.db.log(
        "memoria",
        &format!("Borraste toda la memoria ({n} recuerdos)"),
    );
    notify(&app, "memory");
    Ok(n)
}

// ----------------------------------------------------------------- tareas

#[tauri::command]
pub fn task_list(state: State<'_, AppState>, filter: TaskFilter) -> Result<Vec<Task>, String> {
    state.db.task_list(filter)
}

#[tauri::command]
pub fn task_add(
    app: AppHandle,
    state: State<'_, AppState>,
    input: TaskInput,
) -> Result<Task, String> {
    let t = state.db.task_add(&input)?;
    state.db.log("tarea", &format!("Nueva tarea: {}", t.title));
    notify(&app, "tasks");
    Ok(t)
}

#[tauri::command]
pub fn task_update(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    input: TaskInput,
) -> Result<Task, String> {
    let t = state.db.task_update(id, &input)?;
    state.db.log("tarea", &format!("Editaste: {}", t.title));
    notify(&app, "tasks");
    Ok(t)
}

#[tauri::command]
pub fn task_set_done(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    done: bool,
) -> Result<Task, String> {
    let t = state.db.task_set_done(id, done)?;
    state.db.log(
        "tarea",
        &format!(
            "{}: {}",
            if done { "Completada" } else { "Reabierta" },
            t.title
        ),
    );
    notify(&app, "tasks");
    Ok(t)
}

#[tauri::command]
pub fn task_delete(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<bool, String> {
    let title = state.db.task_get(id)?.map(|t| t.title).unwrap_or_default();
    let ok = state.db.task_delete(id)?;
    if ok {
        state.db.log("tarea", &format!("Eliminaste: {title}"));
        notify(&app, "tasks");
    }
    Ok(ok)
}

// -------------------------------------------------------------- actividad

#[tauri::command]
pub fn activity_list(state: State<'_, AppState>) -> Result<Vec<Activity>, String> {
    state.db.activity_list(100)
}

// --------------------------------------------------------- conversaciones

#[tauri::command]
pub fn conversation_save(
    state: State<'_, AppState>,
    id: String,
    title: String,
    data: String,
) -> Result<(), String> {
    if !state.current_settings().save_conversations {
        return Ok(());
    }
    if data.len() > 5_000_000 {
        return Err("La conversación es demasiado grande para guardarla.".into());
    }
    state.db.conversation_save(&id, &title, &data)
}

#[tauri::command]
pub fn conversation_list(state: State<'_, AppState>) -> Result<Vec<ConversationMeta>, String> {
    state.db.conversation_list()
}

#[tauri::command]
pub fn conversation_get(state: State<'_, AppState>, id: String) -> Result<Option<String>, String> {
    state.db.conversation_get(&id)
}

#[tauri::command]
pub fn conversation_delete(state: State<'_, AppState>, id: String) -> Result<bool, String> {
    state.db.conversation_delete(&id)
}

#[tauri::command]
pub fn conversation_clear(state: State<'_, AppState>) -> Result<usize, String> {
    let n = state.db.conversation_clear()?;
    state.db.log(
        "privacidad",
        &format!("Borraste el historial de chats ({n})"),
    );
    Ok(n)
}

/// Contexto fijo para una conversación nueva: fecha + memoria + resumen de tareas.
#[tauri::command]
pub fn conversation_context(state: State<'_, AppState>) -> Result<String, String> {
    let now = chrono::Local::now();
    let days = [
        "lunes",
        "martes",
        "miércoles",
        "jueves",
        "viernes",
        "sábado",
        "domingo",
    ];
    use chrono::Datelike;
    let weekday = days[now.weekday().num_days_from_monday() as usize];
    Ok(format!(
        "Fecha y hora al empezar esta conversación: {weekday} {} (hora local).\n{}",
        now.format("%Y-%m-%d %H:%M"),
        state.db.context_summary()?
    ))
}
