//! Herramientas que Canela puede usar desde el chat.
//!
//! Cada llamada pasa por tres fases:
//! 1. `prepare`: valida los parámetros y calcula qué se va a hacer
//!    (incluida la vista previa de cambios). Aquí no se toca nada.
//! 2. Permiso: `Permission::Auto` (lectura o acciones inocuas) o
//!    `Permission::Confirm` (el usuario debe aprobarlo en una tarjeta).
//! 3. `run`: ejecuta la acción ya validada.
//!
//! No existe ninguna herramienta para ejecutar comandos de consola.

pub mod apps;
pub mod files;

use crate::ai::{self, ToolDef};
use crate::db::{looks_sensitive, normalize_kind, Db, TaskFilter, TaskInput};
use crate::jobs::Jobs;
use crate::settings::Settings;
use crate::vision;
use files::{display, Scope};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

/// Acceso a la IA para trabajos en segundo plano (analizar proyectos).
#[derive(Clone)]
pub struct LlmAccess {
    pub http: reqwest::Client,
    pub settings: Settings,
    pub api_key: String,
}

pub struct ToolCtx {
    pub db: Arc<Db>,
    pub settings: Settings,
    pub scope: Scope,
    pub backups_dir: PathBuf,
    pub jobs: Option<Arc<Jobs>>,
    pub llm: Option<LlmAccess>,
    /// Avisa a la interfaz de que cambiaron tareas o memoria.
    pub notify: Arc<dyn Fn(&'static str) + Send + Sync>,
    /// Capturas de pantalla (None = no disponibles en este contexto).
    pub screen: Option<Arc<dyn ScreenCapturer>>,
}

type BoxFut<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Quien sabe capturar la pantalla (en la app: oculta a Canela y captura).
pub trait ScreenCapturer: Send + Sync {
    fn capture(&self, target: vision::Target) -> BoxFut<'_, Result<image::RgbaImage, String>>;
    /// Id de una ventana cuyo título contiene `title` (sin ventanas sensibles).
    fn find_window(&self, title: &str) -> Option<u32>;
}

/// Lo que una herramienta devuelve al modelo.
pub struct ToolOutput {
    pub text: String,
    pub images: Vec<vision::ImageData>,
}

pub enum Permission {
    Auto,
    Confirm {
        title: String,
        details: String,
        diff: Option<String>,
        /// Vista previa (data URL) de una imagen que se enviaría.
        image: Option<String>,
    },
}

pub struct Prepared {
    /// Descripción corta para la interfaz ("Abrir Visual Studio Code").
    pub label: String,
    pub permission: Permission,
    action: Action,
}

enum Action {
    SendImage(vision::ImageData),
    Remember {
        kind: String,
        content: String,
    },
    SearchMemory {
        query: Option<String>,
        kind: Option<String>,
    },
    Forget {
        id: i64,
    },
    SearchConversations {
        query: String,
    },
    AddTask(TaskInput),
    ListTasks(TaskFilter),
    UpdateTask {
        id: i64,
        input: TaskInput,
        done: Option<bool>,
    },
    DeleteTask {
        id: i64,
    },
    OpenApp(apps::Target),
    OpenWeb {
        url: String,
        browser: Option<String>,
    },
    ListDir(Option<PathBuf>),
    Search {
        query: String,
        root: Option<PathBuf>,
        only_dirs: Option<bool>,
    },
    Recent {
        limit: usize,
    },
    Read(PathBuf),
    OpenPath(PathBuf),
    CreateDir(PathBuf),
    Write {
        path: PathBuf,
        content: String,
    },
    Organize {
        dir: PathBuf,
        plan: Vec<(PathBuf, &'static str)>,
    },
    Analyze {
        dir: PathBuf,
        focus: Option<String>,
    },
}

// ----------------------------------------------------------- definiciones

fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}

pub fn definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "recordar",
            description: "Guarda en la memoria local algo que el usuario quiere que recuerdes (proyectos, preferencias, horarios, apps favoritas, datos importantes). Nunca datos sensibles.",
            schema: obj(
                json!({
                    "tipo": { "type": "string", "enum": ["proyecto", "preferencia", "horario", "aplicacion", "persona", "dato", "otro"] },
                    "contenido": { "type": "string", "description": "Frase breve y autocontenida, p. ej. «Está desarrollando una página web para la importadora»." }
                }),
                &["tipo", "contenido"],
            ),
        },
        ToolDef {
            name: "buscar_recuerdos",
            description: "Busca en la memoria local. Sin parámetros devuelve los recuerdos más recientes.",
            schema: obj(json!({ "consulta": { "type": "string" }, "tipo": { "type": "string" } }), &[]),
        },
        ToolDef {
            name: "olvidar",
            description: "Borra un recuerdo por su id (obtenido con buscar_recuerdos). Pide confirmación al usuario.",
            schema: obj(json!({ "id": { "type": "integer" } }), &["id"]),
        },
        ToolDef {
            name: "buscar_conversaciones",
            description: "Busca en conversaciones anteriores guardadas (sólo si el usuario activó el historial).",
            schema: obj(json!({ "consulta": { "type": "string" } }), &["consulta"]),
        },
        ToolDef {
            name: "crear_tarea",
            description: "Crea una tarea pendiente, opcionalmente con fecha, recordatorio y prioridad.",
            schema: obj(
                json!({
                    "titulo": { "type": "string" },
                    "fecha": { "type": "string", "description": "AAAA-MM-DD o AAAA-MM-DD HH:MM" },
                    "recordatorio": { "type": "string", "description": "AAAA-MM-DD HH:MM; se mostrará una notificación de Windows" },
                    "prioridad": { "type": "string", "enum": ["alta", "normal", "baja"] },
                    "notas": { "type": "string" }
                }),
                &["titulo"],
            ),
        },
        ToolDef {
            name: "listar_tareas",
            description: "Lista tareas. 'hoy' = pendientes para hoy o atrasadas.",
            schema: obj(json!({ "filtro": { "type": "string", "enum": ["hoy", "pendientes", "completadas", "todas"] } }), &[]),
        },
        ToolDef {
            name: "actualizar_tarea",
            description: "Edita una tarea o márcala como completada. Usa listar_tareas para obtener el id. Para quitar la fecha o el recordatorio envía cadena vacía.",
            schema: obj(
                json!({
                    "id": { "type": "integer" },
                    "titulo": { "type": "string" },
                    "fecha": { "type": "string" },
                    "recordatorio": { "type": "string" },
                    "prioridad": { "type": "string", "enum": ["alta", "normal", "baja"] },
                    "completada": { "type": "boolean" }
                }),
                &["id"],
            ),
        },
        ToolDef {
            name: "eliminar_tarea",
            description: "Elimina una tarea definitivamente (pide confirmación). Si sólo está hecha, usa actualizar_tarea con completada=true.",
            schema: obj(json!({ "id": { "type": "integer" } }), &["id"]),
        },
        ToolDef {
            name: "abrir_aplicacion",
            description: "Abre una aplicación instalada por su nombre (p. ej. «Visual Studio Code», «Chrome», «Spotify», «Calculadora», «Explorador de archivos»).",
            schema: obj(json!({ "nombre": { "type": "string" } }), &["nombre"]),
        },
        ToolDef {
            name: "abrir_web",
            description: "Abre una página web o una búsqueda de Google en el navegador.",
            schema: obj(
                json!({
                    "url": { "type": "string" },
                    "busqueda": { "type": "string", "description": "Texto a buscar en Google si no hay URL" },
                    "navegador": { "type": "string", "enum": ["predeterminado", "chrome", "edge", "firefox"] }
                }),
                &[],
            ),
        },
        ToolDef {
            name: "listar_carpeta",
            description: "Lista el contenido de una carpeta autorizada. Sin ruta, muestra las carpetas autorizadas. Acepta rutas completas o que empiecen por Documentos, Descargas o Escritorio.",
            schema: obj(json!({ "ruta": { "type": "string" } }), &[]),
        },
        ToolDef {
            name: "buscar_archivos",
            description: "Busca archivos o carpetas por nombre dentro de las carpetas autorizadas (p. ej. «react» para encontrar un proyecto).",
            schema: obj(
                json!({
                    "texto": { "type": "string" },
                    "en": { "type": "string", "description": "Carpeta donde buscar (opcional)" },
                    "solo": { "type": "string", "enum": ["archivos", "carpetas", "ambos"] }
                }),
                &["texto"],
            ),
        },
        ToolDef {
            name: "archivos_recientes",
            description: "Muestra los archivos modificados más recientemente en las carpetas autorizadas.",
            schema: obj(json!({ "limite": { "type": "integer" } }), &[]),
        },
        ToolDef {
            name: "leer_archivo",
            description: "Lee un archivo de texto o código (máx. 300 KB) dentro de las carpetas autorizadas.",
            schema: obj(json!({ "ruta": { "type": "string" } }), &["ruta"]),
        },
        ToolDef {
            name: "abrir_ruta",
            description: "Abre una carpeta en el Explorador o un documento con su programa predeterminado. No abre programas ni scripts.",
            schema: obj(json!({ "ruta": { "type": "string" } }), &["ruta"]),
        },
        ToolDef {
            name: "crear_carpeta",
            description: "Crea una carpeta nueva dentro de una carpeta autorizada (pide confirmación).",
            schema: obj(json!({ "ruta": { "type": "string" } }), &["ruta"]),
        },
        ToolDef {
            name: "escribir_archivo",
            description: "Crea un archivo de texto/código o reemplaza uno existente con el contenido COMPLETO nuevo. El usuario ve los cambios y debe aprobarlos; se guarda una copia del original.",
            schema: obj(json!({ "ruta": { "type": "string" }, "contenido": { "type": "string" } }), &["ruta", "contenido"]),
        },
        ToolDef {
            name: "organizar_carpeta",
            description: "Ordena los archivos sueltos de una carpeta (p. ej. Descargas) en subcarpetas por tipo: Imágenes, Documentos, Videos, etc. Sólo mueve, nunca borra. Pide confirmación mostrando el plan.",
            schema: obj(json!({ "ruta": { "type": "string" } }), &["ruta"]),
        },
        ToolDef {
            name: "ver_pantalla",
            description: "Toma una captura de la pantalla (o de una ventana) para ver lo que el usuario tiene delante: errores, mensajes, código. El usuario verá la captura y decidirá si se envía. Úsala cuando te pidan mirar la pantalla.",
            schema: obj(
                json!({
                    "que": { "type": "string", "enum": ["pantalla", "ventana"] },
                    "ventana": { "type": "string", "description": "Parte del título de la ventana (si que=ventana)" }
                }),
                &[],
            ),
        },
        ToolDef {
            name: "analizar_proyecto",
            description: "Analiza la estructura y archivos clave de un proyecto de software en SEGUNDO PLANO y luego entrega un informe en el chat. El usuario puede seguir conversando mientras tanto.",
            schema: obj(
                json!({ "ruta": { "type": "string" }, "enfoque": { "type": "string", "description": "Qué revisar con más atención (opcional)" } }),
                &["ruta"],
            ),
        },
    ]
}

// ---------------------------------------------------------------- helpers

fn s(input: &Value, key: &str) -> Option<String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn s_raw(input: &Value, key: &str) -> Option<String> {
    input.get(key).and_then(Value::as_str).map(str::to_string)
}

fn req(input: &Value, key: &str) -> Result<String, String> {
    s(input, key).ok_or_else(|| format!("Falta el parámetro «{key}»."))
}

fn int(input: &Value, key: &str) -> Result<i64, String> {
    input
        .get(key)
        .and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_str().and_then(|s| s.trim().trim_start_matches('#').parse().ok()))
        })
        .ok_or_else(|| format!("Falta el parámetro numérico «{key}»."))
}

fn priority(input: &Value) -> Result<Option<i64>, String> {
    match s(input, "prioridad").map(|p| p.to_lowercase()) {
        None => Ok(None),
        Some(p) => match p.as_str() {
            "alta" | "high" | "1" => Ok(Some(1)),
            "normal" | "media" | "2" => Ok(Some(2)),
            "baja" | "low" | "3" => Ok(Some(3)),
            _ => Err(format!("Prioridad no válida: «{p}» (usa alta, normal o baja).")),
        },
    }
}

fn confirm(title: impl Into<String>, details: impl Into<String>, diff: Option<String>) -> Permission {
    Permission::Confirm {
        title: title.into(),
        details: details.into(),
        diff,
        image: None,
    }
}

fn task_line(t: &crate::db::Task) -> String {
    let mut s = format!("#{} {}", t.id, t.title);
    if let Some(d) = &t.due {
        s.push_str(&format!(" · fecha {d}"));
    }
    if t.priority == 1 {
        s.push_str(" · prioridad alta");
    }
    if let Some(r) = &t.remind_at {
        s.push_str(&format!(" · aviso {r}"));
    }
    if t.done {
        s.push_str(" · completada");
    }
    s
}

/// Nombre corto («notas.md») para etiquetas; la ruta completa va en los detalles.
fn short(p: &std::path::Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| display(p))
}

fn parent(p: &std::path::Path) -> String {
    p.parent().map(display).unwrap_or_default()
}

fn fmt_time(t: std::time::SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Local> = t.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}

// ---------------------------------------------------------------- prepare

/// Como `prepare`, pero puede hacer trabajo asíncrono previo (capturas).
pub async fn prepare_async(ctx: &ToolCtx, name: &str, input: &Value) -> Result<Prepared, String> {
    if name != "ver_pantalla" {
        return prepare(ctx, name, input);
    }
    let screen = ctx.screen.clone().ok_or("La captura de pantalla no está disponible ahora.")?;
    if !ctx.settings.vision_enabled {
        return Err("La visión de pantalla está desactivada en Ajustes.".into());
    }
    if ai::model_supports_images(ctx.settings.provider, &ai::model_of(&ctx.settings)) == Some(false) {
        return Err("El modelo elegido no acepta imágenes. Cambia a un modelo con visión en Ajustes.".into());
    }
    let target = match s(input, "que").as_deref() {
        Some("ventana") => {
            let title = req(input, "ventana")?;
            let id = screen
                .find_window(&title)
                .ok_or_else(|| format!("No encontré una ventana abierta con «{title}» (o es una ventana privada)."))?;
            vision::Target::Window(id)
        }
        _ => vision::Target::Active,
    };
    let img = screen.capture(target).await?;
    let for_model = vision::for_model(&img, None)?;
    let preview = vision::preview(&img)?.data_url();
    drop(img);
    Ok(Prepared {
        label: "Mirar tu pantalla".into(),
        permission: Permission::Confirm {
            title: "¿Envío esta captura a la IA?".into(),
            details: "Revisa que no aparezca nada privado. Sólo se enviará si pulsas Permitir.".into(),
            diff: None,
            image: Some(preview),
        },
        action: Action::SendImage(for_model),
    })
}

pub fn prepare(ctx: &ToolCtx, name: &str, input: &Value) -> Result<Prepared, String> {
    let p = |label: String, permission: Permission, action: Action| Ok(Prepared { label, permission, action });
    match name {
        "recordar" => {
            let content = req(input, "contenido")?;
            if let Some(why) = looks_sensitive(&content) {
                return Err(format!("No guardo información sensible ({why}). Por seguridad no lo recordaré."));
            }
            let kind = normalize_kind(&s(input, "tipo").unwrap_or_default());
            p(format!("Recordar: {content}"), Permission::Auto, Action::Remember { kind, content })
        }
        "buscar_recuerdos" => p(
            "Buscar en mi memoria".into(),
            Permission::Auto,
            Action::SearchMemory {
                query: s(input, "consulta"),
                kind: s(input, "tipo"),
            },
        ),
        "olvidar" => {
            let id = int(input, "id")?;
            let m = ctx.db.memory_get(id)?.ok_or_else(|| format!("No existe el recuerdo #{id}."))?;
            p(
                "Olvidar un recuerdo".to_string(),
                confirm("Olvidar este recuerdo", m.content, None),
                Action::Forget { id },
            )
        }
        "buscar_conversaciones" => {
            if !ctx.settings.save_conversations {
                return Err("El historial de conversaciones está desactivado (Ajustes › Memoria).".into());
            }
            p(
                "Buscar en conversaciones anteriores".into(),
                Permission::Auto,
                Action::SearchConversations {
                    query: req(input, "consulta")?,
                },
            )
        }
        "crear_tarea" => {
            let title = req(input, "titulo")?;
            let ti = TaskInput {
                title: Some(title.clone()),
                notes: s(input, "notas"),
                priority: priority(input)?,
                due: s(input, "fecha"),
                remind_at: s(input, "recordatorio"),
            };
            // Valida fechas antes de crear nada.
            if let Some(d) = &ti.due {
                crate::db::normalize_datetime(d, true)?;
            }
            if let Some(d) = &ti.remind_at {
                crate::db::normalize_datetime(d, false)?;
            }
            p(format!("Crear tarea: {title}"), Permission::Auto, Action::AddTask(ti))
        }
        "listar_tareas" => {
            let f = match s(input, "filtro").as_deref() {
                Some("hoy") => TaskFilter::Today,
                Some("completadas") => TaskFilter::Done,
                Some("todas") => TaskFilter::All,
                _ => TaskFilter::Pending,
            };
            p("Revisar tus tareas".into(), Permission::Auto, Action::ListTasks(f))
        }
        "actualizar_tarea" => {
            let id = int(input, "id")?;
            let t = ctx.db.task_get(id)?.ok_or_else(|| format!("No existe la tarea #{id}."))?;
            let ti = TaskInput {
                title: s(input, "titulo"),
                notes: None,
                priority: priority(input)?,
                due: s_raw(input, "fecha"),
                remind_at: s_raw(input, "recordatorio"),
            };
            let done = input.get("completada").and_then(Value::as_bool);
            let label = if done == Some(true) {
                format!("Completar: {}", t.title)
            } else {
                format!("Editar tarea: {}", t.title)
            };
            p(label, Permission::Auto, Action::UpdateTask { id, input: ti, done })
        }
        "eliminar_tarea" => {
            let id = int(input, "id")?;
            let t = ctx.db.task_get(id)?.ok_or_else(|| format!("No existe la tarea #{id}."))?;
            p(
                format!("Eliminar tarea: {}", t.title),
                confirm("Eliminar tarea", t.title, None),
                Action::DeleteTask { id },
            )
        }
        "abrir_aplicacion" => {
            let name = req(input, "nombre")?;
            let target = apps::resolve(&name, &apps::start_menu_shortcuts())
                .ok_or_else(|| format!("No encontré «{name}» entre las aplicaciones instaladas del menú Inicio."))?;
            let label = format!("Abrir {}", target.label());
            let perm = if ctx.settings.auto_open_apps {
                Permission::Auto
            } else {
                confirm(label.clone(), "Canela abrirá esta aplicación.", None)
            };
            p(label, perm, Action::OpenApp(target))
        }
        "abrir_web" => {
            let url = apps::web_target(s(input, "url").as_deref(), s(input, "busqueda").as_deref())?;
            let browser = s(input, "navegador").filter(|b| b != "predeterminado");
            let label = match s(input, "busqueda") {
                Some(q) if s(input, "url").is_none() => format!("Buscar en la web: {q}"),
                _ => format!("Abrir {url}"),
            };
            let perm = if ctx.settings.auto_open_web {
                Permission::Auto
            } else {
                confirm("Abrir en el navegador", url.clone(), None)
            };
            p(label, perm, Action::OpenWeb { url, browser })
        }
        "listar_carpeta" => match s(input, "ruta") {
            None => p("Ver carpetas autorizadas".into(), Permission::Auto, Action::ListDir(None)),
            Some(r) => {
                let dir = ctx.scope.existing(&r)?;
                p(
                    format!("Ver la carpeta {}", short(&dir)),
                    Permission::Auto,
                    Action::ListDir(Some(dir)),
                )
            }
        },
        "buscar_archivos" => {
            let query = req(input, "texto")?;
            let root = match s(input, "en") {
                Some(r) => Some(ctx.scope.existing(&r)?),
                None => None,
            };
            let only_dirs = match s(input, "solo").as_deref() {
                Some("carpetas") => Some(true),
                Some("archivos") => Some(false),
                _ => None,
            };
            p(
                format!("Buscar «{query}»"),
                Permission::Auto,
                Action::Search { query, root, only_dirs },
            )
        }
        "archivos_recientes" => {
            let limit = input.get("limite").and_then(Value::as_u64).unwrap_or(15).clamp(1, 40) as usize;
            p("Ver archivos recientes".into(), Permission::Auto, Action::Recent { limit })
        }
        "leer_archivo" => {
            let path = ctx.scope.existing(&req(input, "ruta")?)?;
            p(format!("Leer {}", short(&path)), Permission::Auto, Action::Read(path))
        }
        "abrir_ruta" => {
            let path = ctx.scope.existing(&req(input, "ruta")?)?;
            if path.is_file() && files::is_blocked(&path) {
                return Err("Por seguridad no abro programas, instaladores ni scripts.".into());
            }
            let label = format!("Abrir {}", short(&path));
            let perm = if ctx.settings.auto_open_apps {
                Permission::Auto
            } else {
                confirm(label.clone(), format!("En: {}", parent(&path)), None)
            };
            p(label, perm, Action::OpenPath(path))
        }
        "crear_carpeta" => {
            let path = ctx.scope.target(&req(input, "ruta")?)?;
            if path.exists() {
                return Err(format!("La carpeta «{}» ya existe.", display(&path)));
            }
            p(
                format!("Crear la carpeta {}", short(&path)),
                confirm(
                    format!("Crear la carpeta «{}»", short(&path)),
                    format!("Dentro de: {}", parent(&path)),
                    None,
                ),
                Action::CreateDir(path),
            )
        }
        "escribir_archivo" => {
            let path = ctx.scope.target(&req(input, "ruta")?)?;
            let content = s_raw(input, "contenido").ok_or("Falta el parámetro «contenido».")?;
            if files::is_blocked(&path) {
                return Err("Por seguridad no creo ni modifico programas ni scripts ejecutables (.exe, .bat, .ps1…).".into());
            }
            if content.len() > files::MAX_WRITE_BYTES {
                return Err("El contenido es demasiado grande (máximo 500 KB).".into());
            }
            if path.is_dir() {
                return Err("Esa ruta es una carpeta.".into());
            }
            let (title, old) = if path.exists() {
                (format!("Modificar «{}»", short(&path)), files::read_text(&path)?)
            } else {
                (format!("Crear «{}»", short(&path)), String::new())
            };
            if old == content {
                return Err("El archivo ya tiene exactamente ese contenido; no hay cambios.".into());
            }
            let diff = files::unified_diff(&old, &content, 400);
            let details = if old.is_empty() {
                format!("Carpeta: {}\nArchivo nuevo ({} líneas).", parent(&path), content.lines().count())
            } else {
                format!(
                    "Carpeta: {}\nRevisa los cambios. Se guardará una copia del original.",
                    parent(&path)
                )
            };
            p(title.clone(), confirm(title, details, Some(diff)), Action::Write { path, content })
        }
        "organizar_carpeta" => {
            let dir = ctx.scope.existing(&req(input, "ruta")?)?;
            let plan = files::organize_plan(&dir)?;
            if plan.is_empty() {
                return Err("No hay archivos sueltos que organizar en esa carpeta.".into());
            }
            let mut counts: Vec<(&str, usize)> = Vec::new();
            for (_, cat) in &plan {
                match counts.iter_mut().find(|(c, _)| c == cat) {
                    Some(e) => e.1 += 1,
                    None => counts.push((cat, 1)),
                }
            }
            let summary = counts.iter().map(|(c, n)| format!("{n} → {c}")).collect::<Vec<_>>().join("\n");
            let preview = plan
                .iter()
                .take(60)
                .map(|(f, c)| {
                    format!(
                        "{}  →  {c}/",
                        f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            p(
                format!("Organizar {}", short(&dir)),
                confirm(
                    format!("Organizar {} archivos de «{}»", plan.len(), short(&dir)),
                    format!("Sólo se moverán; no se borra nada.\n{summary}"),
                    Some(preview),
                ),
                Action::Organize { dir, plan },
            )
        }
        "analizar_proyecto" => {
            let dir = ctx.scope.existing(&req(input, "ruta")?)?;
            if !dir.is_dir() {
                return Err("Indica la carpeta del proyecto.".into());
            }
            if ctx.jobs.is_none() || ctx.llm.is_none() {
                return Err("El análisis en segundo plano no está disponible ahora.".into());
            }
            p(
                format!("Analizar el proyecto {}", short(&dir)),
                Permission::Auto,
                Action::Analyze {
                    dir,
                    focus: s(input, "enfoque"),
                },
            )
        }
        other => Err(format!("No conozco la herramienta «{other}».")),
    }
}

// -------------------------------------------------------------------- run

pub async fn run(ctx: &ToolCtx, prepared: Prepared) -> Result<ToolOutput, String> {
    if let Action::SendImage(img) = prepared.action {
        ctx.db.log("pantalla", "Enviaste una captura de pantalla a la IA");
        return Ok(ToolOutput {
            text: "Captura de pantalla adjunta (autorizada por el usuario).".into(),
            images: vec![img],
        });
    }
    run_text(ctx, prepared).await.map(|text| ToolOutput { text, images: vec![] })
}

async fn run_text(ctx: &ToolCtx, prepared: Prepared) -> Result<String, String> {
    let db = &ctx.db;
    match prepared.action {
        Action::SendImage(_) => Ok(String::new()),
        Action::Remember { kind, content } => {
            let m = db.memory_add(&kind, &content, "canela")?;
            db.log("memoria", &format!("Canela recordó: {}", m.content));
            (ctx.notify)("memory");
            Ok(format!("Guardado en memoria como #{} ({}).", m.id, m.kind))
        }
        Action::SearchMemory { query, kind } => {
            let items = db.memory_list(kind.as_deref(), query.as_deref(), 40)?;
            if items.is_empty() {
                return Ok("No hay recuerdos que coincidan.".into());
            }
            Ok(items
                .iter()
                .map(|m| format!("#{} [{}] {}", m.id, m.kind, m.content))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Action::Forget { id } => {
            db.memory_delete(id)?;
            db.log("memoria", &format!("Canela olvidó el recuerdo #{id}"));
            (ctx.notify)("memory");
            Ok(format!("Recuerdo #{id} borrado."))
        }
        Action::SearchConversations { query } => {
            let found = db.conversation_search(&query, 5)?;
            if found.is_empty() {
                return Ok("No encontré nada en conversaciones anteriores.".into());
            }
            Ok(found
                .iter()
                .map(|(m, snip)| format!("«{}» ({}): …{}…", m.title, &m.updated_at[..10], snip))
                .collect::<Vec<_>>()
                .join("\n\n"))
        }
        Action::AddTask(input) => {
            let t = db.task_add(&input)?;
            db.log("tarea", &format!("Canela creó: {}", t.title));
            (ctx.notify)("tasks");
            Ok(format!("Tarea creada: {}", task_line(&t)))
        }
        Action::ListTasks(filter) => {
            let tasks = db.task_list(filter)?;
            if tasks.is_empty() {
                return Ok("No hay tareas en esa lista.".into());
            }
            Ok(format!(
                "Hoy es {}.\n{}",
                crate::db::today_str(),
                tasks.iter().map(task_line).collect::<Vec<_>>().join("\n")
            ))
        }
        Action::UpdateTask { id, input, done } => {
            let changed = input.title.is_some() || input.priority.is_some() || input.due.is_some() || input.remind_at.is_some();
            let mut t = if changed {
                db.task_update(id, &input)?
            } else {
                db.task_get(id)?.ok_or("Tarea no encontrada.")?
            };
            if let Some(d) = done {
                t = db.task_set_done(id, d)?;
            }
            db.log("tarea", &format!("Canela actualizó: {}", t.title));
            (ctx.notify)("tasks");
            Ok(format!("Tarea actualizada: {}", task_line(&t)))
        }
        Action::DeleteTask { id } => {
            db.task_delete(id)?;
            db.log("tarea", &format!("Canela eliminó la tarea #{id}"));
            (ctx.notify)("tasks");
            Ok(format!("Tarea #{id} eliminada."))
        }
        Action::OpenApp(target) => {
            apps::launch(&target)?;
            db.log("aplicación", &format!("Abrió {}", target.label()));
            Ok(format!("Abrí {}.", target.label()))
        }
        Action::OpenWeb { url, browser } => {
            let used = apps::open_url(&url, browser.as_deref())?;
            db.log("web", &format!("Abrió {url}"));
            Ok(format!("Abrí {url} en {used}."))
        }
        Action::ListDir(None) => Ok(format!(
            "Carpetas autorizadas:\n{}",
            ctx.scope.roots.iter().map(|r| display(r)).collect::<Vec<_>>().join("\n")
        )),
        Action::ListDir(Some(dir)) => {
            let entries = files::list_dir(&dir)?;
            let total = entries.len();
            let lines: Vec<String> = entries
                .iter()
                .take(200)
                .map(|e| {
                    if e.is_dir {
                        format!("[carpeta] {}", e.name)
                    } else {
                        format!(
                            "{} ({} KB, {})",
                            e.name,
                            e.size.div_ceil(1024),
                            e.modified.map(fmt_time).unwrap_or_default()
                        )
                    }
                })
                .collect();
            Ok(format!(
                "{} ({} elementos{}):\n{}",
                display(&dir),
                total,
                if total > 200 { ", se muestran 200" } else { "" },
                lines.join("\n")
            ))
        }
        Action::Search { query, root, only_dirs } => {
            let roots = root.map(|r| vec![r]).unwrap_or_else(|| ctx.scope.roots.clone());
            let found = tokio::task::spawn_blocking(move || files::search(&roots, &query, only_dirs, 40))
                .await
                .map_err(|e| e.to_string())?;
            if found.is_empty() {
                return Ok("No encontré coincidencias en las carpetas autorizadas.".into());
            }
            Ok(found
                .iter()
                .map(|(p, d)| format!("{}{}", if *d { "[carpeta] " } else { "" }, display(p)))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Action::Recent { limit } => {
            let roots = ctx.scope.roots.clone();
            let items = tokio::task::spawn_blocking(move || files::recent(&roots, limit))
                .await
                .map_err(|e| e.to_string())?;
            if items.is_empty() {
                return Ok("No encontré archivos recientes.".into());
            }
            Ok(items
                .iter()
                .map(|(p, t)| format!("{}  ({})", display(p), fmt_time(*t)))
                .collect::<Vec<_>>()
                .join("\n"))
        }
        Action::Read(path) => {
            let text = files::read_text(&path)?;
            Ok(format!(
                "Contenido de {} ({} líneas):\n{}",
                display(&path),
                text.lines().count(),
                text
            ))
        }
        Action::OpenPath(path) => {
            apps::open_with_shell(&display(&path)).map_err(|e| format!("No pude abrirlo: {e}"))?;
            db.log("archivo", &format!("Abrió {}", display(&path)));
            Ok(format!("Abrí {}.", display(&path)))
        }
        Action::CreateDir(path) => {
            std::fs::create_dir_all(&path).map_err(|e| format!("No pude crear la carpeta: {e}"))?;
            db.log("archivo", &format!("Creó la carpeta {}", display(&path)));
            Ok(format!("Carpeta creada: {}", display(&path)))
        }
        Action::Write { path, content } => {
            let backup = files::write_with_backup(&path, &content, &ctx.backups_dir)?;
            db.log("archivo", &format!("Escribió {}", display(&path)));
            Ok(match backup {
                Some(b) => format!("Archivo actualizado: {}. Copia del original: {}", display(&path), display(&b)),
                None => format!("Archivo creado: {}", display(&path)),
            })
        }
        Action::Organize { dir, plan } => {
            let d = dir.clone();
            let (moved, errors) = tokio::task::spawn_blocking(move || files::organize_apply(&d, &plan))
                .await
                .map_err(|e| e.to_string())?;
            db.log("archivo", &format!("Organizó {} ({moved} archivos)", display(&dir)));
            let mut msg = format!("Moví {moved} archivos a subcarpetas dentro de {}.", display(&dir));
            if !errors.is_empty() {
                msg.push_str(&format!(
                    " No pude mover {}: {}",
                    errors.len(),
                    errors.iter().take(5).cloned().collect::<Vec<_>>().join("; ")
                ));
            }
            Ok(msg)
        }
        Action::Analyze { dir, focus } => {
            let jobs = ctx.jobs.clone().ok_or("Trabajos no disponibles.")?;
            let llm = ctx.llm.clone().ok_or("IA no disponible.")?;
            let title = format!(
                "Analizar {}",
                dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
            );
            let id = jobs.spawn(&title, move |progress| async move {
                progress.set("Leyendo la estructura del proyecto…");
                let d = dir.clone();
                let p2 = progress.clone();
                let snapshot = tokio::task::spawn_blocking(move || files::project_snapshot(&d, &|m| p2.set(m)))
                    .await
                    .map_err(|e| e.to_string())??;
                progress.set("Preparando el informe…");
                analyze_with_llm(&llm, &snapshot, focus.as_deref()).await
            })?;
            db.log("trabajo", &title);
            Ok(format!("Empecé el análisis en segundo plano (trabajo {id}). Avisaré en el chat cuando termine; mientras tanto podemos seguir hablando."))
        }
    }
}

/// Pide a la IA (sin herramientas) un informe del proyecto.
async fn analyze_with_llm(llm: &LlmAccess, snapshot: &str, focus: Option<&str>) -> Result<String, String> {
    let system = ai::system_prompt(&llm.settings.user_name, "", false);
    let prompt = format!(
        "Analiza este proyecto de software. Explica en español claro: qué es, tecnologías, cómo está organizado, \
posibles problemas y 3-5 mejoras concretas priorizadas.{}\n\n{snapshot}",
        focus.map(|f| format!(" Presta especial atención a: {f}.")).unwrap_or_default()
    );
    let messages = vec![json!({ "role": "user", "content": prompt })];
    let (req, parser) = ai::build_turn(&llm.http, &llm.settings, &llm.api_key, &system, &messages, &[]);
    let model = ai::model_of(&llm.settings);
    let text = Arc::new(std::sync::Mutex::new(String::new()));
    let sink = text.clone();
    let emit = move |e: ai::StreamEvent| {
        if let ai::StreamEvent::Delta { text } = e {
            sink.lock().unwrap().push_str(&text);
        }
    };
    ai::run_turn(llm.settings.provider, &model, req, parser, &emit).await?;
    let out = text.lock().unwrap().clone();
    if out.trim().is_empty() {
        Err("La IA no devolvió ningún informe.".into())
    } else {
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn ctx(root: &std::path::Path) -> ToolCtx {
        ToolCtx {
            db: Arc::new(Db::open_in_memory().unwrap()),
            settings: Settings {
                auto_open_apps: true,
                auto_open_web: false,
                ..Settings::default()
            },
            scope: Scope::new(vec![root.to_path_buf()], vec![("descargas".into(), root.to_path_buf())]),
            backups_dir: root.join(".bak"),
            screen: Some(Arc::new(FakeScreen)),
            jobs: None,
            llm: None,
            notify: Arc::new(|_| {}),
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("canela-tools-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    struct FakeScreen;
    impl ScreenCapturer for FakeScreen {
        fn capture(&self, _t: vision::Target) -> BoxFut<'_, Result<image::RgbaImage, String>> {
            Box::pin(async { Ok(image::RgbaImage::from_pixel(2560, 1440, image::Rgba([20, 20, 20, 255]))) })
        }
        fn find_window(&self, title: &str) -> Option<u32> {
            title.to_lowercase().contains("code").then_some(7)
        }
    }

    #[tokio::test]
    async fn screen_tool_always_asks_and_sends_only_after() {
        let root = tmp("screen");
        let c = ctx(&root);
        let p = prepare_async(&c, "ver_pantalla", &json!({})).await.unwrap();
        match &p.permission {
            Permission::Confirm { image: Some(url), .. } => assert!(url.starts_with("data:image/jpeg;base64,")),
            _ => panic!("ver_pantalla debe pedir permiso mostrando la captura"),
        }
        let out = run(&c, p).await.unwrap();
        assert_eq!(out.images.len(), 1);
        assert!(out.images[0].width <= vision::MAX_SIDE);
        assert!(prepare_async(&c, "ver_pantalla", &json!({"que":"ventana","ventana":"Bitwarden"}))
            .await
            .is_err());
        assert!(
            prepare_async(&c, "ver_pantalla", &json!({"que":"ventana","ventana":"Visual Studio Code"}))
                .await
                .is_ok()
        );
        // Con un modelo sin visión, se rechaza antes de capturar.
        let mut c2 = ctx(&root);
        c2.settings.provider = crate::settings::ProviderId::Openai;
        c2.settings.openai_model = "gpt-3.5-turbo".into();
        assert!(prepare_async(&c2, "ver_pantalla", &json!({})).await.is_err());
        let _ = fs::remove_dir_all(&root);
    }

    fn is_auto(p: &Prepared) -> bool {
        matches!(p.permission, Permission::Auto)
    }

    #[test]
    fn every_definition_has_a_valid_schema() {
        let defs = definitions();
        assert!(defs.len() >= 18);
        for d in defs {
            assert_eq!(d.schema["type"], "object", "{}", d.name);
            assert!(d.description.len() > 20, "{}", d.name);
            assert!(d.name.chars().all(|c| c.is_ascii_lowercase() || c == '_'), "{}", d.name);
        }
    }

    #[tokio::test]
    async fn memory_tools_and_sensitive_filter() {
        let root = tmp("mem");
        let c = ctx(&root);
        let p = prepare(&c, "recordar", &json!({"tipo":"proyecto","contenido":"Desarrolla una página web"})).unwrap();
        assert!(is_auto(&p));
        assert!(run(&c, p).await.unwrap().text.contains("Guardado"));
        assert!(prepare(&c, "recordar", &json!({"tipo":"dato","contenido":"mi contraseña es hunter2"})).is_err());
        let out = run(&c, prepare(&c, "buscar_recuerdos", &json!({"consulta":"web"})).unwrap())
            .await
            .unwrap()
            .text;
        assert!(out.contains("página web"));
        let forget = prepare(&c, "olvidar", &json!({"id": 1})).unwrap();
        assert!(!is_auto(&forget), "olvidar debe pedir confirmación");
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn task_tools() {
        let root = tmp("tasks");
        let c = ctx(&root);
        run(
            &c,
            prepare(
                &c,
                "crear_tarea",
                &json!({"titulo":"Estudiar programación","fecha": crate::db::today_str(), "prioridad":"alta"}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
        assert!(prepare(&c, "crear_tarea", &json!({"titulo":"x","fecha":"mañana"})).is_err());
        let today = run(&c, prepare(&c, "listar_tareas", &json!({"filtro":"hoy"})).unwrap())
            .await
            .unwrap()
            .text;
        assert!(today.contains("Estudiar programación"));
        let done = run(&c, prepare(&c, "actualizar_tarea", &json!({"id": 1, "completada": true})).unwrap())
            .await
            .unwrap()
            .text;
        assert!(done.contains("completada"));
        assert!(!is_auto(&prepare(&c, "eliminar_tarea", &json!({"id": 1})).unwrap()));
        assert!(prepare(&c, "eliminar_tarea", &json!({"id": 99})).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn file_tools_respect_scope_and_permissions() {
        let root = tmp("files");
        fs::write(root.join("a.jpg"), "x").unwrap();
        fs::write(root.join("notas.txt"), "hola\n").unwrap();
        let c = ctx(&root);
        // Lectura: automática
        let r = prepare(&c, "leer_archivo", &json!({"ruta":"Descargas/notas.txt"})).unwrap();
        assert!(is_auto(&r));
        assert!(run(&c, r).await.unwrap().text.contains("hola"));
        // Fuera del ámbito: rechazado
        assert!(prepare(
            &c,
            "leer_archivo",
            &json!({"ruta": std::env::temp_dir().join("otro.txt").to_string_lossy()})
        )
        .is_err());
        // Escribir: requiere confirmación y muestra diff
        let w = prepare(
            &c,
            "escribir_archivo",
            &json!({"ruta":"Descargas/notas.txt","contenido":"hola\nmundo\n"}),
        )
        .unwrap();
        match &w.permission {
            Permission::Confirm { diff: Some(d), .. } => assert!(d.contains("+mundo")),
            _ => panic!("debe pedir confirmación con diff"),
        }
        run(&c, w).await.unwrap();
        assert_eq!(fs::read_to_string(root.join("notas.txt")).unwrap(), "hola\nmundo\n");
        // Ejecutables: bloqueados
        assert!(prepare(&c, "escribir_archivo", &json!({"ruta":"Descargas/x.bat","contenido":"del *"})).is_err());
        // Crear carpeta y organizar: confirmación
        assert!(!is_auto(
            &prepare(&c, "crear_carpeta", &json!({"ruta":"Descargas/Universidad"})).unwrap()
        ));
        let org = prepare(&c, "organizar_carpeta", &json!({"ruta":"Descargas"})).unwrap();
        assert!(!is_auto(&org));
        assert!(run(&c, org).await.unwrap().text.contains("Moví 2"));
        assert!(root.join("Imágenes/a.jpg").exists());
        // Buscar y recientes
        let found = run(&c, prepare(&c, "buscar_archivos", &json!({"texto":"notas"})).unwrap())
            .await
            .unwrap()
            .text;
        assert!(found.contains("notas.txt"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn web_requires_confirmation_by_default() {
        let root = tmp("web");
        let c = ctx(&root);
        let w = prepare(&c, "abrir_web", &json!({"busqueda":"React hooks"})).unwrap();
        assert!(!is_auto(&w));
        assert!(prepare(&c, "abrir_web", &json!({"url":"file:///C:/"})).is_err());
        assert!(prepare(&c, "herramienta_inventada", &json!({})).is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
