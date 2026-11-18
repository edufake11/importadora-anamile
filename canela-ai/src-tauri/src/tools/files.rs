//! Operaciones de archivos limitadas a las carpetas autorizadas.
//!
//! Reglas de seguridad:
//! - Toda ruta se resuelve a su forma canónica (sin `..` ni enlaces) y debe
//!   quedar DENTRO de una carpeta autorizada.
//! - Nunca se borra nada. Organizar sólo mueve; sobrescribir guarda una copia.
//! - No se abren ni se escriben ejecutables o scripts.

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

pub const MAX_READ_BYTES: u64 = 300 * 1024;
pub const MAX_WRITE_BYTES: usize = 500 * 1024;

/// Extensiones que Canela nunca abre ni escribe (pueden ejecutar código).
const BLOCKED_EXT: [&str; 22] = [
    "exe", "msi", "bat", "cmd", "com", "scr", "ps1", "psm1", "vbs", "vbe", "wsf", "wsh", "hta", "lnk", "reg", "dll", "sys", "cpl", "jar",
    "msc", "pif", "appx",
];

const SKIP_DIRS: [&str; 14] = [
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    ".cache",
    "AppData",
    "$RECYCLE.BIN",
    ".idea",
    ".gradle",
];

pub fn is_blocked(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| BLOCKED_EXT.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Quita el prefijo `\\?\` que añade `canonicalize` en Windows.
pub fn display(p: &Path) -> String {
    let s = p.to_string_lossy().to_string();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' => 'a',
            'é' | 'è' | 'ë' => 'e',
            'í' | 'ì' | 'ï' => 'i',
            'ó' | 'ò' | 'ö' => 'o',
            'ú' | 'ù' | 'ü' => 'u',
            _ => c,
        })
        .collect()
}

/// Carpetas autorizadas + alias ("descargas", "documentos", "escritorio").
#[derive(Debug, Clone, Default)]
pub struct Scope {
    pub roots: Vec<PathBuf>,
    pub aliases: Vec<(String, PathBuf)>,
}

impl Scope {
    pub fn new(roots: Vec<PathBuf>, aliases: Vec<(String, PathBuf)>) -> Self {
        let roots = roots.into_iter().filter_map(|r| fs::canonicalize(r).ok()).collect();
        Self { roots, aliases }
    }

    fn expand(&self, raw: &str) -> Result<PathBuf, String> {
        let raw = raw.trim().trim_matches('"');
        if raw.is_empty() {
            return Err("Falta la ruta.".into());
        }
        let p = PathBuf::from(raw);
        if p.is_absolute() {
            return Ok(p);
        }
        // "Descargas/fotos" o "Mis documentos\\tarea.txt"
        let mut comps = raw.split(['/', '\\']);
        let first = fold(comps.next().unwrap_or(""));
        let first = first.trim_start_matches("mis ").to_string();
        for (alias, dir) in &self.aliases {
            if fold(alias) == first {
                let mut out = dir.clone();
                for c in comps.filter(|c| !c.is_empty()) {
                    out.push(c);
                }
                return Ok(out);
            }
        }
        // Relativa a la única carpeta autorizada, si sólo hay una.
        if self.roots.len() == 1 {
            return Ok(self.roots[0].join(raw));
        }
        Err(format!(
            "No sé a qué carpeta te refieres con «{raw}». Usa una ruta completa o empieza por Documentos, Descargas o Escritorio."
        ))
    }

    fn check_inside(&self, canonical: &Path) -> Result<(), String> {
        if self.roots.iter().any(|r| canonical.starts_with(r)) {
            Ok(())
        } else {
            Err(format!(
                "No tengo permiso para acceder a «{}». Puedes autorizar esa carpeta en Ajustes › Permisos.",
                display(canonical)
            ))
        }
    }

    /// Ruta existente dentro del ámbito.
    pub fn existing(&self, raw: &str) -> Result<PathBuf, String> {
        let p = self.expand(raw)?;
        let c = fs::canonicalize(&p).map_err(|_| format!("No encontré «{}».", display(&p)))?;
        self.check_inside(&c)?;
        Ok(c)
    }

    /// Ruta que puede no existir aún (crear carpeta/archivo).
    pub fn target(&self, raw: &str) -> Result<PathBuf, String> {
        let p = self.expand(raw)?;
        if p.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err("La ruta no puede contener «..».".into());
        }
        let mut existing = p.clone();
        let mut rest: Vec<std::ffi::OsString> = Vec::new();
        while !existing.exists() {
            let name = existing.file_name().ok_or("Ruta no válida.")?.to_os_string();
            rest.push(name);
            if !existing.pop() {
                return Err("Ruta no válida.".into());
            }
        }
        let mut c = fs::canonicalize(&existing).map_err(|e| format!("Ruta no válida: {e}"))?;
        for part in rest.into_iter().rev() {
            c.push(part);
        }
        self.check_inside(&c)?;
        Ok(c)
    }
}

// ----------------------------------------------------------------- lectura

pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

pub fn list_dir(dir: &Path) -> Result<Vec<Entry>, String> {
    if !dir.is_dir() {
        return Err(format!("«{}» no es una carpeta.", display(dir)));
    }
    let mut out: Vec<Entry> = fs::read_dir(dir)
        .map_err(|e| format!("No pude abrir la carpeta: {e}"))?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let md = e.metadata().ok()?;
            Some(Entry {
                name: e.file_name().to_string_lossy().to_string(),
                is_dir: md.is_dir(),
                size: md.len(),
                modified: md.modified().ok(),
            })
        })
        .collect();
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| fold(&a.name).cmp(&fold(&b.name))));
    Ok(out)
}

fn walk(root: &Path, max_depth: usize, budget: &mut usize, visit: &mut dyn FnMut(&Path, &fs::Metadata, usize)) {
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut seen = HashSet::new();
    while let Some((dir, depth)) = stack.pop() {
        if !seen.insert(dir.clone()) {
            continue;
        }
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.filter_map(Result::ok) {
            if *budget == 0 {
                return;
            }
            *budget -= 1;
            let name = e.file_name().to_string_lossy().to_string();
            let Ok(md) = e.metadata() else { continue };
            let path = e.path();
            visit(&path, &md, depth);
            if md.is_dir()
                && depth < max_depth
                && !name.starts_with('.')
                && !SKIP_DIRS.contains(&name.as_str())
                && !md.file_type().is_symlink()
            {
                stack.push((path, depth + 1));
            }
        }
    }
}

/// Archivos (no carpetas) bajo `roots`, como mucho `max_files`.
pub fn list_files(roots: &[PathBuf], max_depth: usize, max_files: usize) -> Vec<(PathBuf, fs::Metadata)> {
    let mut out = Vec::new();
    let mut budget = 80_000;
    for root in roots {
        walk(root, max_depth, &mut budget, &mut |p, md, _| {
            if md.is_file() && out.len() < max_files {
                out.push((p.to_path_buf(), md.clone()));
            }
        });
    }
    out
}

/// Busca por nombre (sin distinguir mayúsculas ni acentos).
pub fn search(roots: &[PathBuf], query: &str, only: Option<bool>, limit: usize) -> Vec<(PathBuf, bool)> {
    let q = fold(query.trim());
    let words: Vec<&str> = q.split_whitespace().collect();
    let mut found = Vec::new();
    let mut budget = 60_000;
    for root in roots {
        walk(root, 8, &mut budget, &mut |p, md, _| {
            if found.len() >= limit {
                return;
            }
            if only.is_some_and(|dirs| dirs != md.is_dir()) {
                return;
            }
            let name = fold(&p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
            if !words.is_empty() && words.iter().all(|w| name.contains(w)) {
                found.push((p.to_path_buf(), md.is_dir()));
            }
        });
    }
    found
}

/// Archivos modificados más recientemente.
pub fn recent(roots: &[PathBuf], limit: usize) -> Vec<(PathBuf, SystemTime)> {
    let mut all: Vec<(PathBuf, SystemTime)> = Vec::new();
    let mut budget = 40_000;
    for root in roots {
        walk(root, 4, &mut budget, &mut |p, md, _| {
            if md.is_file() {
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if !name.starts_with('.') && !name.starts_with("~$") && !name.ends_with(".tmp") {
                    if let Ok(m) = md.modified() {
                        all.push((p.to_path_buf(), m));
                    }
                }
            }
        });
    }
    all.sort_by_key(|e| std::cmp::Reverse(e.1));
    all.truncate(limit);
    all
}

pub fn read_text(path: &Path) -> Result<String, String> {
    let md = fs::metadata(path).map_err(|e| format!("No pude leer el archivo: {e}"))?;
    if md.is_dir() {
        return Err("Es una carpeta, no un archivo.".into());
    }
    if md.len() > MAX_READ_BYTES {
        return Err(format!(
            "El archivo es demasiado grande ({} KB; máximo {} KB).",
            md.len() / 1024,
            MAX_READ_BYTES / 1024
        ));
    }
    let bytes = fs::read(path).map_err(|e| format!("No pude leer el archivo: {e}"))?;
    if bytes.iter().take(8192).any(|b| *b == 0) {
        return Err("Es un archivo binario (imagen, programa, etc.); sólo puedo leer texto.".into());
    }
    String::from_utf8(bytes).map_err(|_| "El archivo no es texto UTF-8.".into())
}

// --------------------------------------------------------------- escritura

pub fn unified_diff(old: &str, new: &str, max_lines: usize) -> String {
    let diff = similar::TextDiff::from_lines(old, new);
    let text = diff.unified_diff().context_radius(3).header("antes", "después").to_string();
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() > max_lines {
        format!("{}\n… ({} líneas más)", lines[..max_lines].join("\n"), lines.len() - max_lines)
    } else {
        text
    }
}

pub fn write_with_backup(path: &Path, content: &str, backups: &Path) -> Result<Option<PathBuf>, String> {
    let mut backup = None;
    if path.exists() {
        fs::create_dir_all(backups).map_err(|e| format!("No pude crear la carpeta de copias: {e}"))?;
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "archivo".into());
        let dest = backups.join(format!("{stamp}_{name}"));
        fs::copy(path, &dest).map_err(|e| format!("No pude crear la copia de seguridad: {e}"))?;
        backup = Some(dest);
    } else if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("No pude crear la carpeta: {e}"))?;
    }
    fs::write(path, content).map_err(|e| format!("No pude escribir el archivo: {e}"))?;
    Ok(backup)
}

// -------------------------------------------------------------- organizar

pub fn category(path: &Path) -> &'static str {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" | "heic" | "ico" | "tif" | "tiff" => "Imágenes",
        "pdf" | "doc" | "docx" | "odt" | "rtf" | "txt" | "md" | "xls" | "xlsx" | "ods" | "csv" | "ppt" | "pptx" | "odp" | "epub" => {
            "Documentos"
        }
        "mp4" | "mkv" | "mov" | "avi" | "webm" | "wmv" => "Videos",
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" => "Música",
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" => "Comprimidos",
        "exe" | "msi" | "msix" | "appx" => "Instaladores",
        "js" | "ts" | "tsx" | "jsx" | "py" | "rs" | "java" | "c" | "cpp" | "cs" | "go" | "rb" | "php" | "html" | "css" | "json" | "sql"
        | "sh" | "ipynb" => "Código",
        "ttf" | "otf" | "woff" | "woff2" => "Fuentes",
        _ => "Otros",
    }
}

/// Plan para organizar los archivos sueltos (sólo el primer nivel) por tipo.
pub fn organize_plan(dir: &Path) -> Result<Vec<(PathBuf, &'static str)>, String> {
    let mut plan = Vec::new();
    for e in fs::read_dir(dir)
        .map_err(|e| format!("No pude abrir la carpeta: {e}"))?
        .filter_map(Result::ok)
    {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_file()
            && !name.starts_with('.')
            && !name.ends_with(".crdownload")
            && !name.ends_with(".part")
            && !name.eq_ignore_ascii_case("desktop.ini")
        {
            plan.push((p.clone(), category(&p)));
        }
    }
    plan.sort_by(|a, b| a.1.cmp(b.1).then_with(|| a.0.cmp(&b.0)));
    Ok(plan)
}

fn unique_dest(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let p = Path::new(name);
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (2..1000)
        .map(|i| dir.join(format!("{stem} ({i}){ext}")))
        .find(|c| !c.exists())
        .unwrap_or(candidate)
}

/// Mueve los archivos según el plan. Nunca sobrescribe ni borra.
pub fn organize_apply(dir: &Path, plan: &[(PathBuf, &'static str)]) -> (usize, Vec<String>) {
    let mut moved = 0;
    let mut errors = Vec::new();
    for (src, cat) in plan {
        let target_dir = dir.join(cat);
        if let Err(e) = fs::create_dir_all(&target_dir) {
            errors.push(format!("{cat}: {e}"));
            continue;
        }
        let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        match fs::rename(src, unique_dest(&target_dir, &name)) {
            Ok(()) => moved += 1,
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }
    (moved, errors)
}

// ----------------------------------------------------- análisis de proyecto

const KEY_FILES: [&str; 16] = [
    "README.md",
    "readme.md",
    "package.json",
    "tsconfig.json",
    "vite.config.ts",
    "Cargo.toml",
    "pyproject.toml",
    "requirements.txt",
    "setup.py",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "composer.json",
    "Gemfile",
    "index.html",
    ".gitignore",
];

/// Estructura (hasta 300 entradas) + archivos clave (hasta ~60 KB) de un proyecto.
pub fn project_snapshot(dir: &Path, progress: &dyn Fn(String)) -> Result<String, String> {
    if !dir.is_dir() {
        return Err("No es una carpeta.".into());
    }
    let mut tree = Vec::new();
    let mut budget = 20_000;
    let mut count = 0usize;
    walk(dir, 6, &mut budget, &mut |p, md, depth| {
        count += 1;
        if tree.len() < 300 {
            let rel = p.strip_prefix(dir).map(|r| r.to_string_lossy().to_string()).unwrap_or_default();
            tree.push(format!("{}{}{}", "  ".repeat(depth), rel, if md.is_dir() { "/" } else { "" }));
        }
    });
    progress(format!("{count} elementos revisados"));
    tree.sort();
    let mut out = format!(
        "Proyecto: {}\n\nEstructura ({} elementos, se muestran {}):\n{}\n",
        display(dir),
        count,
        tree.len(),
        tree.join("\n")
    );
    let mut used = 0usize;
    for name in KEY_FILES {
        let p = dir.join(name);
        if let Ok(text) = read_text(&p) {
            let take: String = text.chars().take(12_000).collect();
            used += take.len();
            out.push_str(&format!("\n--- {name} ---\n{take}\n"));
            if used > 60_000 {
                break;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("canela-fs-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn scope_blocks_paths_outside_and_traversal() {
        let root = tmp("scope");
        let outside = tmp("outside");
        fs::write(outside.join("secreto.txt"), "x").unwrap();
        let scope = Scope::new(vec![root.clone()], vec![("descargas".into(), root.clone())]);
        assert!(scope.existing(&outside.join("secreto.txt").to_string_lossy()).is_err());
        assert!(scope.target("descargas/../../etc").is_err());
        assert!(scope.target(&format!("{}/nueva/carpeta", root.display())).is_ok());
        assert!(scope.existing("Descargas").is_ok());
        assert!(scope.existing("mis descargas").is_ok());
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_cannot_escape() {
        let root = tmp("link");
        let outside = tmp("link-out");
        fs::write(outside.join("x.txt"), "x").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("atajo")).unwrap();
        let scope = Scope::new(vec![root.clone()], vec![]);
        assert!(scope.existing(&root.join("atajo/x.txt").to_string_lossy()).is_err());
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
    }

    #[test]
    fn search_recent_and_read() {
        let root = tmp("search");
        fs::create_dir_all(root.join("proyectos/mi-app-react/src")).unwrap();
        fs::create_dir_all(root.join("proyectos/mi-app-react/node_modules/react")).unwrap();
        fs::write(
            root.join("proyectos/mi-app-react/package.json"),
            r#"{"name":"mi-app","dependencies":{"react":"19"}}"#,
        )
        .unwrap();
        fs::write(root.join("Tarea Universidad.docx"), [0u8, 1, 2]).unwrap();
        let found = search(std::slice::from_ref(&root), "react", Some(true), 10);
        assert_eq!(found.len(), 1, "no debe entrar en node_modules");
        assert!(found[0].0.ends_with("mi-app-react"));
        assert_eq!(search(std::slice::from_ref(&root), "universidad", None, 10).len(), 1);
        assert!(!recent(std::slice::from_ref(&root), 5).is_empty());
        assert!(read_text(&root.join("proyectos/mi-app-react/package.json"))
            .unwrap()
            .contains("react"));
        assert!(read_text(&root.join("Tarea Universidad.docx")).is_err());
        let snap = project_snapshot(&root.join("proyectos/mi-app-react"), &|_| {}).unwrap();
        assert!(snap.contains("package.json"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn write_creates_backup_and_diff() {
        let root = tmp("write");
        let f = root.join("notas.md");
        assert!(write_with_backup(&f, "hola\n", &root.join("bak")).unwrap().is_none());
        let bak = write_with_backup(&f, "hola\nmundo\n", &root.join("bak")).unwrap().unwrap();
        assert_eq!(fs::read_to_string(bak).unwrap(), "hola\n");
        let d = unified_diff("hola\n", "hola\nmundo\n", 50);
        assert!(d.contains("+mundo"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn organize_moves_without_overwriting() {
        let root = tmp("org");
        fs::write(root.join("foto.jpg"), "a").unwrap();
        fs::write(root.join("informe.pdf"), "b").unwrap();
        fs::write(root.join("raro.xyz"), "c").unwrap();
        fs::create_dir_all(root.join("Imágenes")).unwrap();
        fs::write(root.join("Imágenes/foto.jpg"), "ya existía").unwrap();
        let plan = organize_plan(&root).unwrap();
        assert_eq!(plan.len(), 3);
        let (moved, errors) = organize_apply(&root, &plan);
        assert_eq!((moved, errors.len()), (3, 0));
        assert_eq!(fs::read_to_string(root.join("Imágenes/foto.jpg")).unwrap(), "ya existía");
        assert!(root.join("Imágenes/foto (2).jpg").exists());
        assert!(root.join("Documentos/informe.pdf").exists());
        assert!(root.join("Otros/raro.xyz").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn blocked_extensions() {
        assert!(is_blocked(Path::new("virus.EXE")));
        assert!(is_blocked(Path::new("script.ps1")));
        assert!(!is_blocked(Path::new("main.py")));
    }
}
