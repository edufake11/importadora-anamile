//! Abrir aplicaciones, páginas web y rutas. No se ejecutan comandos
//! arbitrarios: sólo se lanzan accesos directos del menú Inicio, un pequeño
//! conjunto de apps del sistema o el navegador con una URL http/https.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' => 'a',
            'é' => 'e',
            'í' => 'i',
            'ó' => 'o',
            'ú' | 'ü' => 'u',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// Nombres coloquiales -> nombre canónico.
const ALIASES: [(&str, &str); 22] = [
    ("vscode", "visual studio code"),
    ("vs code", "visual studio code"),
    ("code", "visual studio code"),
    ("visual code", "visual studio code"),
    ("chrome", "google chrome"),
    ("edge", "microsoft edge"),
    ("word", "word"),
    ("excel", "excel"),
    ("powerpoint", "powerpoint"),
    ("explorador de archivos", "@explorer"),
    ("explorador", "@explorer"),
    ("archivos", "@explorer"),
    ("mis archivos", "@explorer"),
    ("calculadora", "@calc"),
    ("bloc de notas", "@notepad"),
    ("notepad", "@notepad"),
    ("paint", "@paint"),
    ("terminal", "@terminal"),
    ("consola", "@terminal"),
    ("configuracion", "@settings"),
    ("ajustes de windows", "@settings"),
    ("configuracion de windows", "@settings"),
];

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// Acceso directo (.lnk/.url) del menú Inicio.
    Shortcut { name: String, path: PathBuf },
    /// App integrada de Windows.
    System {
        name: &'static str,
        program: &'static str,
        arg: Option<&'static str>,
    },
}

impl Target {
    pub fn label(&self) -> String {
        match self {
            Target::Shortcut { name, .. } => name.clone(),
            Target::System { name, .. } => name.to_string(),
        }
    }
}

fn system_app(key: &str) -> Option<Target> {
    let (name, program, arg) = match key {
        "@explorer" => ("Explorador de archivos", "explorer.exe", None),
        "@calc" => ("Calculadora", "calc.exe", None),
        "@notepad" => ("Bloc de notas", "notepad.exe", None),
        "@paint" => ("Paint", "mspaint.exe", None),
        "@terminal" => ("Terminal", "wt.exe", None),
        "@settings" => ("Configuración de Windows", "explorer.exe", Some("ms-settings:")),
        _ => return None,
    };
    Some(Target::System { name, program, arg })
}

/// Puntuación de coincidencia entre lo pedido y el nombre de un acceso directo.
pub fn score(query: &str, name: &str) -> u32 {
    let q = fold(query);
    let n = fold(name);
    if q.is_empty() {
        return 0;
    }
    if n == q {
        return 100;
    }
    if n.starts_with(&q) {
        return 80;
    }
    let words: Vec<&str> = q.split_whitespace().collect();
    if words.iter().all(|w| n.contains(w)) {
        return 60;
    }
    if n.contains(&q) {
        return 50;
    }
    0
}

/// Elige el mejor acceso directo; prefiere nombres cortos (evita "Desinstalar X").
pub fn best_match<'a>(query: &str, names: &'a [(String, PathBuf)]) -> Option<&'a (String, PathBuf)> {
    names
        .iter()
        .filter(|(n, _)| {
            let f = fold(n);
            !f.contains("uninstall") && !f.contains("desinstalar") && !f.contains("readme") && !f.contains("ayuda")
        })
        .map(|e| (score(query, &e.0), e))
        .filter(|(s, _)| *s > 0)
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1 .0.len().cmp(&a.1 .0.len())))
        .map(|(_, e)| e)
}

/// Accesos directos del menú Inicio (usuario + todos los usuarios).
pub fn start_menu_shortcuts() -> Vec<(String, PathBuf)> {
    let mut dirs = Vec::new();
    if let Ok(p) = std::env::var("ProgramData") {
        dirs.push(PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    if let Ok(p) = std::env::var("APPDATA") {
        dirs.push(PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs"));
    }
    let mut out = Vec::new();
    for d in dirs {
        collect_shortcuts(&d, 0, &mut out);
    }
    out
}

fn collect_shortcuts(dir: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.filter_map(Result::ok) {
        let p = e.path();
        if p.is_dir() && depth < 3 {
            collect_shortcuts(&p, depth + 1, out);
        } else if matches!(
            p.extension().and_then(|x| x.to_str()).map(|x| x.to_lowercase()).as_deref(),
            Some("lnk") | Some("url")
        ) {
            if let Some(stem) = p.file_stem() {
                out.push((stem.to_string_lossy().to_string(), p.clone()));
            }
        }
    }
}

/// Resuelve el nombre pedido a algo que se pueda abrir.
pub fn resolve(query: &str, shortcuts: &[(String, PathBuf)]) -> Option<Target> {
    let q = fold(query.trim_start_matches("la ").trim_start_matches("el "));
    let canonical = ALIASES.iter().find(|(a, _)| *a == q).map(|(_, c)| *c).unwrap_or(q.as_str());
    if let Some(t) = system_app(canonical) {
        return Some(t);
    }
    best_match(canonical, shortcuts)
        .or_else(|| best_match(&q, shortcuts))
        .map(|(n, p)| Target::Shortcut {
            name: n.clone(),
            path: p.clone(),
        })
}

pub fn launch(target: &Target) -> Result<(), String> {
    let result = match target {
        Target::Shortcut { path, .. } => open_with_shell(&path.to_string_lossy()),
        Target::System { program, arg, .. } => {
            let mut c = Command::new(program);
            if let Some(a) = arg {
                c.arg(a);
            }
            c.spawn().map(|_| ()).map_err(|e| e.to_string())
        }
    };
    result.map_err(|e| format!("No pude abrir {}: {e}", target.label()))
}

/// Abre un archivo, carpeta o URL con el programa predeterminado de Windows.
pub fn open_with_shell(target: &str) -> Result<(), String> {
    #[cfg(windows)]
    let mut c = Command::new("explorer.exe");
    #[cfg(target_os = "macos")]
    let mut c = Command::new("open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut c = Command::new("xdg-open");
    c.arg(target).spawn().map(|_| ()).map_err(|e| e.to_string())
}

// ----------------------------------------------------------------- web

pub fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(*b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Valida una URL http(s) o construye una búsqueda de Google.
pub fn web_target(url: Option<&str>, search: Option<&str>) -> Result<String, String> {
    if let Some(u) = url.map(str::trim).filter(|u| !u.is_empty()) {
        let lower = u.to_lowercase();
        let full = if lower.starts_with("http://") || lower.starts_with("https://") {
            u.to_string()
        } else if !u.contains("://") && u.contains('.') && !u.contains(' ') {
            format!("https://{u}")
        } else {
            return Err("Sólo puedo abrir direcciones web http o https.".into());
        };
        if full.chars().any(|c| c.is_control() || c == '"' || c == ' ') || full.len() > 2000 {
            return Err("La dirección web no es válida.".into());
        }
        return Ok(full);
    }
    match search.map(str::trim).filter(|s| !s.is_empty()) {
        Some(q) => Ok(format!("https://www.google.com/search?q={}", percent_encode(q))),
        None => Err("Indica una dirección web o qué quieres buscar.".into()),
    }
}

/// Ruta del navegador pedido, si está instalado (sólo Windows).
pub fn browser_path(browser: &str) -> Option<PathBuf> {
    let rel: &[&str] = match fold(browser).as_str() {
        "chrome" | "google chrome" => &[r"Google\Chrome\Application\chrome.exe"],
        "edge" | "microsoft edge" => &[r"Microsoft\Edge\Application\msedge.exe"],
        "firefox" | "mozilla firefox" => &[r"Mozilla Firefox\firefox.exe"],
        _ => return None,
    };
    let bases = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"];
    bases
        .iter()
        .filter_map(|b| std::env::var(b).ok())
        .flat_map(|b| rel.iter().map(move |r| PathBuf::from(&b).join(r)))
        .find(|p| p.exists())
}

pub fn open_url(url: &str, browser: Option<&str>) -> Result<String, String> {
    if let Some(path) = browser.and_then(browser_path) {
        Command::new(&path)
            .arg(url)
            .spawn()
            .map_err(|e| format!("No pude abrir el navegador: {e}"))?;
        return Ok(path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default());
    }
    open_with_shell(url).map_err(|e| format!("No pude abrir el navegador: {e}"))?;
    Ok("navegador predeterminado".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shortcuts() -> Vec<(String, PathBuf)> {
        [
            "Visual Studio Code",
            "Uninstall Visual Studio Code",
            "Google Chrome",
            "Discord",
            "Spotify",
            "Microsoft Edge",
        ]
        .iter()
        .map(|n| (n.to_string(), PathBuf::from(format!("C:/x/{n}.lnk"))))
        .collect()
    }

    #[test]
    fn resolves_common_names() {
        let s = shortcuts();
        assert_eq!(resolve("Visual Studio Code", &s).unwrap().label(), "Visual Studio Code");
        assert_eq!(resolve("vscode", &s).unwrap().label(), "Visual Studio Code");
        assert_eq!(resolve("Chrome", &s).unwrap().label(), "Google Chrome");
        assert_eq!(resolve("spotify", &s).unwrap().label(), "Spotify");
        assert_eq!(resolve("la calculadora", &s).unwrap().label(), "Calculadora");
        assert_eq!(resolve("configuración", &s).unwrap().label(), "Configuración de Windows");
        assert!(resolve("photoshop", &s).is_none());
    }

    #[test]
    fn web_targets_are_validated() {
        assert_eq!(web_target(Some("github.com"), None).unwrap(), "https://github.com");
        assert_eq!(
            web_target(None, Some("React hooks")).unwrap(),
            "https://www.google.com/search?q=React+hooks"
        );
        assert!(web_target(Some("file:///C:/Windows"), None).is_err());
        assert!(web_target(Some("javascript:alert(1)"), None).is_err());
        assert!(web_target(None, None).is_err());
        assert_eq!(percent_encode("ñ&"), "%C3%B1%26");
    }
}
