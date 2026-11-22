//! Verificación avanzada de tareas (v0.8, en desarrollo).
//!
//! Canela NO da un trabajo por terminado porque la respuesta suene bien:
//! 1. De la petición se deducen **objetivos comprobables** («crea una tarea»,
//!    «guarda el informe en un archivo», «investiga…»), con reglas
//!    explicables y sin llamar a ningún modelo.
//! 2. Cada objetivo se contrasta con lo que el PROGRAMA registró que se hizo
//!    (acciones ejecutadas y su comprobación objetiva), no con lo que dice el
//!    modelo.
//! 3. Resultado: completo, parcial o no logrado, con qué falló y cómo
//!    arreglarlo. Si la respuesta afirma haber terminado y la comprobación no
//!    lo confirma, se avisa.
//! 4. Comprobaciones extra: sintaxis básica del código escrito (sin ejecutar
//!    nada) y datos numéricos de una investigación frente a las fuentes leídas.

use crate::agents::ActionRecord;
use crate::coordination::{split_clauses, verb_base};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// Qué se pidió («Crear la tarea "Pagar la luz"»).
    pub what: String,
    /// «cumplido» (hecho y verificado) | «sin_verificar» (hecho, pero su efecto
    /// no se puede comprobar) | «parcial» (una parte) | «fallido» |
    /// «no_intentado» (sólo hubo texto) | «no_verificable» (afirmaciones sin
    /// respaldo en la evidencia disponible)
    pub status: String,
    /// Prueba de que se hizo (qué acción y qué comprobación).
    pub evidence: Option<String>,
    /// Qué falló, en palabras sencillas.
    pub problem: Option<String>,
    /// Cómo arreglarlo.
    pub fix: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    /// «completo» (todo hecho y verificado) | «parcial» | «no_logrado» |
    /// «no_verificable» (hecho, pero sin evidencia suficiente para confirmarlo
    /// todo) | «sin_comprobar» (charla: no había nada que comprobar)
    pub status: String,
    pub items: Vec<Item>,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Goal {
    CrearTarea,
    EliminarTarea,
    Recordar,
    CrearCarpeta,
    EscribirArchivo,
    Organizar,
    Investigar,
    Leer,
    Diagnostico,
    Multimedia,
    AbrirApp,
    ConsultarTareas,
    ActualizarTarea,
    Rutina,
    Comprobar,
}

impl Goal {
    /// Herramientas que cumplen el objetivo.
    pub fn tools(self) -> &'static [&'static str] {
        match self {
            Goal::CrearTarea => &["crear_tarea"],
            Goal::EliminarTarea => &["eliminar_tarea"],
            Goal::Recordar => &["recordar", "crear_tarea"],
            Goal::CrearCarpeta => &["crear_carpeta"],
            Goal::EscribirArchivo => &["escribir_archivo"],
            Goal::Organizar => &["organizar_carpeta"],
            Goal::Investigar => &["buscar_wikipedia", "leer_pagina"],
            Goal::Leer => &[
                "leer_archivo",
                "leer_documento",
                "analizar_proyecto",
                "listar_carpeta",
                "buscar_archivos",
                "buscar_en_documentos",
                "archivos_recientes",
                "ver_imagen",
            ],
            Goal::Diagnostico => &["info_sistema", "diagnostico_memoria"],
            Goal::Multimedia => &["controlar_multimedia"],
            Goal::AbrirApp => &["abrir_aplicacion", "abrir_con", "abrir_ruta", "abrir_web"],
            Goal::ConsultarTareas => &["listar_tareas"],
            Goal::ActualizarTarea => &["actualizar_tarea"],
            Goal::Rutina => &["ejecutar_rutina", "ejecutar_plan"],
            Goal::Comprobar => &["comprobar_codigo"],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expectation {
    pub goal: Goal,
    /// La parte de la petición de la que sale («crea una tarea para…»).
    pub part: String,
    /// Mínimo de acciones correctas («crea 3 tareas» → 3).
    pub min: usize,
}

fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' => 'a',
            'é' => 'e',
            'í' => 'i',
            'ó' => 'o',
            'ú' | 'ü' => 'u',
            c => c,
        })
        .collect()
}

fn has(t: &str, words: &[&str]) -> bool {
    words.iter().any(|w| crate::agents::kw_find(t, w).is_some())
}

/// «crea 3 tareas», «dos tareas» → 3, 2.
fn count_before(t: &str, noun: &str) -> usize {
    let words: Vec<&str> = t.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        if w.starts_with(noun) && i > 0 {
            let prev = words[i - 1];
            if let Ok(n) = prev.parse::<usize>() {
                return n.clamp(1, 20);
            }
            let n = match prev {
                "dos" => 2,
                "tres" => 3,
                "cuatro" => 4,
                "cinco" => 5,
                _ => 0,
            };
            if n > 0 {
                return n;
            }
        }
    }
    1
}

const CODE_WORDS: &[&str] = &[
    "codigo", "script", "funcion", "readme", "programa", ".rs", ".py", ".js", ".ts", ".json", ".html", ".css",
];
const FILE_WORDS: &[&str] = &[
    "archivo",
    "documento",
    "fichero",
    ".txt",
    ".md",
    ".csv",
    ".json",
    ".py",
    ".js",
    ".ts",
    ".rs",
    ".html",
    ".css",
    "readme",
    "nota de texto",
];

/// Objetivos comprobables de una petición (sólo lo que escribió el usuario).
pub fn expectations(request: &str) -> Vec<Expectation> {
    let user = request.split("[Archivo adjunto:").next().unwrap_or(request);
    let mut out: Vec<Expectation> = Vec::new();
    for part in split_clauses(user) {
        let t = fold(&part);
        let system_topic = crate::agents::keyword_agents(&t)
            .iter()
            .any(|(a, _)| *a == crate::agents::AgentId::Sistema);
        let Some(verb) = part.split_whitespace().find_map(verb_base) else {
            // Pregunta sobre el equipo sin verbo («¿cuánta RAM uso?»): se responde midiendo.
            let question = has(&t, &["cuanta", "cuanto", "cuantos", "cuantas", "que", "como", "por que", "cuales"]);
            if question && has(&t, &["tarea", "pendiente"]) {
                // «¿Qué tareas tengo hoy?»: se responde consultándolas.
                out.push(Expectation {
                    goal: Goal::ConsultarTareas,
                    part: part.clone(),
                    min: 1,
                });
            } else if system_topic && question {
                out.push(Expectation {
                    goal: Goal::Diagnostico,
                    part: part.clone(),
                    min: 1,
                });
            }
            continue;
        };
        let mut push = |goal: Goal, min: usize| {
            if !out.iter().any(|e| e.goal == goal && e.part == part) {
                out.push(Expectation {
                    goal,
                    part: part.clone(),
                    min,
                });
            }
        };
        let is = |vs: &[&str]| vs.contains(&verb);
        if is(&["ejecuta", "inicia", "activa"]) && has(&t, &["rutina", "plan", "automatizacion"]) {
            push(Goal::Rutina, 1);
        } else if is(&["ejecuta", "prueba", "comprueba", "verifica", "compila", "revisa", "corrige"])
            && has(
                &t,
                &["pruebas", "prueba", "tests", "test", "compila", "funciona", "sintaxis", "build"],
            )
            && !(is(&["corrige"]) && !has(&t, &["comprueba", "funciona", "pruebas", "test"]))
        {
            push(Goal::Comprobar, 1);
        } else if is(&["marca", "completa", "actualiza", "cambia", "edita"]) && has(&t, &["tarea"]) {
            push(Goal::ActualizarTarea, 1);
        } else if is(&["borra", "elimina"]) && has(&t, &["tarea"]) {
            push(Goal::EliminarTarea, 1);
        } else if is(&["crea", "agenda", "apunta", "anota", "agrega", "anade", "programa", "pon"])
            && has(&t, &["tarea", "recordatorio", "pendiente", "alarma", "cita", "evento"])
        {
            let n = count_before(&t, "tarea");
            push(Goal::CrearTarea, if has(&t, &["por cada", "cada uno"]) { 1 } else { n });
        } else if is(&["recuerda", "avisa"]) {
            push(Goal::Recordar, 1);
        } else if is(&["dime", "muestra", "revisa", "mira", "consulta", "lista", "enumera"]) && has(&t, &["tarea", "pendiente"]) {
            push(Goal::ConsultarTareas, 1);
        } else if is(&["crea", "haz", "genera"]) && has(&t, &["carpeta"]) && !has(&t, FILE_WORDS) {
            push(Goal::CrearCarpeta, 1);
        } else if is(&[
            "guarda",
            "escribe",
            "crea",
            "redacta",
            "genera",
            "actualiza",
            "corrige",
            "arregla",
            "documenta",
        ]) && (has(&t, FILE_WORDS) || (is(&["corrige", "arregla"]) && has(&t, CODE_WORDS)))
        {
            push(Goal::EscribirArchivo, 1);
        } else if is(&["organiza", "ordena", "mueve", "clasifica", "agrupa"])
            && has(
                &t,
                &["carpeta", "descargas", "archivos", "fotos", "escritorio", "documentos", "imagenes"],
            )
        {
            push(Goal::Organizar, 1);
        } else if is(&["investiga", "averigua"])
            || (is(&["busca", "consulta", "lee"])
                && has(
                    &t,
                    &[
                        "informacion",
                        "internet",
                        "wikipedia",
                        "en la web",
                        "articulo",
                        "noticias",
                        "http://",
                        "https://",
                        "pagina",
                    ],
                ))
        {
            push(Goal::Investigar, 1);
        } else if is(&["lee", "revisa", "analiza", "mira", "busca", "encuentra", "explica", "escanea"])
            && has(
                &t,
                &[
                    "archivo",
                    "pdf",
                    "documento",
                    "codigo",
                    "proyecto",
                    "script",
                    "csv",
                    "word",
                    "excel",
                    "carpeta",
                    "repositorio",
                    "foto",
                    "imagen",
                ],
            )
        {
            push(Goal::Leer, 1);
        } else if is(&[
            "revisa",
            "mira",
            "dime",
            "diagnostica",
            "comprueba",
            "checa",
            "mide",
            "analiza",
            "verifica",
        ]) && system_topic
        {
            push(Goal::Diagnostico, 1);
        } else if is(&["pon", "pausa", "sube", "baja", "silencia", "reproduce", "apaga"])
            && has(&t, &["musica", "cancion", "volumen", "playlist"])
        {
            push(Goal::Multimedia, 1);
        } else if is(&["abre"])
            && has(
                &t,
                &[
                    "calculadora",
                    "spotify",
                    "navegador",
                    "vs code",
                    "aplicacion",
                    "bloc de notas",
                    "carpeta",
                ],
            )
        {
            push(Goal::AbrirApp, 1);
        }
    }
    out
}

/// Cómo arreglar un problema, según el mensaje de error o de comprobación.
pub fn suggest_fix(detail: &str) -> String {
    let d = fold(detail);
    let fix = if d.contains("no tengo permiso para acceder") || d.contains("ajustes › permisos") {
        "Autoriza esa carpeta en Ajustes › Permisos o usa una carpeta que ya esté autorizada."
    } else if d.contains("no encontre") || d.contains("no existe") {
        "Comprueba el nombre o la ruta; puedo buscarlo con «busca el archivo…»."
    } else if d.contains("sintaxis") || d.contains("sin cerrar") || d.contains("sin abrir") || d.contains("json no valido") {
        "El archivo se guardó, pero el código parece tener un error. Puedo corregirlo si me lo pides (no se ejecutó nada)."
    } else if d.contains("contenido es distinto") {
        "Otro programa pudo cambiar el archivo justo después. Revísalo o pídeme escribirlo de nuevo."
    } else if d.contains("temporal")
        || d.contains("tardo demasiado")
        || d.contains("conexion")
        || d.contains("red")
        || d.contains("problema interno")
    {
        "Fue un error temporal; vuelve a pedírmelo en un momento."
    } else if d.contains("no autorizo") || d.contains("no autoriz") {
        "No lo autorizaste. Si quieres hacerlo, pídemelo de nuevo y pulsa Permitir."
    } else if d.contains("no tiene permiso para usar") {
        "Esa acción es de otro agente. Pídemelo de nuevo y lo hará el agente adecuado."
    } else if d.contains("limite de gasto") {
        "Se alcanzó tu límite de gasto. Súbelo en Ajustes si quieres continuar."
    } else {
        "Revisa el detalle y pídemelo de nuevo si quieres que lo intente."
    };
    fix.to_string()
}

fn label_of(goal: Goal, part: &str) -> String {
    let what = match goal {
        Goal::CrearTarea => "Crear la tarea",
        Goal::EliminarTarea => "Eliminar la tarea",
        Goal::Recordar => "Guardar el recordatorio",
        Goal::CrearCarpeta => "Crear la carpeta",
        Goal::EscribirArchivo => "Escribir el archivo",
        Goal::Organizar => "Organizar los archivos",
        Goal::Investigar => "Consultar fuentes",
        Goal::Leer => "Leer o revisar el contenido",
        Goal::Diagnostico => "Medir el estado del equipo",
        Goal::Multimedia => "Controlar la música o el volumen",
        Goal::AbrirApp => "Abrir lo pedido",
        Goal::ConsultarTareas => "Consultar tus tareas",
        Goal::ActualizarTarea => "Actualizar la tarea",
        Goal::Rutina => "Ejecutar la rutina o el plan",
        Goal::Comprobar => "Comprobar el código (pruebas, compilación o sintaxis)",
    };
    let p: String = part.chars().take(70).collect();
    format!("{what} («{}»)", p.trim())
}

// ------------------------------------------------------------ comprobaciones extra

/// Números del texto (≥ 2 cifras), sin separadores: «1.887», «1887» → «1887».
fn numbers(text: &str) -> Vec<String> {
    let no_urls: String = text
        .split_whitespace()
        .filter(|w| !w.contains("://") && !w.starts_with("www."))
        .collect::<Vec<_>>()
        .join(" ");
    let mut out = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = no_urls.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if c.is_ascii_digit() {
            cur.push(*c);
        } else if (*c == '.' || *c == ',') && !cur.is_empty() && chars.get(i + 1).is_some_and(|n| n.is_ascii_digit()) {
            // separador de miles o decimales: se ignora
        } else {
            if cur.len() >= 2 {
                out.push(std::mem::take(&mut cur));
            }
            cur.clear();
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

/// Datos numéricos de una respuesta que NO aparecen en las fuentes leídas
/// ni en la petición (posibles datos inventados).
pub fn unsupported_numbers(answer: &str, fetched: &str, request: &str) -> Vec<String> {
    let known: Vec<String> = numbers(fetched).into_iter().chain(numbers(request)).collect();
    let mut out: Vec<String> = Vec::new();
    for n in numbers(answer) {
        if !known.contains(&n) && !out.contains(&n) {
            out.push(n);
        }
    }
    out.truncate(5);
    out
}

/// URLs de «Fuente: …» que se leyeron.
fn fetched_sources(fetched: &str) -> Vec<String> {
    fetched
        .lines()
        .filter_map(|l| l.trim().strip_prefix("Fuente: "))
        .map(|u| u.trim().to_string())
        .collect()
}

/// Comprobación estática y básica del código escrito (NO lo ejecuta):
/// JSON válido, paréntesis/llaves/corchetes equilibrados, comillas cerradas,
/// y en Python/YAML, sangría sin mezclar tabulaciones.
pub fn code_check(path: &str, content: &str) -> Vec<String> {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    let mut problems = Vec::new();
    if ext == "json" {
        if let Err(e) = serde_json::from_str::<serde_json::Value>(content) {
            problems.push(format!("JSON no válido: {e}"));
        }
        return problems;
    }
    let c_like = [
        "rs", "js", "jsx", "ts", "tsx", "c", "h", "cpp", "hpp", "cs", "java", "go", "kt", "swift", "php", "css", "scss", "dart",
    ];
    let hash_comments = ["py", "sh", "rb", "yaml", "yml", "toml", "ps1"];
    let code = c_like.contains(&ext.as_str()) || hash_comments.contains(&ext.as_str());
    if !code {
        return problems;
    }
    if ["py", "yaml", "yml"].contains(&ext.as_str()) {
        let (mut tabs, mut spaces) = (None, None);
        for (i, l) in content.lines().enumerate() {
            let indent: String = l.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
            if indent.contains('\t') {
                tabs.get_or_insert(i + 1);
            }
            if indent.contains(' ') {
                spaces.get_or_insert(i + 1);
            }
        }
        if ext != "py" {
            if let Some(l) = tabs {
                problems.push(format!("YAML no admite tabulaciones en la sangría (línea {l})"));
            }
        } else if let (Some(a), Some(b)) = (tabs, spaces) {
            problems.push(format!("mezcla tabulaciones y espacios en la sangría (líneas {a} y {b})"));
        }
    }
    let quotes: &[char] = match ext.as_str() {
        "rs" => &['"'],
        "js" | "jsx" | "ts" | "tsx" => &['"', '\'', '`'],
        _ => &['"', '\''],
    };
    let mut stack: Vec<(char, usize)> = Vec::new();
    let mut line = 1;
    let mut chars = content.chars().peekable();
    let mut in_str: Option<(char, usize)> = None;
    let (mut in_line_comment, mut in_block_comment) = (false, false);
    while let Some(c) = chars.next() {
        if c == '\n' {
            line += 1;
            in_line_comment = false;
            if let Some((q, _)) = in_str {
                // Las comillas simples y dobles no cruzan líneas (salvo `…`).
                if q != '`' && ext != "py" {
                    in_str = None;
                }
            }
            continue;
        }
        if in_line_comment {
            continue;
        }
        if in_block_comment {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block_comment = false;
            }
            continue;
        }
        if let Some((q, _)) = in_str {
            if c == '\\' {
                chars.next();
            } else if c == q {
                in_str = None;
            }
            continue;
        }
        if hash_comments.contains(&ext.as_str()) && c == '#' {
            in_line_comment = true;
            continue;
        }
        if c_like.contains(&ext.as_str()) && c == '/' {
            match chars.peek() {
                Some('/') if ext != "css" && ext != "scss" => {
                    in_line_comment = true;
                    continue;
                }
                Some('*') => {
                    chars.next();
                    in_block_comment = true;
                    continue;
                }
                _ => {}
            }
        }
        if quotes.contains(&c) {
            in_str = Some((c, line));
            continue;
        }
        match c {
            '(' | '[' | '{' => stack.push((c, line)),
            ')' | ']' | '}' => {
                let want = match c {
                    ')' => '(',
                    ']' => '[',
                    _ => '{',
                };
                match stack.pop() {
                    Some((open, _)) if open == want => {}
                    Some((open, l)) => {
                        problems.push(format!("«{open}» de la línea {l} se cierra con «{c}» en la línea {line}"));
                        break;
                    }
                    None => {
                        problems.push(format!("«{c}» sin abrir (línea {line})"));
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    if problems.is_empty() {
        if let Some((open, l)) = stack.last() {
            problems.push(format!("«{open}» de la línea {l} sin cerrar"));
        }
        if let Some((q, l)) = in_str.filter(|(q, _)| *q == '`' || ext == "py") {
            problems.push(format!("comillas {q} sin cerrar (línea {l})"));
        }
    }
    problems.truncate(3);
    problems
}

// ------------------------------------------------------------ evaluación

fn status_of(items: &[Item]) -> &'static str {
    let any = |st: &[&str]| items.iter().any(|i| st.contains(&i.status.as_str()));
    if items.is_empty() {
        "sin_comprobar"
    } else if any(&["fallido", "no_intentado", "parcial"]) {
        if any(&["cumplido", "sin_verificar", "parcial", "no_verificable"]) {
            "parcial"
        } else {
            "no_logrado"
        }
    } else if items.iter().all(|i| i.status == "cumplido") {
        "completo"
    } else {
        // Todo se hizo, pero no todo se puede confirmar con la evidencia.
        "no_verificable"
    }
}

pub fn summarize(status: &str, items: &[Item]) -> String {
    let verified = items.iter().filter(|i| i.status == "cumplido").count();
    let done = items
        .iter()
        .filter(|i| ["cumplido", "sin_verificar", "no_verificable"].contains(&i.status.as_str()))
        .count();
    let n = items.len();
    match status {
        "completo" => format!("Comprobado: {verified} de {n} objetivos cumplidos y verificados."),
        "parcial" => format!("Resultado parcial: {done} de {n} objetivos cumplidos."),
        "no_logrado" => format!("No se logró lo pedido: 0 de {n} objetivos cumplidos."),
        "no_verificable" => {
            format!("Hecho, pero no todo se puede verificar con la evidencia disponible: {verified} de {n} objetivos verificados.")
        }
        _ => "Nada que comprobar: no se pidió ninguna acción verificable.".into(),
    }
}

/// Herramientas cuya ejecución correcta ES la evidencia (consultas: el
/// programa registró qué se leyó o se midió).
fn consultation(tool: &str) -> bool {
    crate::tools::is_read_only(tool) || matches!(tool, "analizar_proyecto" | "buscar_wikipedia" | "leer_pagina")
}

const CLAIM_STOP: &[&str] = &[
    "para", "como", "este", "esta", "esto", "estos", "estas", "pero", "tambien", "segun", "fuente", "fuentes", "desde", "hasta", "sobre",
    "entre", "cuando", "donde", "porque", "tiene", "tienen", "puede", "pueden", "unos", "unas", "muy", "mas", "menos", "hace", "suele",
    "ademas", "otro", "otra", "otros", "otras", "todo", "toda", "todos", "todas", "cada", "aqui", "alli", "luego", "despues", "antes",
    "mismo", "misma", "solo", "sólo", "algo", "nada", "bien",
];

fn content_words(text: &str) -> Vec<String> {
    fold(text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 4 && !CLAIM_STOP.contains(w) && !w.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_string)
        .collect()
}

/// Afirmaciones de una respuesta sin respaldo en las fuentes leídas.
/// «Respaldada» significa que las fuentes dicen algo parecido (mismas
/// palabras con contenido), NO que sea verdad: por eso lo que no tiene
/// respaldo se marca como «no verificable», no como falso.
pub fn unsupported_claims(answer: &str, fetched: &str) -> (usize, Vec<String>) {
    let source: std::collections::HashSet<String> = content_words(fetched).into_iter().collect();
    let stems: std::collections::HashSet<String> = source.iter().map(|w| w.chars().take(5).collect()).collect();
    let mut supported = 0;
    let mut missing = Vec::new();
    for sentence in answer.split(['.', '!', '?', '\n']) {
        let low = sentence.to_lowercase();
        if low.contains("fuente") || low.contains("http") || low.contains("www.") {
            continue;
        }
        let words = content_words(sentence);
        if words.len() < 3 {
            continue;
        }
        let hits = words
            .iter()
            .filter(|w| source.contains(*w) || (w.chars().count() >= 5 && stems.contains(&w.chars().take(5).collect::<String>())))
            .count();
        if hits * 2 >= words.len() {
            supported += 1;
        } else {
            missing.push(sentence.trim().chars().take(120).collect());
        }
    }
    (supported, missing)
}

/// Nombres concretos que menciona una parte: archivos con extensión o
/// nombres entre comillas («app.js», «Facturas»).
fn hints(part: &str) -> Vec<String> {
    let mut out = Vec::new();
    for w in part.split_whitespace() {
        let w = fold(w.trim_matches(|c: char| !c.is_alphanumeric() && c != '.' && c != '_' && c != '-'));
        let w = w.trim_matches('.');
        if let Some((name, ext)) = w.rsplit_once('.') {
            if !name.is_empty() && (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
                out.push(w.to_string());
            }
        }
    }
    for (open, close) in [('«', '»'), ('"', '"'), ('\'', '\'')] {
        let mut rest = part;
        while let Some(a) = rest.find(open) {
            let after = &rest[a + open.len_utf8()..];
            let Some(b) = after.find(close) else { break };
            let q = fold(after[..b].trim());
            if !q.is_empty() && q.chars().count() <= 60 {
                out.push(q);
            }
            rest = &after[b + close.len_utf8()..];
        }
    }
    out
}

/// Resultado de una respuesta: objetivos de `request` frente a lo que se hizo.
pub fn assess(request: &str, actions: &[ActionRecord], answer: &str, fetched: &str) -> Verdict {
    let mut items: Vec<Item> = Vec::new();
    let mut used = vec![false; actions.len()];
    // Cada acción correcta cuenta para UN solo objetivo; si la parte nombra un
    // archivo («app.js»), sólo cuentan las acciones sobre ese archivo. Primero
    // se asignan los objetivos que nombran algo concreto.
    let exps = expectations(request);
    let mut consumed = vec![false; actions.len()];
    let mut order: Vec<usize> = (0..exps.len()).collect();
    order.sort_by_key(|&i| hints(&exps[i].part).is_empty());
    let mut assigned: Vec<(Vec<usize>, Vec<usize>)> = vec![(vec![], vec![]); exps.len()];
    for &k in &order {
        let e = &exps[k];
        let h = hints(&e.part);
        let of_tool: Vec<usize> = (0..actions.len())
            .filter(|&i| e.goal.tools().contains(&actions[i].tool.as_str()))
            .collect();
        let named: Vec<usize> = of_tool
            .iter()
            .copied()
            .filter(|&i| {
                let l = fold(&format!("{} {}", actions[i].label, actions[i].detail.clone().unwrap_or_default()));
                h.iter().any(|x| l.contains(x.as_str()))
            })
            .collect();
        // Éxito: sólo acciones sobre lo que se nombró (si se nombró algo).
        // Para explicar fallos, si ninguna lo nombra, sirven las de su tipo.
        let success_pool = if h.is_empty() { of_tool.clone() } else { named.clone() };
        let matching = if h.is_empty() || named.is_empty() { of_tool } else { named };
        let good: Vec<usize> = success_pool
            .iter()
            .copied()
            .filter(|&i| !consumed[i] && actions[i].status == "ok" && actions[i].verified != Some(false))
            .take(e.min)
            .collect();
        for &i in &good {
            consumed[i] = true;
        }
        assigned[k] = (matching, good);
    }
    for (k, e) in exps.into_iter().enumerate() {
        let what = label_of(e.goal, &e.part);
        let (matching, good) = &assigned[k];
        for &i in matching {
            used[i] = true;
        }
        let ok: Vec<&ActionRecord> = good.iter().map(|&i| &actions[i]).collect();
        let item = if ok.len() >= e.min {
            let consulted = ok.iter().all(|a| consultation(&a.tool));
            let verified = ok.iter().all(|a| a.verified == Some(true) || consultation(&a.tool));
            let evidence = ok.iter().map(|a| a.label.clone()).collect::<Vec<_>>().join("; ");
            if verified {
                Item {
                    what,
                    status: "cumplido".into(),
                    evidence: Some(if consulted {
                        format!("{evidence} (consulta registrada)")
                    } else {
                        format!("{evidence} (comprobado)")
                    }),
                    problem: None,
                    fix: None,
                }
            } else {
                Item {
                    what,
                    status: "sin_verificar".into(),
                    evidence: Some(evidence),
                    problem: Some("Se hizo, pero su efecto no se puede comprobar con la evidencia disponible.".into()),
                    fix: Some("Compruébalo tú mismo (por ejemplo, mira si se abrió o si suena la música).".into()),
                }
            }
        } else if !ok.is_empty() {
            Item {
                what,
                status: "parcial".into(),
                evidence: Some(ok.iter().map(|a| a.label.clone()).collect::<Vec<_>>().join("; ")),
                problem: Some(format!("Se hicieron {} de {} acciones pedidas.", ok.len(), e.min)),
                fix: Some("Pídeme las que faltan.".into()),
            }
        } else if let Some(a) = matching
            .iter()
            .map(|&i| &actions[i])
            .find(|a| a.status == "ok" && a.verified == Some(false))
        {
            let d = a.detail.clone().unwrap_or_default();
            // Avance medido: «2 de 3 pasos», «7 de 10 archivos» → parcial.
            let partial = a.progress.filter(|[done, total]| *done > 0 && done < total);
            Item {
                what,
                status: if partial.is_some() { "parcial" } else { "fallido" }.into(),
                evidence: Some(a.label.clone()),
                problem: Some(match partial {
                    Some([done, total]) => format!("Sólo se completó una parte ({done} de {total}): {d}."),
                    None => format!("La comprobación falló: {d}."),
                }),
                fix: Some(if partial.is_some() && !d.contains("sintaxis") {
                    "Revisa qué faltó y pídeme sólo eso; lo ya hecho no se repetirá.".into()
                } else {
                    suggest_fix(&d)
                }),
            }
        } else if let Some(a) = matching.iter().map(|&i| &actions[i]).find(|a| a.status != "ok") {
            let d = a.detail.clone().unwrap_or_default();
            let problem = match a.status.as_str() {
                "denegada" => "No lo autorizaste, así que no se hizo.".to_string(),
                "no permitida" => "El agente que respondía no tiene permiso para esta acción.".to_string(),
                _ => format!("Falló: {}", d.lines().next().unwrap_or("error desconocido")),
            };
            Item {
                what,
                status: "fallido".into(),
                evidence: None,
                problem: Some(problem),
                fix: Some(match a.status.as_str() {
                    "denegada" => "Si quieres hacerlo, pídemelo de nuevo y pulsa Permitir.".into(),
                    "no permitida" => "Pídemelo de nuevo: lo hará el agente adecuado.".into(),
                    _ => suggest_fix(&d),
                }),
            }
        } else {
            Item {
                what,
                status: "no_intentado".into(),
                evidence: None,
                problem: Some("No se ejecutó ninguna acción para esto: sólo hubo texto.".into()),
                fix: Some(format!("Pídemelo de nuevo de forma concreta: «{}».", e.part.trim())),
            }
        };
        let investigated = e.goal == Goal::Investigar && item.status == "cumplido";
        items.push(item);
        // Investigación: procedencia (fuentes citadas y consultadas) y
        // respaldo de lo que se afirma. Coincidir con las fuentes NO prueba que
        // algo sea verdad; lo que no coincide queda «no verificable».
        if investigated && !answer.trim().is_empty() {
            let sources = fetched_sources(fetched);
            let foreign = crate::agents::uncited_sources(answer, fetched);
            if !sources.is_empty() && !sources.iter().any(|s| answer.contains(s.as_str())) {
                items.push(Item {
                    what: "Citar las fuentes consultadas".into(),
                    status: "fallido".into(),
                    evidence: None,
                    problem: Some(if foreign.is_empty() {
                        "La respuesta no cita ninguna de las páginas que se leyeron.".into()
                    } else {
                        format!("La respuesta cita fuentes que no se consultaron: {}.", foreign.join(", "))
                    }),
                    fix: Some(format!("Las fuentes consultadas fueron: {}.", sources.join(", "))),
                });
            }
            let invented = unsupported_numbers(answer, fetched, request);
            if !invented.is_empty() {
                items.push(Item {
                    what: "Cifras de la respuesta".into(),
                    status: "no_verificable".into(),
                    evidence: None,
                    problem: Some(format!("Estas cifras no aparecen en las fuentes leídas: {}.", invented.join(", "))),
                    fix: Some("Trátalas como no confirmadas; puedo buscar otra fuente.".into()),
                });
            }
            let (backed, missing) = unsupported_claims(answer, fetched);
            items.push(if missing.is_empty() {
                Item {
                    what: "Afirmaciones con respaldo en las fuentes leídas".into(),
                    status: "cumplido".into(),
                    evidence: Some(format!(
                        "{backed} afirmaciones coinciden con lo que dicen las fuentes (coincidir no garantiza que sean ciertas)"
                    )),
                    problem: None,
                    fix: None,
                }
            } else {
                Item {
                    what: "Afirmaciones con respaldo en las fuentes leídas".into(),
                    status: "no_verificable".into(),
                    evidence: Some(format!("{backed} con respaldo")),
                    problem: Some(format!(
                        "No encontré respaldo en las fuentes leídas para: «{}».",
                        missing.iter().take(3).cloned().collect::<Vec<_>>().join("»; «")
                    )),
                    fix: Some("Trátalo como no confirmado; puedo buscar otra fuente.".into()),
                }
            });
        }
    }
    // Acciones que fallaron aunque no fueran un objetivo explícito.
    for (i, a) in actions.iter().enumerate() {
        if used[i] {
            continue;
        }
        let failed_check = a.status == "ok" && a.verified == Some(false);
        if a.status != "ok" || failed_check {
            let d = a.detail.clone().unwrap_or_default();
            items.push(Item {
                what: a.label.clone(),
                status: "fallido".into(),
                evidence: None,
                problem: Some(match a.status.as_str() {
                    "denegada" => "No lo autorizaste, así que no se hizo.".to_string(),
                    "no permitida" => "El agente que respondía no tiene permiso para esta acción.".to_string(),
                    _ if failed_check => format!("La comprobación falló: {d}."),
                    _ => format!("Falló: {}", d.lines().next().unwrap_or("error desconocido")),
                }),
                fix: Some(match a.status.as_str() {
                    "denegada" => "Si quieres hacerlo, pídemelo de nuevo y pulsa Permitir.".into(),
                    "no permitida" => "Pídemelo de nuevo: lo hará el agente adecuado.".into(),
                    _ => suggest_fix(&d),
                }),
            });
        }
    }
    let status = status_of(&items);
    Verdict {
        status: status.into(),
        summary: summarize(status, &items),
        items,
    }
}

/// ¿La respuesta dice que el trabajo está terminado?
pub fn claims_done(answer: &str) -> bool {
    let t = fold(answer);
    crate::agents::claims_action(answer)
        || [
            "listo",
            "termine",
            "terminado",
            "completado",
            "ya quedo",
            "ya esta",
            "hecho",
            "todo bien",
            "he completado",
            "quedo listo",
        ]
        .iter()
        .any(|k| crate::agents::kw_find(&t, k).is_some())
}

/// Une los resultados de varios pasos (prefijo «Paso N»).
pub fn merge(steps: &[(usize, &Verdict)]) -> Option<Verdict> {
    let mut items = Vec::new();
    for (n, v) in steps {
        for it in &v.items {
            let mut it = it.clone();
            it.what = format!("Paso {n}: {}", it.what);
            items.push(it);
        }
    }
    if items.is_empty() {
        return None;
    }
    let status = status_of(&items);
    Some(Verdict {
        status: status.into(),
        summary: summarize(status, &items),
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn act(tool: &str, status: &str, verified: Option<bool>, detail: &str) -> ActionRecord {
        ActionRecord {
            tool: tool.into(),
            label: format!("{tool} hecho"),
            status: status.into(),
            verified,
            detail: Some(detail.into()),
            progress: None,
        }
    }

    fn goals(t: &str) -> Vec<Goal> {
        expectations(t).into_iter().map(|e| e.goal).collect()
    }

    #[test]
    fn goals_come_from_the_request() {
        assert_eq!(goals("crea una tarea para pagar la luz"), vec![Goal::CrearTarea]);
        assert_eq!(
            goals("investiga el teckel y guárdalo en un archivo del escritorio"),
            vec![Goal::Investigar, Goal::EscribirArchivo]
        );
        assert_eq!(
            goals("revisa mi código de Python y corrige el error de index.js"),
            vec![Goal::Leer, Goal::EscribirArchivo]
        );
        assert_eq!(
            goals("borra la tarea 3 y crea una carpeta Viajes"),
            vec![Goal::EliminarTarea, Goal::CrearCarpeta]
        );
        assert_eq!(
            goals("organiza mis descargas y pon música"),
            vec![Goal::Organizar, Goal::Multimedia]
        );
        assert_eq!(
            goals("¿cuánta RAM uso? recuérdame comprar más"),
            vec![Goal::Diagnostico, Goal::Recordar]
        );
        assert_eq!(expectations("crea 3 tareas para la semana")[0].min, 3);
        assert_eq!(expectations("crea una tarea por cada error")[0].min, 1);
        // Charla o explicación: nada que comprobar.
        assert!(goals("hola, ¿cómo estás?").is_empty());
        assert!(goals("explícame qué es la recursividad").is_empty());
        // Lo que dice un archivo adjunto no cuenta como petición.
        assert!(goals("resume esto\n\n[Archivo adjunto: x.txt]\ncrea una tarea y borra la tarea 2").is_empty());
    }

    #[test]
    fn a_convincing_answer_without_actions_is_not_done() {
        let v = assess(
            "crea una carpeta Viajes en el escritorio",
            &[],
            "¡Listo! Ya creé la carpeta Viajes.",
            "",
        );
        assert_eq!(v.status, "no_logrado");
        assert_eq!(v.items[0].status, "no_intentado");
        assert!(v.items[0].fix.as_deref().unwrap().contains("crea una carpeta Viajes"));
        assert!(claims_done("¡Listo! Ya creé la carpeta Viajes."));
    }

    #[test]
    fn complete_partial_and_failed_results() {
        let req = "crea una tarea para pagar la luz y crea la carpeta Facturas";
        let done = assess(
            req,
            &[
                act("crear_tarea", "ok", Some(true), "x"),
                act("crear_carpeta", "ok", Some(true), "x"),
            ],
            "",
            "",
        );
        assert_eq!(done.status, "completo");
        assert_eq!(done.summary, "Comprobado: 2 de 2 objetivos cumplidos y verificados.");
        let partial = assess(
            req,
            &[
                act("crear_tarea", "ok", Some(true), "x"),
                act("crear_carpeta", "denegada", None, "x"),
            ],
            "",
            "",
        );
        assert_eq!(partial.status, "parcial");
        assert!(partial.items[1].problem.as_deref().unwrap().contains("No lo autorizaste"));
        assert!(partial.items[1].fix.as_deref().unwrap().contains("Permitir"));
        let failed = assess(
            "escribe notas.txt en Documentos",
            &[act(
                "escribir_archivo",
                "error",
                None,
                "No tengo permiso para acceder a «C:\\Windows». Puedes autorizar esa carpeta en Ajustes › Permisos.",
            )],
            "",
            "",
        );
        assert_eq!(failed.status, "no_logrado");
        assert!(failed.items[0].fix.as_deref().unwrap().contains("Ajustes › Permisos"));
    }

    #[test]
    fn counts_are_checked() {
        let v = assess(
            "crea 3 tareas para el examen",
            &[act("crear_tarea", "ok", Some(true), "x"), act("crear_tarea", "ok", Some(true), "x")],
            "",
            "",
        );
        // Hizo 2 de 3: parcial (no «no logrado»: algo sí se hizo).
        assert_eq!(v.status, "parcial");
        assert!(v.items[0].problem.as_deref().unwrap().contains("2 de 3"));
    }

    #[test]
    fn failed_verification_is_a_failure_with_a_fix() {
        let v = assess(
            "guarda el informe en informe.txt",
            &[act(
                "escribir_archivo",
                "ok",
                Some(false),
                "el archivo existe pero su contenido es distinto (10 bytes)",
            )],
            "Guardé el informe.",
            "",
        );
        assert_eq!(v.status, "no_logrado");
        assert!(v.items[0].fix.as_deref().unwrap().contains("Otro programa"));
    }

    #[test]
    fn research_must_cite_and_use_real_numbers() {
        let fetched = "Fuente: https://es.wikipedia.org/wiki/Teckel\nTítulo: Teckel\nRaza alemana reconocida en 1888, pesa unos 9 kg.";
        let ok = assess(
            "investiga el teckel",
            &[act("buscar_wikipedia", "ok", None, "x")],
            "El teckel fue reconocido en 1888. Fuente: https://es.wikipedia.org/wiki/Teckel",
            fetched,
        );
        assert_eq!(ok.status, "completo", "{ok:?}");
        let bad = assess(
            "investiga el teckel",
            &[act("buscar_wikipedia", "ok", None, "x")],
            "El teckel fue reconocido en 1870 y vive 25 años.",
            fetched,
        );
        assert_eq!(bad.status, "parcial");
        let problems: Vec<&str> = bad.items.iter().filter_map(|i| i.problem.as_deref()).collect();
        assert!(problems.iter().any(|p| p.contains("no cita")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("1870") && p.contains("25")), "{problems:?}");
        assert_eq!(unsupported_numbers("Cuesta 1.250 pesos", "precio: 1250", ""), Vec::<String>::new());
    }

    #[test]
    fn unrelated_failures_are_reported_too() {
        let v = assess("hola", &[act("abrir_web", "error", None, "La solicitud tardó demasiado.")], "", "");
        assert_eq!(v.status, "no_logrado");
        assert!(v.items[0].fix.as_deref().unwrap().contains("temporal"));
    }

    #[test]
    fn code_check_finds_basic_syntax_errors_without_running_anything() {
        assert!(code_check("a.json", r#"{"a": 1,}"#)[0].contains("JSON no válido"));
        assert!(code_check("a.json", r#"{"a": [1, 2]}"#).is_empty());
        assert_eq!(
            code_check("app.js", "function f() {\n  if (x) {\n    return 1;\n}\n"),
            vec!["«{» de la línea 1 sin cerrar"]
        );
        assert!(code_check("app.js", "const s = \"{ no cuenta }\"; // ( tampoco\nlet t = `{${x}}`;\n/* ) */").is_empty());
        assert_eq!(
            code_check("main.rs", "fn main() {\n    let v = vec![1, 2);\n}\n"),
            vec!["«[» de la línea 2 se cierra con «)» en la línea 2"]
        );
        assert!(code_check("main.rs", "fn f<'a>(x: &'a str) -> &'a str { x }").is_empty());
        assert!(code_check("s.py", "def f():\n\tif x:\n        return 1\n")[0].contains("mezcla tabulaciones"));
        assert!(code_check("s.py", "def f(x):\n    return (x + 1)  # (comentario\n").is_empty());
        assert!(code_check("c.yaml", "a:\n\tb: 1\n")[0].contains("YAML"));
        assert!(code_check("notas.txt", "{{{ (((").is_empty(), "no es código");
    }

    #[test]
    fn fixes_are_specific() {
        assert!(suggest_fix("No encontré «x».").contains("nombre o la ruta"));
        assert!(suggest_fix("Ollama tuvo un problema interno (500).").contains("temporal"));
        assert!(suggest_fix("tiene un posible error de sintaxis: «{» sin cerrar").contains("corregirlo"));
    }

    #[test]
    fn merged_plan_results_keep_the_step() {
        let a = assess("crea una tarea", &[act("crear_tarea", "ok", Some(true), "x")], "", "");
        let b = assess("crea la carpeta X", &[], "", "");
        let m = merge(&[(1, &a), (2, &b)]).unwrap();
        assert_eq!(m.status, "parcial");
        assert!(m.items[1].what.starts_with("Paso 2:"));
    }
}

/// Evaluación del veredicto con escenarios etiquetados a mano (petición,
/// acciones registradas, respuesta → resultado correcto). Escritos después de
/// implementar el verificador: el resultado es orientativo, no a ciegas.
#[cfg(test)]
mod eval {
    use super::*;

    fn a(tool: &str, status: &str, verified: Option<bool>, label: &str, detail: &str) -> ActionRecord {
        ActionRecord {
            tool: tool.into(),
            label: label.into(),
            status: status.into(),
            verified,
            detail: Some(detail.into()),
            progress: None,
        }
    }

    #[test]
    fn verdict_evaluation() {
        let wiki = "Fuente: https://es.wikipedia.org/wiki/Teckel\nRaza alemana de 1888 que pesa 9 kg.";
        let cases: Vec<(&str, Vec<ActionRecord>, &str, &str, &str)> = vec![
            (
                "crea una tarea para pagar la luz",
                vec![a("crear_tarea", "ok", Some(true), "Crear tarea «Pagar la luz»", "")],
                "Creé la tarea.",
                "",
                "completo",
            ),
            (
                "crea una tarea para pagar la luz",
                vec![],
                "¡Listo! Ya está tu tarea.",
                "",
                "no_logrado",
            ),
            (
                "crea 2 tareas: pan y leche",
                vec![a("crear_tarea", "ok", Some(true), "Crear tarea «pan»", "")],
                "Listo",
                "",
                // Reetiquetado al cerrar la v0.8 por las nuevas categorías pedidas (no por ajuste):
                // hizo 1 de 2 → parcial.
                "parcial",
            ),
            (
                "crea 2 tareas: pan y leche",
                vec![
                    a("crear_tarea", "ok", Some(true), "pan", ""),
                    a("crear_tarea", "ok", Some(true), "leche", ""),
                ],
                "",
                "",
                "completo",
            ),
            (
                "borra la tarea 3",
                vec![a("eliminar_tarea", "ok", Some(true), "Eliminar #3", "")],
                "",
                "",
                "completo",
            ),
            (
                "borra la tarea 3",
                vec![a("eliminar_tarea", "denegada", None, "Eliminar #3", "")],
                "",
                "",
                "no_logrado",
            ),
            (
                "borra la tarea 3",
                vec![a("eliminar_tarea", "ok", Some(false), "Eliminar #3", "la tarea sigue existiendo")],
                "Borrada.",
                "",
                "no_logrado",
            ),
            (
                "crea la carpeta Viajes",
                vec![a("crear_carpeta", "ok", Some(true), "Crear carpeta «Viajes»", "")],
                "",
                "",
                "completo",
            ),
            (
                "crea la carpeta Viajes",
                vec![a(
                    "crear_carpeta",
                    "error",
                    None,
                    "crear_carpeta «Viajes»",
                    "No tengo permiso para acceder a «C:\\Windows\\Viajes». Puedes autorizar esa carpeta en Ajustes › Permisos.",
                )],
                "",
                "",
                "no_logrado",
            ),
            (
                "guarda la lista en lista.txt",
                vec![a("escribir_archivo", "ok", Some(true), "Crear «lista.txt»", "")],
                "",
                "",
                "completo",
            ),
            (
                "guarda la lista en lista.txt",
                vec![a(
                    "escribir_archivo",
                    "ok",
                    Some(false),
                    "Crear «lista.txt»",
                    "el archivo NO existe",
                )],
                "Guardada.",
                "",
                "no_logrado",
            ),
            (
                "corrige el error de app.py",
                vec![
                    a("leer_archivo", "ok", None, "Leer «app.py»", ""),
                    a("escribir_archivo", "ok", Some(true), "Modificar «app.py»", ""),
                ],
                "",
                "",
                "completo",
            ),
            (
                "corrige el error de app.py",
                vec![a(
                    "escribir_archivo",
                    "ok",
                    Some(false),
                    "Modificar «app.py»",
                    "posible error de sintaxis: «(» de la línea 3 sin cerrar",
                )],
                "Corregido.",
                "",
                "no_logrado",
            ),
            (
                "corrige app.js y guarda config.json",
                vec![
                    a("escribir_archivo", "ok", Some(false), "Modificar «app.js»", "sintaxis"),
                    a("escribir_archivo", "ok", Some(true), "Crear «config.json»", ""),
                ],
                "Listo.",
                "",
                "parcial",
            ),
            (
                "revisa el código de mi proyecto",
                vec![a("analizar_proyecto", "ok", None, "Analizar proyecto", "")],
                "Encontré 2 problemas.",
                "",
                "completo",
            ),
            (
                "revisa el código de mi proyecto",
                vec![],
                "Revisé tu código: está bien.",
                "",
                "no_logrado",
            ),
            (
                "lee el PDF del contrato y crea una tarea con la fecha",
                vec![a("leer_documento", "ok", None, "Leer «contrato.pdf»", "")],
                "Vence el 5 de mayo.",
                "",
                "parcial",
            ),
            (
                "organiza mis descargas",
                vec![a("organizar_carpeta", "denegada", None, "Organizar Descargas", "")],
                "",
                "",
                "no_logrado",
            ),
            (
                "organiza mis descargas",
                vec![a("organizar_carpeta", "ok", None, "Organizar Descargas", "")],
                "",
                "",
                // Reetiquetado al cerrar la v0.8 por las nuevas categorías pedidas (no por ajuste):
                // sin comprobación del efecto → no verificable.
                "no_verificable",
            ),
            (
                "investiga el teckel",
                vec![a("buscar_wikipedia", "ok", None, "Wikipedia: Teckel", "")],
                "Raza de 1888. Fuente: https://es.wikipedia.org/wiki/Teckel",
                wiki,
                "completo",
            ),
            (
                "investiga el teckel",
                vec![a("buscar_wikipedia", "ok", None, "Wikipedia: Teckel", "")],
                "Raza de 1870. Fuente: https://es.wikipedia.org/wiki/Teckel",
                wiki,
                // Reetiquetado al cerrar la v0.8 por las nuevas categorías pedidas (no por ajuste):
                // una cifra sin respaldo es «no verificable», no falsa.
                "no_verificable",
            ),
            (
                "investiga el teckel",
                vec![a("buscar_wikipedia", "ok", None, "Wikipedia: Teckel", "")],
                "Raza alemana de 1888.",
                wiki,
                "parcial",
            ),
            ("investiga el teckel", vec![], "Según Wikipedia, es de 1888.", "", "no_logrado"),
            (
                "investiga el teckel",
                vec![a(
                    "buscar_wikipedia",
                    "error",
                    None,
                    "buscar_wikipedia «Teckel»",
                    "No pude consultar Wikipedia: tiempo agotado",
                )],
                "",
                "",
                "no_logrado",
            ),
            (
                "¿cuánta RAM estoy usando?",
                vec![a("info_sistema", "ok", None, "Estado del equipo", "")],
                "Usas 9 GB.",
                "",
                "completo",
            ),
            ("¿cuánta RAM estoy usando?", vec![], "Usas unos 9 GB.", "", "no_logrado"),
            (
                "revisa la memoria y crea una tarea para cerrar programas",
                vec![
                    a("diagnostico_memoria", "ok", None, "Diagnóstico", ""),
                    a("crear_tarea", "ok", Some(true), "Crear tarea", ""),
                ],
                "",
                "",
                "completo",
            ),
            (
                "revisa la memoria y crea una tarea para cerrar programas",
                vec![a("diagnostico_memoria", "ok", None, "Diagnóstico", "")],
                "Listo, todo hecho.",
                "",
                "parcial",
            ),
            (
                "pon música",
                vec![a("controlar_multimedia", "ok", None, "Reproducir", "")],
                "",
                "",
                // Reetiquetado al cerrar la v0.8 por las nuevas categorías pedidas (no por ajuste):
                // el efecto de reproducir no se puede comprobar.
                "no_verificable",
            ),
            (
                "abre la calculadora",
                vec![a("abrir_aplicacion", "ok", None, "Abrir calculadora", "")],
                "",
                "",
                // Reetiquetado al cerrar la v0.8 por las nuevas categorías pedidas (no por ajuste):
                // abrir una app no se puede comprobar.
                "no_verificable",
            ),
            (
                "abre la calculadora",
                vec![a("abrir_aplicacion", "no permitida", None, "abrir_aplicacion", "")],
                "",
                "",
                "no_logrado",
            ),
            (
                "recuérdame comprar pan mañana",
                vec![a("crear_tarea", "ok", Some(true), "Crear tarea «Comprar pan»", "")],
                "",
                "",
                "completo",
            ),
            (
                "recuerda que mi color favorito es el morado",
                vec![a("recordar", "ok", Some(true), "Recordar", "")],
                "",
                "",
                "completo",
            ),
            ("hola, ¿cómo estás?", vec![], "¡Guau! Muy bien.", "", "sin_comprobar"),
            (
                "explícame qué es la recursividad",
                vec![],
                "Es cuando una función se llama a sí misma.",
                "",
                "sin_comprobar",
            ),
            (
                "cuéntame un chiste",
                vec![],
                "¿Qué hace un perro con un taladro? ¡Taladrando!",
                "",
                "sin_comprobar",
            ),
            (
                "crea la carpeta Fotos y guarda dentro lista.txt",
                vec![
                    a("crear_carpeta", "ok", Some(true), "Crear carpeta «Fotos»", ""),
                    a(
                        "escribir_archivo",
                        "error",
                        None,
                        "escribir_archivo «Fotos/lista.txt»",
                        "La solicitud tardó demasiado.",
                    ),
                ],
                "",
                "",
                "parcial",
            ),
            (
                "crea una tarea y pon música",
                vec![
                    a("crear_tarea", "ok", Some(true), "Crear tarea", ""),
                    a("controlar_multimedia", "error", None, "controlar_multimedia", "no hay reproductor"),
                ],
                "",
                "",
                "parcial",
            ),
            (
                "hola",
                vec![a("abrir_web", "error", None, "abrir_web", "La solicitud tardó demasiado.")],
                "",
                "",
                "no_logrado",
            ),
            // Etiqueta corregida tras la primera medición (decía «completo»): se
            // escribió OTRO archivo, así que lo pedido no se hizo.
            (
                "guarda el resumen en resumen.md",
                vec![a("escribir_archivo", "ok", Some(true), "Crear «otro.md»", "")],
                "Guardado.",
                "",
                "no_logrado",
            ),
        ];
        let n = cases.len();
        let mut ok = 0;
        let mut false_complete = 0;
        for (req, actions, answer, fetched, expected) in &cases {
            let v = assess(req, actions, answer, fetched);
            if v.status == *expected {
                ok += 1;
            } else {
                if v.status == "completo" {
                    false_complete += 1;
                }
                eprintln!("  fallo: {req:?} → {} (esperado {expected})", v.status);
            }
        }
        eprintln!("Veredicto: {ok}/{n} escenarios correctos; «completo» falsos: {false_complete}");
        assert!(ok * 100 / n >= 80, "{ok}/{n}");
    }
}
