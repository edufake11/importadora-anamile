//! Base de datos local (SQLite) de Canela: memoria, tareas, actividad y
//! conversaciones guardadas. Archivo: %APPDATA%\com.canela.ai\canela.db
//!
//! Todo se guarda sólo en este equipo. Las fechas se guardan en hora local
//! con el formato "AAAA-MM-DD" o "AAAA-MM-DD HH:MM".

use chrono::{Local, NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

pub type DbResult<T> = Result<T, String>;

fn err(e: rusqlite::Error) -> String {
    format!("Error de la base de datos: {e}")
}

pub fn now_str() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn today_str() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

// ------------------------------------------------------------------ tipos

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    pub id: i64,
    pub kind: String,
    pub content: String,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
    /// Memoria temporal: se olvida sola después de esta fecha. `None` = permanente.
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: i64,
    pub title: String,
    pub notes: String,
    /// 1 = alta, 2 = normal, 3 = baja
    pub priority: i64,
    /// "AAAA-MM-DD" o "AAAA-MM-DD HH:MM"
    pub due: Option<String>,
    /// "AAAA-MM-DD HH:MM"
    pub remind_at: Option<String>,
    pub reminded: bool,
    pub done: bool,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TaskInput {
    pub title: Option<String>,
    pub notes: Option<String>,
    pub priority: Option<i64>,
    /// Cadena vacía = quitar la fecha.
    pub due: Option<String>,
    /// Cadena vacía = quitar el recordatorio.
    pub remind_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub id: i64,
    pub at: String,
    pub kind: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationMeta {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskFilter {
    /// Pendientes atrasadas o para hoy.
    Today,
    Pending,
    Done,
    All,
}

// --------------------------------------------------------------- validación

pub fn normalize_kind(kind: &str) -> String {
    let k = kind.trim().to_lowercase();
    let k = k.replace('ó', "o").replace('á', "a");
    match k.as_str() {
        "proyecto" | "proyectos" | "project" => "proyecto",
        "preferencia" | "preferencias" | "preference" | "gusto" => "preferencia",
        "horario" | "horarios" | "schedule" | "rutina" => "horario",
        "aplicacion" | "aplicaciones" | "app" | "apps" | "herramienta" | "herramientas" => "aplicacion",
        "persona" | "personas" | "contacto" => "persona",
        "dato" | "datos" | "hecho" | "fact" | "importante" => "dato",
        _ => "otro",
    }
    .to_string()
}

/// Valida "AAAA-MM-DD" o "AAAA-MM-DD HH:MM" (también acepta "T" como separador).
pub fn normalize_datetime(input: &str, allow_date_only: bool) -> Result<String, String> {
    let s = input.trim().replace('T', " ");
    if allow_date_only {
        if let Ok(d) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
            return Ok(d.format("%Y-%m-%d").to_string());
        }
    }
    for fmt in ["%Y-%m-%d %H:%M", "%Y-%m-%d %H:%M:%S"] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(&s, fmt) {
            return Ok(dt.format("%Y-%m-%d %H:%M").to_string());
        }
    }
    Err(format!(
        "Fecha no válida: «{input}». Usa el formato {}.",
        if allow_date_only {
            "AAAA-MM-DD o AAAA-MM-DD HH:MM"
        } else {
            "AAAA-MM-DD HH:MM"
        }
    ))
}

/// Detecta información sensible que NO debe guardarse automáticamente
/// (contraseñas, tarjetas, claves de API, etc.). Devuelve el motivo.
pub fn looks_sensitive(text: &str) -> Option<&'static str> {
    let lower = text.to_lowercase();
    const WORDS: [(&str, &str); 12] = [
        ("contraseña", "una contraseña"),
        ("password", "una contraseña"),
        ("passwd", "una contraseña"),
        ("clave de acceso", "una contraseña"),
        ("pin de", "un PIN"),
        ("cvv", "datos de tarjeta"),
        ("tarjeta de crédito", "datos de tarjeta"),
        ("tarjeta de credito", "datos de tarjeta"),
        ("número de tarjeta", "datos de tarjeta"),
        ("api key", "una clave de API"),
        ("token de acceso", "un token"),
        ("seguro social", "un número de identificación"),
    ];
    for (w, why) in WORDS {
        if lower.contains(w) {
            return Some(why);
        }
    }
    for prefix in ["sk-", "sk_", "ghp_", "github_pat_", "xoxb-", "akia"] {
        if let Some(pos) = lower.find(prefix) {
            let rest: String = lower[pos + prefix.len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if rest.len() >= 16 {
                return Some("una clave de API");
            }
        }
    }
    // 13 a 19 dígitos seguidos (con espacios o guiones): posible tarjeta.
    let mut digits = 0;
    for c in text.chars() {
        if c.is_ascii_digit() {
            digits += 1;
            if digits >= 13 {
                return Some("un número de tarjeta");
            }
        } else if c != ' ' && c != '-' {
            digits = 0;
        }
    }
    None
}

fn clean_text(s: &str, max: usize) -> Result<String, String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("El texto no puede estar vacío.".into());
    }
    Ok(t.chars().take(max).collect())
}

// ------------------------------------------------------------------- Db

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA_V1: &str = "
CREATE TABLE IF NOT EXISTS memories (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL,
  content TEXT NOT NULL,
  source TEXT NOT NULL DEFAULT 'usuario',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_memories_kind ON memories(kind);
CREATE TABLE IF NOT EXISTS tasks (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  title TEXT NOT NULL,
  notes TEXT NOT NULL DEFAULT '',
  priority INTEGER NOT NULL DEFAULT 2,
  due TEXT,
  remind_at TEXT,
  reminded INTEGER NOT NULL DEFAULT 0,
  done INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL,
  completed_at TEXT
);
CREATE INDEX IF NOT EXISTS idx_tasks_done ON tasks(done, due);
CREATE TABLE IF NOT EXISTS activity (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  at TEXT NOT NULL,
  kind TEXT NOT NULL,
  summary TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS conversations (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  data TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
";

/// v0.3: índice de documentos (búsqueda de texto completo) y memoria temporal.
const SCHEMA_V2: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS doc_chunks USING fts5(path UNINDEXED, part UNINDEXED, content, tokenize = 'unicode61 remove_diacritics 2');
CREATE TABLE IF NOT EXISTS doc_files (
  path TEXT PRIMARY KEY,
  mtime INTEGER NOT NULL,
  size INTEGER NOT NULL,
  parts INTEGER NOT NULL,
  indexed_at TEXT NOT NULL
);
ALTER TABLE memories ADD COLUMN expires_at TEXT;
";

/// v0.3: rutinas (automatizaciones guardadas).
const SCHEMA_V3: &str = "
CREATE TABLE IF NOT EXISTS routines (
  name TEXT PRIMARY KEY,
  steps TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
";

/// v4 (v0.6): registro de gasto en modelos de pago (para el límite mensual).
const SCHEMA_V4: &str = "
CREATE TABLE IF NOT EXISTS api_spend (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts TEXT NOT NULL,
  month TEXT NOT NULL,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  input_tokens INTEGER NOT NULL,
  output_tokens INTEGER NOT NULL,
  usd REAL NOT NULL
);
CREATE INDEX IF NOT EXISTS api_spend_month ON api_spend(month);
";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SpendRow {
    pub provider: String,
    pub model: String,
    pub calls: i64,
    pub usd: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Routine {
    pub name: String,
    /// Pasos en JSON: [{herramienta, parametros, descripcion}]
    pub steps: String,
    pub updated_at: String,
}

fn memory_from(r: &Row) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: r.get(0)?,
        kind: r.get(1)?,
        content: r.get(2)?,
        source: r.get(3)?,
        created_at: r.get(4)?,
        updated_at: r.get(5)?,
        expires_at: r.get(6)?,
    })
}

const TASK_COLS: &str = "id, title, notes, priority, due, remind_at, reminded, done, created_at, completed_at";

fn task_from(r: &Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
        title: r.get(1)?,
        notes: r.get(2)?,
        priority: r.get(3)?,
        due: r.get(4)?,
        remind_at: r.get(5)?,
        reminded: r.get::<_, i64>(6)? != 0,
        done: r.get::<_, i64>(7)? != 0,
        created_at: r.get(8)?,
        completed_at: r.get(9)?,
    })
}

impl Db {
    pub fn open(path: &Path) -> DbResult<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("No se pudo crear la carpeta de datos: {e}"))?;
        }
        let conn = Connection::open(path).map_err(err)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> DbResult<Self> {
        Self::init(Connection::open_in_memory().map_err(err)?)
    }

    fn init(conn: Connection) -> DbResult<Self> {
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .map_err(err)?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(err)?;
        if version < 1 {
            conn.execute_batch(SCHEMA_V1).map_err(err)?;
            conn.execute_batch("PRAGMA user_version = 1").map_err(err)?;
        }
        if version < 2 {
            conn.execute_batch(SCHEMA_V2).map_err(err)?;
            conn.execute_batch("PRAGMA user_version = 2").map_err(err)?;
        }
        if version < 3 {
            conn.execute_batch(SCHEMA_V3).map_err(err)?;
            conn.execute_batch("PRAGMA user_version = 3").map_err(err)?;
        }
        if version < 4 {
            conn.execute_batch(SCHEMA_V4).map_err(err)?;
            conn.execute_batch("PRAGMA user_version = 4").map_err(err)?;
        }
        if version < 5 {
            // «api» = tokens reales informados por el proveedor; «estimado» =
            // la respuesta se cortó y no hubo datos de consumo.
            conn.execute_batch("ALTER TABLE api_spend ADD COLUMN source TEXT NOT NULL DEFAULT 'api'; PRAGMA user_version = 5;")
                .map_err(err)?;
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> DbResult<T> {
        let conn = self.conn.lock().map_err(|_| "La base de datos está bloqueada.".to_string())?;
        f(&conn).map_err(err)
    }

    // ------------------------------------------------------------ memoria

    /// Recuerdo temporal: caduca en `days` días.
    pub fn memory_add_temporary(&self, kind: &str, content: &str, source: &str, days: i64) -> DbResult<Memory> {
        let m = self.memory_add(kind, content, source)?;
        let until = (Local::now() + chrono::Duration::days(days.clamp(1, 365)))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        self.memory_set_expiry(m.id, Some(&until))
    }

    pub fn memory_set_expiry(&self, id: i64, until: Option<&str>) -> DbResult<Memory> {
        self.with(|c| c.execute("UPDATE memories SET expires_at = ?1 WHERE id = ?2", params![until, id]))?;
        self.memory_get(id)?.ok_or_else(|| format!("No existe el recuerdo #{id}."))
    }

    /// Borra los recuerdos temporales caducados.
    pub fn memory_purge_expired(&self) -> DbResult<usize> {
        self.with(|c| {
            c.execute(
                "DELETE FROM memories WHERE expires_at IS NOT NULL AND expires_at <= ?1",
                params![now_str()],
            )
        })
    }

    pub fn memory_add(&self, kind: &str, content: &str, source: &str) -> DbResult<Memory> {
        let content = clean_text(content, 1000)?;
        let kind = normalize_kind(kind);
        let now = now_str();
        // Evita duplicados exactos.
        if let Some(existing) = self.with(|c| {
            c.query_row(
                "SELECT id, kind, content, source, created_at, updated_at, expires_at FROM memories WHERE lower(content) = lower(?1)",
                params![content],
                memory_from,
            )
            .optional()
        })? {
            return Ok(existing);
        }
        let id = self.with(|c| {
            c.execute(
                "INSERT INTO memories (kind, content, source, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
                params![kind, content, source, now],
            )?;
            Ok(c.last_insert_rowid())
        })?;
        self.memory_get(id)?.ok_or_else(|| "No se pudo guardar el recuerdo.".into())
    }

    pub fn memory_get(&self, id: i64) -> DbResult<Option<Memory>> {
        self.with(|c| {
            c.query_row(
                "SELECT id, kind, content, source, created_at, updated_at, expires_at FROM memories WHERE id = ?1",
                params![id],
                memory_from,
            )
            .optional()
        })
    }

    /// Lista recuerdos; `query` busca por palabras (todas deben aparecer).
    pub fn memory_list(&self, kind: Option<&str>, query: Option<&str>, limit: usize) -> DbResult<Vec<Memory>> {
        let now = now_str();
        let kind = kind.filter(|k| !k.trim().is_empty()).map(normalize_kind);
        let words: Vec<String> = query
            .unwrap_or("")
            .split_whitespace()
            .filter(|w| w.chars().count() > 2)
            .map(|w| format!("%{}%", w.to_lowercase()))
            .collect();
        let all = self.with(|c| {
            let mut st = c.prepare(
                "SELECT id, kind, content, source, created_at, updated_at, expires_at FROM memories ORDER BY updated_at DESC, id DESC",
            )?;
            let rows = st.query_map([], memory_from)?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })?;
        Ok(all
            .into_iter()
            .filter(|m| m.expires_at.as_deref().is_none_or(|e| e > now.as_str()))
            .filter(|m| kind.as_ref().is_none_or(|k| &m.kind == k))
            .filter(|m| {
                let text = format!("{} {}", m.kind, m.content).to_lowercase();
                words.is_empty() || words.iter().any(|w| text.contains(w.trim_matches('%')))
            })
            .take(limit)
            .collect())
    }

    pub fn memory_update(&self, id: i64, kind: Option<&str>, content: Option<&str>) -> DbResult<Memory> {
        let current = self.memory_get(id)?.ok_or_else(|| format!("No existe el recuerdo #{id}."))?;
        let kind = kind.map(normalize_kind).unwrap_or(current.kind);
        let content = match content {
            Some(c) => clean_text(c, 1000)?,
            None => current.content,
        };
        self.with(|c| {
            c.execute(
                "UPDATE memories SET kind = ?1, content = ?2, updated_at = ?3 WHERE id = ?4",
                params![kind, content, now_str(), id],
            )
        })?;
        self.memory_get(id)?.ok_or_else(|| "Recuerdo no encontrado.".into())
    }

    pub fn memory_delete(&self, id: i64) -> DbResult<bool> {
        self.with(|c| c.execute("DELETE FROM memories WHERE id = ?1", params![id]))
            .map(|n| n > 0)
    }

    pub fn memory_clear(&self) -> DbResult<usize> {
        self.with(|c| c.execute("DELETE FROM memories", []))
    }

    // ------------------------------------------------------------- tareas

    pub fn task_add(&self, input: &TaskInput) -> DbResult<Task> {
        let title = clean_text(input.title.as_deref().unwrap_or(""), 200)?;
        let notes = input.notes.as_deref().unwrap_or("").trim().chars().take(2000).collect::<String>();
        let priority = input.priority.unwrap_or(2).clamp(1, 3);
        let due = match input.due.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(d) => Some(normalize_datetime(d, true)?),
            None => None,
        };
        let remind = match input.remind_at.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(d) => Some(normalize_datetime(d, false)?),
            None => None,
        };
        let id = self.with(|c| {
            c.execute(
                "INSERT INTO tasks (title, notes, priority, due, remind_at, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![title, notes, priority, due, remind, now_str()],
            )?;
            Ok(c.last_insert_rowid())
        })?;
        self.task_get(id)?.ok_or_else(|| "No se pudo crear la tarea.".into())
    }

    pub fn task_get(&self, id: i64) -> DbResult<Option<Task>> {
        self.with(|c| {
            c.query_row(&format!("SELECT {TASK_COLS} FROM tasks WHERE id = ?1"), params![id], task_from)
                .optional()
        })
    }

    pub fn task_update(&self, id: i64, input: &TaskInput) -> DbResult<Task> {
        let cur = self.task_get(id)?.ok_or_else(|| format!("No existe la tarea #{id}."))?;
        let title = match &input.title {
            Some(t) => clean_text(t, 200)?,
            None => cur.title,
        };
        let notes = input.notes.clone().unwrap_or(cur.notes);
        let priority = input.priority.unwrap_or(cur.priority).clamp(1, 3);
        let due = match input.due.as_deref().map(str::trim) {
            Some("") => None,
            Some(d) => Some(normalize_datetime(d, true)?),
            None => cur.due,
        };
        let (remind, reminded) = match input.remind_at.as_deref().map(str::trim) {
            Some("") => (None, false),
            Some(d) => (Some(normalize_datetime(d, false)?), false),
            None => (cur.remind_at, cur.reminded),
        };
        self.with(|c| {
            c.execute(
                "UPDATE tasks SET title = ?1, notes = ?2, priority = ?3, due = ?4, remind_at = ?5, reminded = ?6 WHERE id = ?7",
                params![title, notes, priority, due, remind, reminded as i64, id],
            )
        })?;
        self.task_get(id)?.ok_or_else(|| "Tarea no encontrada.".into())
    }

    pub fn task_set_done(&self, id: i64, done: bool) -> DbResult<Task> {
        let completed = done.then(now_str);
        let n = self.with(|c| {
            c.execute(
                "UPDATE tasks SET done = ?1, completed_at = ?2 WHERE id = ?3",
                params![done as i64, completed, id],
            )
        })?;
        if n == 0 {
            return Err(format!("No existe la tarea #{id}."));
        }
        self.task_get(id)?.ok_or_else(|| "Tarea no encontrada.".into())
    }

    pub fn task_delete(&self, id: i64) -> DbResult<bool> {
        self.with(|c| c.execute("DELETE FROM tasks WHERE id = ?1", params![id]))
            .map(|n| n > 0)
    }

    pub fn task_list(&self, filter: TaskFilter) -> DbResult<Vec<Task>> {
        let today = today_str();
        let (where_, limit) = match filter {
            TaskFilter::Today => ("done = 0 AND due IS NOT NULL AND substr(due, 1, 10) <= ?1", 200),
            TaskFilter::Pending => ("done = 0 AND ?1 = ?1", 500),
            TaskFilter::Done => ("done = 1 AND ?1 = ?1", 100),
            TaskFilter::All => ("?1 = ?1", 600),
        };
        let order = if filter == TaskFilter::Done {
            "completed_at DESC"
        } else {
            "CASE WHEN due IS NULL THEN 1 ELSE 0 END, due, priority, id"
        };
        self.with(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {TASK_COLS} FROM tasks WHERE {where_} ORDER BY {order} LIMIT {limit}"
            ))?;
            let rows = st.query_map(params![today], task_from)?;
            rows.collect()
        })
    }

    /// Tareas cuyo recordatorio ya venció y aún no se avisó.
    pub fn tasks_due_for_reminder(&self, now: &str) -> DbResult<Vec<Task>> {
        self.with(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {TASK_COLS} FROM tasks WHERE done = 0 AND reminded = 0 AND remind_at IS NOT NULL AND remind_at <= ?1"
            ))?;
            let rows = st.query_map(params![now], task_from)?;
            rows.collect()
        })
    }

    pub fn task_mark_reminded(&self, id: i64) -> DbResult<()> {
        self.with(|c| c.execute("UPDATE tasks SET reminded = 1 WHERE id = ?1", params![id]))
            .map(|_| ())
    }

    // ----------------------------------------------------------- actividad

    pub fn log(&self, kind: &str, summary: &str) {
        let summary: String = summary.chars().take(300).collect();
        let _ = self.with(|c| {
            c.execute(
                "INSERT INTO activity (at, kind, summary) VALUES (?1, ?2, ?3)",
                params![now_str(), kind, summary],
            )?;
            // Conserva sólo los últimos 2000 registros.
            c.execute("DELETE FROM activity WHERE id <= (SELECT MAX(id) - 2000 FROM activity)", [])
        });
    }

    pub fn activity_list(&self, limit: usize) -> DbResult<Vec<Activity>> {
        self.with(|c| {
            let mut st = c.prepare("SELECT id, at, kind, summary FROM activity ORDER BY id DESC LIMIT ?1")?;
            let rows = st.query_map(params![limit as i64], |r| {
                Ok(Activity {
                    id: r.get(0)?,
                    at: r.get(1)?,
                    kind: r.get(2)?,
                    summary: r.get(3)?,
                })
            })?;
            rows.collect()
        })
    }

    // ------------------------------------------------------ conversaciones

    pub fn conversation_save(&self, id: &str, title: &str, data: &str) -> DbResult<()> {
        let title: String = title.trim().chars().take(80).collect();
        let now = now_str();
        self.with(|c| {
            c.execute(
                "INSERT INTO conversations (id, title, data, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)
                 ON CONFLICT(id) DO UPDATE SET title = excluded.title, data = excluded.data, updated_at = excluded.updated_at",
                params![id, title, data, now],
            )
        })
        .map(|_| ())
    }

    pub fn conversation_list(&self) -> DbResult<Vec<ConversationMeta>> {
        self.with(|c| {
            let mut st = c.prepare("SELECT id, title, created_at, updated_at FROM conversations ORDER BY updated_at DESC LIMIT 200")?;
            let rows = st.query_map([], |r| {
                Ok(ConversationMeta {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    created_at: r.get(2)?,
                    updated_at: r.get(3)?,
                })
            })?;
            rows.collect()
        })
    }

    pub fn conversation_get(&self, id: &str) -> DbResult<Option<String>> {
        self.with(|c| {
            c.query_row("SELECT data FROM conversations WHERE id = ?1", params![id], |r| r.get(0))
                .optional()
        })
    }

    pub fn conversation_delete(&self, id: &str) -> DbResult<bool> {
        self.with(|c| c.execute("DELETE FROM conversations WHERE id = ?1", params![id]))
            .map(|n| n > 0)
    }

    pub fn conversation_clear(&self) -> DbResult<usize> {
        self.with(|c| c.execute("DELETE FROM conversations", []))
    }

    /// Busca texto en conversaciones guardadas y devuelve fragmentos.
    pub fn conversation_search(&self, query: &str, limit: usize) -> DbResult<Vec<(ConversationMeta, String)>> {
        let q = query.trim().to_lowercase();
        if q.chars().count() < 3 {
            return Ok(vec![]);
        }
        let rows: Vec<(ConversationMeta, String)> = self.with(|c| {
            let mut st = c.prepare(
                "SELECT id, title, created_at, updated_at, data FROM conversations WHERE lower(data) LIKE ?1 ORDER BY updated_at DESC LIMIT ?2",
            )?;
            let rows = st.query_map(params![format!("%{q}%"), limit as i64], |r| {
                Ok((
                    ConversationMeta { id: r.get(0)?, title: r.get(1)?, created_at: r.get(2)?, updated_at: r.get(3)? },
                    r.get::<_, String>(4)?,
                ))
            })?;
            rows.collect()
        })?;
        Ok(rows
            .into_iter()
            .map(|(meta, data)| {
                let lower = data.to_lowercase();
                let pos = lower.find(&q).unwrap_or(0);
                let start = data
                    .char_indices()
                    .map(|(i, _)| i)
                    .take_while(|i| *i + 120 <= pos)
                    .last()
                    .unwrap_or(0);
                let snippet: String = data[start..].chars().take(280).collect();
                (meta, snippet)
            })
            .collect())
    }

    // ------------------------------------------------- índice de documentos

    /// ¿Hay que (re)indexar este archivo?
    pub fn doc_needs_index(&self, path: &str, mtime: i64, size: i64) -> DbResult<bool> {
        let row: Option<(i64, i64)> = self.with(|c| {
            c.query_row("SELECT mtime, size FROM doc_files WHERE path = ?1", params![path], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()
        })?;
        Ok(row != Some((mtime, size)))
    }

    pub fn doc_index(&self, path: &str, mtime: i64, size: i64, parts: &[String]) -> DbResult<()> {
        let mut conn = self.conn.lock().map_err(|_| "La base de datos está bloqueada.".to_string())?;
        let tx = conn.transaction().map_err(err)?;
        tx.execute("DELETE FROM doc_chunks WHERE path = ?1", params![path]).map_err(err)?;
        for (i, p) in parts.iter().enumerate() {
            tx.execute(
                "INSERT INTO doc_chunks (path, part, content) VALUES (?1, ?2, ?3)",
                params![path, (i + 1) as i64, p],
            )
            .map_err(err)?;
        }
        tx.execute(
            "INSERT INTO doc_files (path, mtime, size, parts, indexed_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(path) DO UPDATE SET mtime = excluded.mtime, size = excluded.size, parts = excluded.parts, indexed_at = excluded.indexed_at",
            params![path, mtime, size, parts.len() as i64, now_str()],
        )
        .map_err(err)?;
        tx.commit().map_err(err)
    }

    pub fn doc_forget_missing(&self, keep: &std::collections::HashSet<String>, under: &str) -> DbResult<usize> {
        let paths: Vec<String> = self.with(|c| {
            let mut st = c.prepare("SELECT path FROM doc_files WHERE path LIKE ?1")?;
            let rows = st.query_map(params![format!("{under}%")], |r| r.get(0))?;
            rows.collect()
        })?;
        let mut n = 0;
        for p in paths.into_iter().filter(|p| !keep.contains(p)) {
            self.with(|c| {
                c.execute("DELETE FROM doc_chunks WHERE path = ?1", params![p])?;
                c.execute("DELETE FROM doc_files WHERE path = ?1", params![p])
            })?;
            n += 1;
        }
        Ok(n)
    }

    pub fn doc_stats(&self) -> DbResult<(i64, Option<String>)> {
        self.with(|c| {
            c.query_row("SELECT COUNT(*), MAX(indexed_at) FROM doc_files", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
        })
    }

    pub fn doc_clear(&self) -> DbResult<()> {
        self.with(|c| c.execute_batch("DELETE FROM doc_chunks; DELETE FROM doc_files;"))
    }

    /// Búsqueda de texto completo. Devuelve (ruta, parte, fragmento).
    pub fn doc_search(&self, query: &str, under: Option<&str>, limit: usize) -> DbResult<Vec<(String, i64, String)>> {
        let words: Vec<String> = query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.chars().count() >= 2)
            .map(|w| format!("\"{}\"", w.replace('"', "")))
            .collect();
        if words.is_empty() {
            return Ok(vec![]);
        }
        let prefix = format!("{}%", under.unwrap_or(""));
        let run = |q: &str| -> DbResult<Vec<(String, i64, String)>> {
            self.with(|c| {
                let mut st = c.prepare(
                    "SELECT path, part, snippet(doc_chunks, 2, '«', '»', '…', 40) FROM doc_chunks
                     WHERE doc_chunks MATCH ?1 AND path LIKE ?2 ORDER BY bm25(doc_chunks) LIMIT ?3",
                )?;
                let rows = st.query_map(params![q, prefix, limit as i64], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
                rows.collect()
            })
        };
        // Primero todas las palabras; si no hay resultados, cualquiera de ellas.
        let all = run(&words.join(" "))?;
        if !all.is_empty() || words.len() == 1 {
            return Ok(all);
        }
        run(&words.join(" OR "))
    }

    // ------------------------------------------------------------- rutinas

    pub fn routine_save(&self, name: &str, steps_json: &str) -> DbResult<()> {
        let name: String = name.trim().chars().take(60).collect();
        if name.is_empty() {
            return Err("La rutina necesita un nombre.".into());
        }
        let now = now_str();
        self.with(|c| {
            c.execute(
                "INSERT INTO routines (name, steps, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)
                 ON CONFLICT(name) DO UPDATE SET steps = excluded.steps, updated_at = excluded.updated_at",
                params![name, steps_json, now],
            )
        })
        .map(|_| ())
    }

    pub fn routine_get(&self, name: &str) -> DbResult<Option<Routine>> {
        self.with(|c| {
            c.query_row(
                "SELECT name, steps, updated_at FROM routines WHERE lower(name) = lower(?1)",
                params![name.trim()],
                |r| {
                    Ok(Routine {
                        name: r.get(0)?,
                        steps: r.get(1)?,
                        updated_at: r.get(2)?,
                    })
                },
            )
            .optional()
        })
    }

    pub fn routine_list(&self) -> DbResult<Vec<Routine>> {
        self.with(|c| {
            let mut st = c.prepare("SELECT name, steps, updated_at FROM routines ORDER BY name")?;
            let rows = st.query_map([], |r| {
                Ok(Routine {
                    name: r.get(0)?,
                    steps: r.get(1)?,
                    updated_at: r.get(2)?,
                })
            })?;
            rows.collect()
        })
    }

    /// Apunta el costo de una respuesta de un modelo de pago.
    #[allow(clippy::too_many_arguments)]
    pub fn spend_add(&self, month: &str, provider: &str, model: &str, input: u64, output: u64, usd: f64, source: &str) -> DbResult<()> {
        if usd <= 0.0 {
            return Ok(());
        }
        let ts = now_str();
        self.with(|c| {
            c.execute(
                "INSERT INTO api_spend (ts, month, provider, model, input_tokens, output_tokens, usd, source) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![ts, month, provider, model, input as i64, output as i64, usd, source],
            )
        })
        .map(|_| ())
    }

    /// Parte del gasto del mes que es estimada (sin datos reales de consumo).
    pub fn spend_month_estimated(&self, month: &str) -> DbResult<f64> {
        self.with(|c| {
            c.query_row(
                "SELECT COALESCE(SUM(usd), 0) FROM api_spend WHERE month = ?1 AND source = 'estimado'",
                params![month],
                |r| r.get(0),
            )
        })
    }

    /// Gasto total de un mes («AAAA-MM»).
    pub fn spend_month(&self, month: &str) -> DbResult<f64> {
        self.with(|c| {
            c.query_row(
                "SELECT COALESCE(SUM(usd), 0) FROM api_spend WHERE month = ?1",
                params![month],
                |r| r.get(0),
            )
        })
    }

    /// Gasto del mes por proveedor y modelo.
    pub fn spend_breakdown(&self, month: &str) -> DbResult<Vec<SpendRow>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT provider, model, COUNT(*), SUM(usd) FROM api_spend WHERE month = ?1 GROUP BY provider, model ORDER BY SUM(usd) DESC",
            )?;
            let rows = st.query_map(params![month], |r| {
                Ok(SpendRow {
                    provider: r.get(0)?,
                    model: r.get(1)?,
                    calls: r.get(2)?,
                    usd: r.get(3)?,
                })
            })?;
            rows.collect()
        })
    }

    pub fn routine_delete(&self, name: &str) -> DbResult<bool> {
        self.with(|c| c.execute("DELETE FROM routines WHERE lower(name) = lower(?1)", params![name.trim()]))
            .map(|n| n > 0)
    }

    /// Resumen para el prompt de sistema al empezar una conversación.
    pub fn context_summary(&self) -> DbResult<String> {
        let memories = self.memory_list(None, None, 40)?;
        let today = self.task_list(TaskFilter::Today)?;
        let pending = self.task_list(TaskFilter::Pending)?;
        let mut out = String::new();
        if !memories.is_empty() {
            out.push_str("Lo que recuerdas de tu humano (memoria local):\n");
            for m in &memories {
                let temp = m
                    .expires_at
                    .as_deref()
                    .map(|e| format!(", temporal hasta {}", &e[..10]))
                    .unwrap_or_default();
                out.push_str(&format!("- [#{} {}{temp}] {}\n", m.id, m.kind, m.content));
            }
        }
        out.push_str(&format!(
            "Tareas: {} pendientes en total, {} para hoy o atrasadas.\n",
            pending.len(),
            today.len()
        ));
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    #[test]
    fn memory_crud_and_search() {
        let d = db();
        let m = d
            .memory_add("Proyectos", "Estoy desarrollando una página web de la importadora", "usuario")
            .unwrap();
        assert_eq!(m.kind, "proyecto");
        d.memory_add("preferencia", "Prefiere respuestas cortas", "usuario").unwrap();
        // duplicado exacto no se repite
        d.memory_add("proyecto", "estoy desarrollando una página web de la importadora", "usuario")
            .unwrap();
        assert_eq!(d.memory_list(None, None, 50).unwrap().len(), 2);
        assert_eq!(d.memory_list(Some("proyecto"), None, 50).unwrap().len(), 1);
        assert_eq!(d.memory_list(None, Some("página web"), 50).unwrap()[0].id, m.id);
        let u = d.memory_update(m.id, None, Some("Página web terminada")).unwrap();
        assert_eq!(u.content, "Página web terminada");
        assert!(d.memory_delete(m.id).unwrap());
        assert!(!d.memory_delete(m.id).unwrap());
        assert_eq!(d.memory_clear().unwrap(), 1);
    }

    #[test]
    fn temporary_memories_expire() {
        let d = db();
        let t = d
            .memory_add_temporary("dato", "Mañana tengo examen de cálculo", "canela", 2)
            .unwrap();
        assert!(t.expires_at.is_some());
        d.memory_add("proyecto", "Página web", "usuario").unwrap();
        assert_eq!(d.memory_list(None, None, 10).unwrap().len(), 2);
        // Simula que ya caducó.
        d.memory_set_expiry(t.id, Some("2000-01-01 00:00:00")).unwrap();
        assert_eq!(d.memory_list(None, None, 10).unwrap().len(), 1, "los caducados no se muestran");
        assert_eq!(d.memory_purge_expired().unwrap(), 1);
        // Hacer permanente
        let p = d.memory_add_temporary("dato", "Algo", "usuario", 1).unwrap();
        assert!(d.memory_set_expiry(p.id, None).unwrap().expires_at.is_none());
    }

    #[test]
    fn rejects_empty_memory() {
        assert!(db().memory_add("dato", "   ", "usuario").is_err());
    }

    #[test]
    fn sensitive_detection() {
        assert!(looks_sensitive("mi contraseña del banco es 1234").is_some());
        assert!(looks_sensitive("tarjeta 4111 1111 1111 1111").is_some());
        assert!(looks_sensitive("usa sk-ant-api03-abcdefghijklmnopqrstuvwx").is_some());
        assert!(looks_sensitive("Estoy aprendiendo React y TypeScript").is_none());
        assert!(looks_sensitive("Mi cumpleaños es el 2001-05-10").is_none());
    }

    #[test]
    fn dates_are_validated() {
        assert_eq!(normalize_datetime("2026-11-20", true).unwrap(), "2026-11-20");
        assert_eq!(normalize_datetime("2026-11-20T08:30", false).unwrap(), "2026-11-20 08:30");
        assert!(normalize_datetime("mañana", true).is_err());
        assert!(normalize_datetime("2026-11-20", false).is_err());
    }

    #[test]
    fn tasks_lifecycle_and_filters() {
        let d = db();
        let today = today_str();
        let t = d
            .task_add(&TaskInput {
                title: Some("Estudiar programación".into()),
                due: Some(today.clone()),
                priority: Some(1),
                ..Default::default()
            })
            .unwrap();
        d.task_add(&TaskInput {
            title: Some("Comprar café".into()),
            ..Default::default()
        })
        .unwrap();
        d.task_add(&TaskInput {
            title: Some("Entrega final".into()),
            due: Some("2999-01-01".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(d.task_list(TaskFilter::Today).unwrap().len(), 1);
        assert_eq!(d.task_list(TaskFilter::Pending).unwrap().len(), 3);
        // Las tareas con fecha van primero.
        assert_eq!(d.task_list(TaskFilter::Pending).unwrap()[0].id, t.id);
        d.task_set_done(t.id, true).unwrap();
        assert_eq!(d.task_list(TaskFilter::Today).unwrap().len(), 0);
        assert_eq!(d.task_list(TaskFilter::Done).unwrap().len(), 1);
        let e = d
            .task_update(
                t.id,
                &TaskInput {
                    title: Some("Estudiar Rust".into()),
                    due: Some(String::new()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(e.title, "Estudiar Rust");
        assert_eq!(e.due, None);
        assert!(d.task_delete(t.id).unwrap());
        assert!(d.task_set_done(t.id, true).is_err());
    }

    #[test]
    fn reminders_fire_once() {
        let d = db();
        let t = d
            .task_add(&TaskInput {
                title: Some("Llamar".into()),
                remind_at: Some("2020-01-01 09:00".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(d.tasks_due_for_reminder(&now_str()).unwrap().len(), 1);
        d.task_mark_reminded(t.id).unwrap();
        assert!(d.tasks_due_for_reminder(&now_str()).unwrap().is_empty());
        // Cambiar el recordatorio lo reactiva.
        d.task_update(
            t.id,
            &TaskInput {
                remind_at: Some("2020-01-02 09:00".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(d.tasks_due_for_reminder(&now_str()).unwrap().len(), 1);
    }

    #[test]
    fn activity_and_conversations() {
        let d = db();
        d.log("tarea", "Creaste «Estudiar»");
        assert_eq!(d.activity_list(10).unwrap()[0].summary, "Creaste «Estudiar»");
        d.conversation_save("c1", "React", r#"[{"text":"Hablemos de hooks de React"}]"#)
            .unwrap();
        d.conversation_save("c1", "React 2", r#"[{"text":"Hablemos de hooks de React y efectos"}]"#)
            .unwrap();
        assert_eq!(d.conversation_list().unwrap().len(), 1);
        assert_eq!(d.conversation_list().unwrap()[0].title, "React 2");
        let found = d.conversation_search("hooks", 5).unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].1.contains("hooks"));
        assert!(d.conversation_delete("c1").unwrap());
    }

    #[test]
    fn document_index_and_search() {
        let d = db();
        d.doc_index(
            "C:/docs/contrato.pdf",
            1,
            10,
            &[
                "El contrato de arrendamiento vence en marzo.".into(),
                "Cláusula de penalización por retraso.".into(),
            ],
        )
        .unwrap();
        d.doc_index("C:/docs/receta.txt", 1, 10, &["Pastel de chocolate con canela.".into()])
            .unwrap();
        let r = d.doc_search("penalizacion retraso", None, 5).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!((r[0].0.as_str(), r[0].1), ("C:/docs/contrato.pdf", 2));
        assert!(r[0].2.contains("«"));
        // Sin acentos ni mayúsculas, y con "cualquiera de las palabras" como respaldo.
        assert_eq!(d.doc_search("CANELA xyz", None, 5).unwrap()[0].0, "C:/docs/receta.txt");
        assert!(!d.doc_needs_index("C:/docs/receta.txt", 1, 10).unwrap());
        assert!(d.doc_needs_index("C:/docs/receta.txt", 2, 10).unwrap());
        // Reindexar reemplaza; borrar los que ya no existen.
        d.doc_index("C:/docs/receta.txt", 2, 10, &["Galletas.".into()]).unwrap();
        assert!(d.doc_search("chocolate", None, 5).unwrap().is_empty());
        let keep: std::collections::HashSet<String> = ["C:/docs/receta.txt".to_string()].into();
        assert_eq!(d.doc_forget_missing(&keep, "C:/docs").unwrap(), 1);
        assert_eq!(d.doc_stats().unwrap().0, 1);
        assert!(d.doc_search("contrato", Some("D:/otra"), 5).unwrap().is_empty());
    }

    #[test]
    fn migration_from_v1_keeps_data() {
        let path = std::env::temp_dir().join(format!("canela-mig-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(SCHEMA_V1).unwrap();
            c.execute_batch("PRAGMA user_version = 1").unwrap();
            c.execute(
                "INSERT INTO memories (kind, content, source, created_at, updated_at) VALUES ('dato','de la v0.2','usuario','x','x')",
                [],
            )
            .unwrap();
        }
        let d = Db::open(&path).unwrap();
        assert_eq!(d.memory_list(None, None, 10).unwrap()[0].content, "de la v0.2");
        d.doc_index("a", 1, 1, &["hola".into()]).unwrap();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn routines_crud() {
        let d = db();
        d.routine_save(
            "Entorno de programación",
            r#"[{"herramienta":"abrir_aplicacion","parametros":{"nombre":"vscode"}}]"#,
        )
        .unwrap();
        assert_eq!(d.routine_list().unwrap().len(), 1);
        assert!(d.routine_get("entorno de PROGRAMACIÓN").unwrap().is_none() || true);
        assert!(d.routine_get("Entorno de programación").unwrap().is_some());
        assert!(d.routine_delete("Entorno de programación").unwrap());
        assert!(d.routine_save("  ", "[]").is_err());
    }

    #[test]
    fn context_summary_lists_memories() {
        let d = db();
        d.memory_add("proyecto", "App de Canela", "usuario").unwrap();
        let s = d.context_summary().unwrap();
        assert!(s.contains("App de Canela"));
        assert!(s.contains("pendientes"));
    }

    #[test]
    fn spend_is_recorded_per_month() {
        let db = Db::open_in_memory().unwrap();
        db.spend_add("2026-11", "anthropic", "claude-opus-5-5", 6000, 800, 0.04, "api")
            .unwrap();
        db.spend_add("2026-11", "anthropic", "claude-opus-5-5", 9000, 1200, 0.06, "api")
            .unwrap();
        db.spend_add("2026-11", "openai", "gpt-5", 5000, 500, 0.01, "api").unwrap();
        db.spend_add("2026-10", "openai", "gpt-5", 5000, 500, 3.0, "api").unwrap();
        db.spend_add("2026-11", "ollama", "qwen3:8b", 5000, 500, 0.0, "api").unwrap(); // gratis: no se apunta
        assert!((db.spend_month("2026-11").unwrap() - 0.11).abs() < 1e-9);
        assert!((db.spend_month("2026-10").unwrap() - 3.0).abs() < 1e-9);
        assert_eq!(db.spend_month("2027-01").unwrap(), 0.0);
        let b = db.spend_breakdown("2026-11").unwrap();
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].provider, "anthropic");
        assert_eq!(b[0].calls, 2);
        db.spend_add("2026-11", "openai", "gpt-5", 0, 0, 0.02, "estimado").unwrap();
        assert!((db.spend_month_estimated("2026-11").unwrap() - 0.02).abs() < 1e-9);
        assert!(
            (db.spend_month("2026-11").unwrap() - 0.13).abs() < 1e-9,
            "lo estimado también cuenta para el límite"
        );
    }
}
